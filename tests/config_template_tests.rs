//! 配置模板一致性：`.env-example` 入库供使用者照抄，一旦与代码读取的变量脱节，
//! 复制出来的 `.env` 会静默缺失配置。这里用「代码读取全集 == 模板生效项」把口径钉死，
//! 替代文档里手写的配置项数量。

use std::collections::BTreeSet;

const CONFIG_SOURCE: &str = include_str!("../src/infrastructure/config.rs");
const TEMPLATE: &str = include_str!("../.env-example");

/// 代码里 `parse_env_or` / `get_env_or` / `get_required_env` 的第一个参数即变量名。
fn keys_read_by_code() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for function in ["get_required_env(", "get_env_or(", "parse_env_or("] {
        let mut rest = CONFIG_SOURCE;
        while let Some(offset) = rest.find(function) {
            // 先跳过空白：形如 `parse_env_or(\n    "KEY",` 的跨行调用同样是字面量。
            let argument = rest[offset + function.len()..].trim_start();
            if let Some(literal) = argument.strip_prefix('"')
                && let Some(close) = literal.find('"')
            {
                keys.insert(literal[..close].to_string());
            }
            rest = argument;
        }
    }
    keys
}

/// 模板中生效（未注释）的 `KEY=` 行；注释行是可选示例，不参与相等比较。
fn keys_in_template() -> (BTreeSet<String>, Vec<String>) {
    let mut keys = BTreeSet::new();
    let mut duplicated = Vec::new();
    for line in TEMPLATE.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        if !keys.insert(key.to_string()) {
            duplicated.push(key.to_string());
        }
    }
    (keys, duplicated)
}

/// 以 `# KEY=...` 形式给出的可选项：保留说明但默认不生效（如跟随 APP_ENV 的 HSTS）。
fn commented_keys_in_template() -> BTreeSet<String> {
    TEMPLATE
        .lines()
        .filter_map(|line| line.trim().strip_prefix('#'))
        .filter_map(|line| line.trim().split_once('='))
        .map(|(key, _)| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect()
}

fn difference(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}

#[test]
fn every_config_key_is_documented_in_the_template() {
    let code = keys_read_by_code();
    let (template, _) = keys_in_template();
    let commented = commented_keys_in_template();

    assert!(
        !code.is_empty(),
        "没有从 config.rs 解析出任何配置键，解析逻辑已失效"
    );

    // 既没生效也没以注释形式给出，等于使用者照抄模板时会静默缺失。
    let mut documented = template.clone();
    documented.extend(commented.iter().cloned());
    assert_eq!(
        difference(&code, &documented),
        Vec::<String>::new(),
        "config.rs 读取但 .env-example 完全没有提及"
    );
}

#[test]
fn only_deliberately_optional_keys_are_commented_out() {
    // HSTS 默认跟随 APP_ENV，写死会覆盖生产环境的默认值，因此以注释形式给出。
    let expected = BTreeSet::from(["MIDDLEWARE_HSTS_ENABLED".to_string()]);

    assert_eq!(
        difference(&commented_keys_in_template(), &expected),
        Vec::<String>::new(),
        "新增了未生效的注释配置项，需确认它确实是可选项"
    );
}

#[test]
fn template_has_no_key_the_code_never_reads() {
    let code = keys_read_by_code();
    let (template, _) = keys_in_template();

    assert_eq!(
        difference(&template, &code),
        Vec::<String>::new(),
        ".env-example 提供了代码从不读取的配置项（无用的假开关）"
    );
}

#[test]
fn template_does_not_repeat_a_key() {
    let (_, duplicated) = keys_in_template();

    assert!(
        duplicated.is_empty(),
        ".env-example 重复定义了同一个配置项: {duplicated:?}"
    );
}
