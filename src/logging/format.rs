use tracing::{Event, Subscriber};
use tracing_subscriber::{
    field::RecordFields,
    fmt::{
        self, FmtContext, FormatEvent, FormatFields,
        format::{DefaultFields, JsonFields, Writer},
        time::FormatTime,
    },
    registry::LookupSpan,
};

use crate::infrastructure::config::LogFormat;

/// 按配置在文本与 JSON 之间二选一的事件格式化器。
///
/// `fmt::format()` 的返回类型由字段格式化器决定（`Full` / `Json`），
/// 无法在同一个函数返回类型里二选一，所以用枚举承载。
pub enum LogFormatter<T: FormatTime> {
    Text(fmt::format::Format<fmt::format::Full, T>),
    Json(fmt::format::Format<fmt::format::Json, T>),
}

impl<T: FormatTime> LogFormatter<T> {
    pub fn new(format: LogFormat, timer: T) -> Self {
        let base = fmt::format().with_ansi(false).with_timer(timer);

        match format {
            // JSON 单行输出：事件字段平铺到顶层，span 字段（method/route/request_id/client_ip）
            // 保留在 `span` 对象中，便于按字段检索。
            LogFormat::Json => Self::Json(base.json().flatten_event(true)),
            LogFormat::Text => Self::Text(base),
        }
    }
}

impl<S, N, T> FormatEvent<S, N> for LogFormatter<T>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatTime,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Text(inner) => inner.format_event(ctx, writer, event),
            Self::Json(inner) => inner.format_event(ctx, writer, event),
        }
    }
}

/// 把内层格式化器的输出先聚合成一行再交给底层 writer。
///
/// tracing 的 fmt 层对一条事件会多次调用 writer（时间戳、级别、字段等分段写入），
/// 直接计数会失真；这里先格式化为完整一行，保证每次事件恰好触发一次 `write`，
/// 分批写入器才能以「条数」精确触发落盘，控制台也不会出现多线程交错写入。
pub struct LineFormat<F>(F);

impl<F> LineFormat<F> {
    pub fn new(inner: F) -> Self {
        Self(inner)
    }
}

impl<S, N, F> FormatEvent<S, N> for LineFormat<F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    F: FormatEvent<S, N>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let mut line = String::new();
        self.0.format_event(ctx, Writer::new(&mut line), event)?;
        let line = match line.strip_suffix('\n') {
            Some(line) => line,
            None => &line,
        };
        writer.write_str(line)?;
        writer.write_str("\n")
    }
}

/// 给字段格式化器加一个「身份标记」。
///
/// tracing 的 fmt 层把 span 字段格式化结果缓存在 span 扩展里，缓存键就是这个类型。
/// 两层（控制台与文件）若共用同一个字段格式化器类型，就会互相读到对方的缓存：
/// 控制台带 ANSI 的文本会被文件层的 JSON 格式化器当成 JSON 解析而 panic。
/// 用不同的标记类型让两层各持一份缓存，互不干扰。
pub struct IsolatedFields<M, F>(F, std::marker::PhantomData<M>);

impl<M, F> IsolatedFields<M, F> {
    fn new(inner: F) -> Self {
        Self(inner, std::marker::PhantomData)
    }
}

impl<'a, M, F> FormatFields<'a> for IsolatedFields<M, F>
where
    M: 'static,
    F: FormatFields<'a> + 'static,
{
    fn format_fields<R: RecordFields>(&self, writer: Writer<'a>, fields: R) -> std::fmt::Result {
        self.0.format_fields(writer, fields)
    }
}

/// 字段格式化器必须与事件格式化器配套：JSON 事件依赖 span 字段也是 JSON，
/// 否则 JSON 格式化器会把文本字段当 JSON 解析（见 `IsolatedFields` 的说明）。
pub enum LogFields<M> {
    Text(IsolatedFields<M, DefaultFields>),
    Json(IsolatedFields<M, JsonFields>),
}

impl<M> LogFields<M> {
    pub fn new(format: LogFormat) -> Self {
        match format {
            LogFormat::Json => Self::Json(IsolatedFields::new(JsonFields::new())),
            LogFormat::Text => Self::Text(IsolatedFields::new(DefaultFields::new())),
        }
    }
}

impl<'a, M> FormatFields<'a> for LogFields<M>
where
    M: 'static,
{
    fn format_fields<R: RecordFields>(&self, writer: Writer<'a>, fields: R) -> std::fmt::Result {
        match self {
            Self::Text(inner) => inner.format_fields(writer, fields),
            Self::Json(inner) => inner.format_fields(writer, fields),
        }
    }
}

/// 文件层的缓存标记。
pub struct FileFieldsMarker;

/// 控制台层的缓存标记。
pub struct ConsoleFieldsMarker;
