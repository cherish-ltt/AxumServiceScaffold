use std::{env, str::FromStr};

use anyhow::{Context, Result, anyhow};

/// 读取必填环境变量。
pub(super) fn get_required_env(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("missing required environment variable: {key}"))
}

/// 读取环境变量，缺失时使用默认值。
pub(super) fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// 读取并解析环境变量，缺失时使用默认值；解析失败直接报错，不做静默回退。
pub(super) fn parse_env_or<T>(key: &str, default: T) -> Result<T>
where
    T: FromStr + Clone,
    <T as FromStr>::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(value) => value
            .parse::<T>()
            .map_err(|error| anyhow!("failed to parse environment variable {key}: {error}")),
        Err(_) => Ok(default),
    }
}

/// 把逗号分隔的列表解析成去空格后的条目，非法条目直接拒绝启动。
pub(super) fn parse_list(
    raw: &str,
    what: &str,
    validate: impl Fn(&str) -> Result<()>,
) -> Result<Vec<String>> {
    let mut items = Vec::new();
    for item in raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        validate(item).map_err(|error| anyhow!("invalid {what} entry {item}: {error}"))?;
        items.push(item.to_string());
    }

    Ok(items)
}

/// 把逗号分隔的列表解析成解析后的类型，非法条目直接拒绝启动。
pub(super) fn parse_items<T>(raw: &str, what: &str) -> Result<Vec<T>>
where
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Display,
{
    let mut items = Vec::new();
    for item in raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let parsed = item
            .parse::<T>()
            .map_err(|error| anyhow!("invalid {what} entry {item}: {error}"))?;
        items.push(parsed);
    }

    Ok(items)
}
