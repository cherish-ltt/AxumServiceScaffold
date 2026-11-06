use axum_service_scaffold::infrastructure::{
    config::DatabaseConfig,
    databases::{connect_database, run_migrations},
};

fn memory_config(url: &str) -> DatabaseConfig {
    DatabaseConfig {
        url: url.to_string(),
        min_connections: 1,
        max_connections: 2,
        connect_timeout_secs: 2,
        idle_secs: 60,
        sqlx_logging: false,
    }
}

#[tokio::test]
async fn supported_memory_forms_share_migrations_across_held_connections() {
    for url in [
        "sqlite::memory:",
        "sqlite:file:pool-test?mode=memory&cache=shared",
        "sqlite://file%3Apool-test?mode=memory&cache=shared",
        "sqlite:file::memory:?cache=shared",
        "sqlite://file%3A%3Amemory%3A?cache=shared",
        "sqlite::memory:?cache=private&mode=memory",
    ] {
        let config = memory_config(url);
        config.validate().unwrap();
        let database = connect_database(&config).await.unwrap();
        run_migrations(&database).await.unwrap();
        let pool = database.get_sqlite_connection_pool();
        let mut first = pool.acquire().await.unwrap();
        let mut second = pool.acquire().await.unwrap();
        for connection in [&mut first, &mut second] {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM transfer_accounts")
                .fetch_one(&mut **connection)
                .await
                .unwrap();
            assert_eq!(count, 2, "{url}");
            let filename: String =
                sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
                    .fetch_one(&mut **connection)
                    .await
                    .unwrap();
            assert!(filename.is_empty(), "{url}: {filename}");
        }
        drop(first);
        drop(second);
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn unshared_memory_and_invalid_urls_fail_static_and_connection_validation() {
    for (url, expected) in [
        ("sqlite::memory:?cache=private", "shared cache"),
        ("sqlite::memory:?mode=memory&cache=private", "shared cache"),
        ("sqlite://?mode=memory&cache=shared", "shared file: URI"),
        (
            "sqlite://named?mode=memory&cache=shared",
            "shared file: URI",
        ),
        ("sqlite://%3Amemory%3A?cache=shared", "shared file: URI"),
        ("sqlite://:memory:", "invalid in-memory DATABASE_URL"),
        (
            "sqlite://file:private-name?mode=memory&cache=shared",
            "invalid in-memory DATABASE_URL",
        ),
        (
            "sqlite://file::memory:?cache=shared",
            "invalid in-memory DATABASE_URL",
        ),
        (
            "sqlite://[private-location?mode=rwc",
            "invalid DATABASE_URL",
        ),
    ] {
        let config = memory_config(url);
        let validation_error = config.validate().unwrap_err().to_string();
        let connection_error = connect_database(&config).await.unwrap_err().to_string();
        assert!(
            validation_error.contains(expected),
            "{url}: {validation_error}"
        );
        assert_eq!(validation_error, connection_error, "{url}");
        assert!(!validation_error.contains(url), "{url}: {validation_error}");
    }
}
