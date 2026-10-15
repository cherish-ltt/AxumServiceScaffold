use anyhow::{Result, anyhow};
use time::{OffsetDateTime, macros::format_description};

use super::env::{get_env_or, parse_env_or};

/// 日志输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// 人类可读的单行文本（默认），用于本地开发。
    Text,
    /// 单行 JSON，字段可被日志平台直接解析检索。
    Json,
}

/// 日志文件轮转粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogRotation {
    Minutely,
    Hourly,
    Daily,
    Weekly,
    Never,
}

impl LogRotation {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "MINUTELY" | "Rotation::MINUTELY" => Ok(Self::Minutely),
            "HOURLY" | "Rotation::HOURLY" => Ok(Self::Hourly),
            "DAILY" | "Rotation::DAILY" => Ok(Self::Daily),
            "WEEKLY" | "Rotation::WEEKLY" => Ok(Self::Weekly),
            "NEVER" | "Rotation::NEVER" => Ok(Self::Never),
            value => Err(anyhow!("unsupported log rotation strategy: {value}")),
        }
    }

    /// 当前轮转周期的时间戳后缀，`Never` 表示文件名不带时间戳。
    ///
    /// 命名与 tracing-appender 的 `Rotation` 保持一致，便于既有日志采集规则沿用。
    pub fn timestamp(&self, now: OffsetDateTime) -> Option<String> {
        let format = match self {
            Self::Minutely => {
                format_description!("[year]-[month]-[day]-[hour]-[minute]")
            },
            Self::Hourly => format_description!("[year]-[month]-[day]-[hour]"),
            Self::Daily | Self::Weekly => format_description!("[year]-[month]-[day]"),
            Self::Never => return None,
        };
        let stamp = match self {
            Self::Weekly => {
                let offset = now.weekday().number_days_from_monday() as i64;
                now - time::Duration::days(offset)
            },
            _ => now,
        };

        Some(
            stamp
                .format(format)
                .unwrap_or_else(|_| stamp.unix_timestamp().to_string()),
        )
    }
}

/// 日志配置。
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub filter: String,
    pub format: LogFormat,
    pub utc_offset_hour: i8,
    pub utc_offset_minute: i8,
    pub utc_offset_second: i8,
    pub filename_prefix: String,
    pub filename_suffix: String,
    pub rotation: LogRotation,
    pub max_log_files: usize,
    /// 单个日志文件的大小上限（字节），超过后立即轮转出新文件。
    pub max_file_bytes: u64,
    /// 文件日志每秒最多写入的事件条数，0 表示不限速；超出部分丢弃并统计。
    pub max_events_per_sec: u64,
    pub out_dir: String,
    /// 文件日志批量落盘：缓冲满 N 条触发一次写入。
    pub batch_max_events: usize,
    /// 文件日志批量落盘：最长 N 秒强制写入一次。
    pub batch_flush_interval_secs: u64,
}

impl LoggingConfig {
    pub fn from_env() -> Result<Self> {
        let config = Self {
            filter: get_env_or("LOG_FILTER", "info,tower_http=info"),
            format: match get_env_or("LOG_FORMAT", "text")
                .to_ascii_lowercase()
                .as_str()
            {
                "text" => LogFormat::Text,
                "json" => LogFormat::Json,
                value => return Err(anyhow!("unsupported LOG_FORMAT: {value}")),
            },
            utc_offset_hour: parse_env_or("LOG_UTC_OFFSET_HOUR", 0_i8)?,
            utc_offset_minute: parse_env_or("LOG_UTC_OFFSET_MINUTE", 0_i8)?,
            utc_offset_second: parse_env_or("LOG_UTC_OFFSET_SECOND", 0_i8)?,
            filename_prefix: get_env_or("LOG_FILENAME_PREFIX", "app"),
            filename_suffix: get_env_or("LOG_FILENAME_SUFFIX", "log"),
            rotation: LogRotation::parse(&get_env_or("LOG_ROTATION", "DAILY"))?,
            max_log_files: parse_env_or("LOG_MAX_LOG_FILES", 60_usize)?,
            max_file_bytes: parse_env_or("LOG_MAX_FILE_BYTES", 64 * 1024 * 1024_u64)?,
            max_events_per_sec: parse_env_or("LOG_MAX_EVENTS_PER_SEC", 2000_u64)?,
            out_dir: get_env_or("LOG_OUT_DIR", "./log"),
            batch_max_events: parse_env_or("LOG_BATCH_MAX_EVENTS", 50_usize)?,
            batch_flush_interval_secs: parse_env_or("LOG_BATCH_FLUSH_INTERVAL_SECS", 3_u64)?,
        };
        config.validate()?;

        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.max_log_files == 0 {
            return Err(anyhow!("LOG_MAX_LOG_FILES must be greater than 0"));
        }
        if self.max_file_bytes == 0 {
            return Err(anyhow!("LOG_MAX_FILE_BYTES must be greater than 0"));
        }
        if self.batch_max_events == 0 || self.batch_flush_interval_secs == 0 {
            return Err(anyhow!("log batch parameters must be greater than 0"));
        }
        if self.filename_prefix.is_empty() || self.filename_suffix.is_empty() {
            return Err(anyhow!(
                "LOG_FILENAME_PREFIX and LOG_FILENAME_SUFFIX must not be empty"
            ));
        }
        // 时间戳后缀参与文件名拼接，含路径分隔符会写出目录之外的文件。
        if self.filename_prefix.contains('/') || self.filename_suffix.contains('/') {
            return Err(anyhow!(
                "LOG_FILENAME_PREFIX and LOG_FILENAME_SUFFIX must not contain '/'"
            ));
        }

        Ok(())
    }
}
