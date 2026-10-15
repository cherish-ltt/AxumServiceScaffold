use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use time::OffsetDateTime;

use crate::infrastructure::config::{LogRotation, LoggingConfig};

/// 单个日志文件的活动句柄。
struct ActiveFile {
    path: PathBuf,
    file: File,
    bytes: u64,
    /// 当前文件所属的轮转周期时间戳；`None` 表示 `LogRotation::Never`。
    period: Option<String>,
}

/// 自主管理的日志文件写入器。
///
/// 与 `tracing-appender` 的 `RollingFileAppender` 相比，这里额外提供单文件大小上限：
/// 时间轮转 + 文件数量保留只能限制「保留多少个文件」，无法限制「单个文件多大」，
/// 持续过载下单文件仍可涨到 GB 级。
pub struct LogFileWriter {
    dir: PathBuf,
    prefix: String,
    suffix: String,
    rotation: LogRotation,
    max_file_bytes: u64,
    max_files: usize,
    current: Option<ActiveFile>,
    /// 同一周期内多次轮转时的序号，保证归档文件名不冲突。
    sequence: u32,
    /// 轮转/清理失败的原因；不阻断写入路径，由外层在持锁之外上报。
    maintenance_error: Option<io::Error>,
}

impl LogFileWriter {
    pub fn new(config: &LoggingConfig) -> Result<Self> {
        let dir = PathBuf::from(&config.out_dir);
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create log directory {}", dir.display()))?;

        Ok(Self {
            dir,
            prefix: config.filename_prefix.clone(),
            suffix: config.filename_suffix.clone(),
            rotation: config.rotation,
            max_file_bytes: config.max_file_bytes,
            max_files: config.max_log_files,
            current: None,
            sequence: 0,
            maintenance_error: None,
        })
    }

    /// 写入一段日志字节，必要时先轮转。
    ///
    /// 轮转失败不会转成写入失败：数据已经落盘，若让调用方重试同一批内容会产生重复行。
    /// 失败原因记在 `maintenance_error` 里，由外层在持锁路径之外上报。
    pub fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.ensure_current()?;
        let active = self
            .current
            .as_mut()
            .expect("ensure_current 会打开文件，此处必然存在");
        active.file.write_all(buf)?;
        active.bytes += buf.len() as u64;
        if active.bytes >= self.max_file_bytes
            && let Err(error) = self.rotate()
        {
            self.maintenance_error.get_or_insert(error);
        }

        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        match self.current.as_mut() {
            Some(active) => active.file.flush(),
            None => Ok(()),
        }
    }

    /// 取走一次轮转/清理失败的原因，避免重复上报。
    pub fn take_maintenance_error(&mut self) -> Option<io::Error> {
        self.maintenance_error.take()
    }

    /// 打开当前周期的文件；周期变化时先归档旧文件。
    fn ensure_current(&mut self) -> io::Result<()> {
        let period = self.rotation.timestamp(OffsetDateTime::now_utc());
        if let Some(active) = &self.current
            && active.period == period
        {
            return Ok(());
        }

        self.close_current()?;
        let path = self.active_path(period.as_deref());
        let file = open_append(&path)?;
        let bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        self.current = Some(ActiveFile {
            path,
            file,
            bytes,
            period,
        });

        // 新周期开始时顺手清理超量文件，避免只在退出时清理。
        if let Err(error) = self.prune() {
            self.maintenance_error.get_or_insert(error);
        }

        Ok(())
    }

    /// 关闭当前文件并把它转成「已归档」名字。
    fn close_current(&mut self) -> io::Result<()> {
        let Some(mut active) = self.current.take() else {
            return Ok(());
        };
        active.file.flush()?;
        drop(active.file);

        let archived = self.archived_path(active.period.as_deref());
        if archived != active.path {
            fs::rename(&active.path, &archived)?;
        }

        Ok(())
    }

    /// 单文件达到大小上限：归档当前文件，下一次写入重新开一个新文件。
    fn rotate(&mut self) -> io::Result<()> {
        self.close_current()?;
        self.prune()
    }

    /// 活动文件名：`<prefix>.<周期时间戳>.<suffix>`，`Never` 时为 `<prefix>.<suffix>`。
    fn active_path(&self, period: Option<&str>) -> PathBuf {
        match period {
            Some(period) => self
                .dir
                .join(format!("{}.{}.{}", self.prefix, period, self.suffix)),
            None => self.dir.join(format!("{}.{}", self.prefix, self.suffix)),
        }
    }

    /// 归档文件名：在周期时间戳后追加序号，避免与活动文件重名。
    fn archived_path(&mut self, period: Option<&str>) -> PathBuf {
        let stamp = period.unwrap_or("archive");
        loop {
            self.sequence += 1;
            let candidate = self.dir.join(format!(
                "{}.{}-{}.{}",
                self.prefix, stamp, self.sequence, self.suffix
            ));
            if !candidate.exists() {
                return candidate;
            }
        }
    }

    /// 只保留最近 `max_files` 个已归档文件（活动文件不计入）。
    fn prune(&self) -> io::Result<()> {
        let active_name = self
            .current
            .as_ref()
            .and_then(|active| active.path.file_name().map(|name| name.to_owned()));

        let mut archived: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&self.dir)?
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter(|entry| self.matches_own_file(&entry.file_name()))
            .filter(|entry| Some(entry.file_name()) != active_name)
            .filter_map(|entry| {
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((modified, entry.path()))
            })
            .collect();

        if archived.len() <= self.max_files {
            return Ok(());
        }
        archived.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for (_, path) in archived.into_iter().skip(self.max_files) {
            fs::remove_file(path)?;
        }

        Ok(())
    }

    /// 文件名是否由本写入器生成（避免误删同目录下的其他文件）。
    fn matches_own_file(&self, name: &std::ffi::OsStr) -> bool {
        let Some(name) = name.to_str() else {
            return false;
        };

        name.starts_with(&format!("{}.", self.prefix))
            && name.ends_with(&format!(".{}", self.suffix))
    }

    /// 当前活动文件路径，供测试断言。
    pub fn current_file(&self) -> Option<PathBuf> {
        self.current.as_ref().map(|active| active.path.clone())
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}
