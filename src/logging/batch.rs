use std::{
    io::{self, Write},
    mem,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::logging::file_writer::LogFileWriter;

/// 文件日志分批写入器：事件先进入内存缓冲，满 `batch_max_events` 条或每
/// `flush_interval` 秒写一次文件，降低高频场景下逐条写文件的 CPU 开销。
///
/// 同时承担写入限速：超过 `max_events_per_sec` 的日志行被丢弃并计数，
/// 计数在窗口切换时以一条汇总行写回日志，保证「丢了多少」可查。
#[derive(Clone)]
pub struct BatchFileWriter {
    inner: Arc<Mutex<LogFileWriter>>,
    state: Arc<Mutex<BatchState>>,
    batch_max_events: usize,
    flush_interval: Duration,
}

/// 批量缓冲状态与限速计数。
struct BatchState {
    buf: Vec<u8>,
    events: usize,
    /// 限速窗口起点；`None` 表示还没写入过。
    window_start: Option<Instant>,
    window_events: u64,
    window_dropped: u64,
    max_events_per_sec: u64,
}

impl BatchState {
    fn new(max_events_per_sec: u64) -> Self {
        Self {
            buf: Vec::new(),
            events: 0,
            window_start: None,
            window_events: 0,
            window_dropped: 0,
            max_events_per_sec,
        }
    }

    /// 记录一次写入尝试，返回是否允许写入。
    fn admit(&mut self, now: Instant) -> bool {
        if self.max_events_per_sec == 0 {
            return true;
        }
        let window_start = *self.window_start.get_or_insert(now);
        if now.duration_since(window_start) >= Duration::from_secs(1) {
            self.window_start = Some(now);
            self.window_events = 0;
            if self.window_dropped > 0 {
                self.buf
                    .extend_from_slice(summary_line(self.window_dropped).as_bytes());
                // 汇总行本身不参与事件计数：它由限速器生成，不是业务日志。
                self.window_dropped = 0;
            }
        }
        if self.window_events >= self.max_events_per_sec {
            self.window_dropped += 1;
            return false;
        }
        self.window_events += 1;

        true
    }
}

/// 限速丢弃汇总行：写入器拿不到配置的日志时区，统一用 UTC 时间戳。
fn summary_line(dropped: u64) -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z WARN log rate limit reached: dropped {} events in the last second\n",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        dropped
    )
}

impl BatchFileWriter {
    pub fn new(
        inner: LogFileWriter,
        batch_max_events: usize,
        flush_interval: Duration,
        max_events_per_sec: u64,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
            state: Arc::new(Mutex::new(BatchState::new(max_events_per_sec))),
            batch_max_events,
            flush_interval,
        }
    }

    /// 把缓冲内容一次性写入文件并清空。
    ///
    /// 磁盘 IO 在锁外进行：先在锁内取走缓冲并重置计数，再释放锁写文件。
    /// 否则定时刷盘或满批落盘时，所有写日志的线程会被写盘时间串行阻塞。
    fn flush(&self) -> io::Result<()> {
        let (mut pending, pending_events) = {
            let mut state = self.lock_state();
            if state.buf.is_empty() {
                return Ok(());
            }
            let pending_events = mem::replace(&mut state.events, 0);
            (mem::take(&mut state.buf), pending_events)
        };

        let write_result = {
            let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            inner.write_all(&pending).and_then(|()| inner.flush())
        };

        if let Err(error) = write_result {
            // 写盘失败时把内容放回缓冲队首，避免丢日志。
            let mut state = self.lock_state();
            pending.extend_from_slice(&state.buf);
            state.buf = pending;
            state.events += pending_events;
            return Err(error);
        }

        Ok(())
    }

    /// 上报（并清空）轮转/清理失败的原因；必须在没有持有 state 锁时调用。
    fn report_maintenance_error(&self) {
        let error = {
            let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            inner.take_maintenance_error()
        };
        if let Some(error) = error {
            tracing::error!("log file rotation failed: {error}");
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, BatchState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// 启动后台定时刷盘线程，返回持有停止信号与线程句柄的 guard。
    pub(super) fn start_flusher(&self) -> BatchGuard {
        let (stop, rx) = std::sync::mpsc::channel();
        let flusher = self.clone();
        let interval = flusher.flush_interval;
        let thread = thread::spawn(move || {
            while rx.recv_timeout(interval).is_err() {
                let _ = flusher.flush();
                flusher.report_maintenance_error();
            }
        });
        BatchGuard {
            stop: Some(stop),
            thread: Some(thread),
            writer: self.clone(),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BatchFileWriter {
    type Writer = BatchWriter;

    fn make_writer(&self) -> Self::Writer {
        BatchWriter(self.clone())
    }
}

/// 每次事件由 fmt 层创建一次；`write` 追加到共享缓冲，满 `batch_max_events` 条时落盘。
pub struct BatchWriter(BatchFileWriter);

impl Write for BatchWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let flush = {
            let mut state = self.0.lock_state();
            if !state.admit(Instant::now()) {
                // 被限速丢弃：告知调用方写入成功，避免 tracing 反复重试同一行。
                return Ok(buf.len());
            }
            state.buf.extend_from_slice(buf);
            state.events += 1;
            state.events >= self.0.batch_max_events
        };
        if flush {
            // 这里不调用 report_maintenance_error：该路径处于 tracing fmt 层的写入过程中，
            // 在 on_event 里再次触发事件有重入风险；定时刷盘线程会在一个刷盘周期内上报。
            self.0.flush()?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// `init` 返回的守卫：drop 时停止定时刷盘线程并落盘剩余缓冲。
pub struct BatchGuard {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    writer: BatchFileWriter,
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Err(error) = self.writer.flush() {
            tracing::error!("failed to flush logs on guard drop: {error}");
        }
        self.writer.report_maintenance_error();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_up_to_capacity_then_counts_drops() {
        let mut state = BatchState::new(2);
        let start = Instant::now();

        assert!(state.admit(start));
        assert!(state.admit(start));
        assert!(!state.admit(start));
        assert!(!state.admit(start));
        assert_eq!(state.window_dropped, 2);

        // 窗口切换后重新放行，并把丢弃条数以汇总行写回缓冲
        let next = start + Duration::from_secs(1);
        assert!(state.admit(next));
        let text = String::from_utf8_lossy(&state.buf).to_string();
        assert!(text.contains("dropped 2 events"), "实际内容: {text}");
        assert_eq!(state.window_dropped, 0);
    }

    #[test]
    fn limiter_disabled_when_zero() {
        let mut state = BatchState::new(0);
        let start = Instant::now();

        for _ in 0..1000 {
            assert!(state.admit(start));
        }
        assert_eq!(state.window_dropped, 0);
    }
}
