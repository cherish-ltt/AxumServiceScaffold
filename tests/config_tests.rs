use std::env;

use axum_service_scaffold::infrastructure::config::AppConfig;

fn restore(key: &str, previous: Option<String>) {
    match previous {
        Some(value) => unsafe { env::set_var(key, value) },
        None => unsafe { env::remove_var(key) },
    }
}

#[test]
fn production_rejects_example_jwt_secret() {
    let keys = ["APP_ENV", "JWT_SECRET"];
    let previous: Vec<_> = keys.iter().map(|key| env::var(key).ok()).collect();

    unsafe {
        env::set_var("APP_ENV", "production");
        env::set_var(
            "JWT_SECRET",
            "change-me-to-a-random-string-with-at-least-32-characters",
        );
    }

    let result = AppConfig::from_env();

    for (key, value) in keys.iter().zip(previous) {
        restore(key, value);
    }

    assert!(result.is_err());
}

#[test]
fn invalid_pool_settings_are_rejected() {
    let key = "DATABASE_MIN_CONNECTIONS";
    let previous = env::var(key).ok();
    unsafe { env::set_var(key, "11") };

    let result = AppConfig::from_env();
    restore(key, previous);

    assert!(result.is_err());
}
