use std::{
    fs,
    io::Read,
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use sqlx::SqlitePool;
use uuid::Uuid;

struct Fixture {
    directory: PathBuf,
    listener: TcpListener,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("axum-cli-{}", Uuid::now_v7()));
        fs::create_dir(&directory).unwrap();
        Self {
            directory,
            listener: TcpListener::bind("127.0.0.1:0").unwrap(),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_axum-service-scaffold"));
        command
            .args(args)
            .current_dir(&self.directory)
            .env_clear()
            .env("JWT_SECRET", "cli-test-signing-key-for-isolated-processes")
            .env(
                "SERVER_PORT",
                self.listener.local_addr().unwrap().port().to_string(),
            )
            .env("DATABASE_URL", "sqlite://application.db?mode=rwc")
            .env("LOG_OUT_DIR", "logs")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        command
    }

    async fn database(&self) -> SqlitePool {
        SqlitePool::connect(&format!(
            "sqlite://{}?mode=rwc",
            self.directory.join("application.db").display()
        ))
        .await
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn run(command: &mut Command) -> Output {
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take();
    let mut stderr = child.stderr.take().unwrap();
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stdout) = stdout {
            stdout.read_to_end(&mut bytes).unwrap();
        }
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let started = Instant::now();
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            timed_out = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = Output {
        status: child.wait().unwrap(),
        stdout: stdout_reader.join().unwrap(),
        stderr: stderr_reader.join().unwrap(),
    };
    assert!(!timed_out, "command did not exit: {output:?}");
    output
}

#[tokio::test]
async fn init_db_prepares_database_without_binding_http() {
    let fixture = Fixture::new();
    let output = run(&mut fixture.command(&["--init-db"]));
    assert!(output.status.success(), "{output:?}");
    assert!(!fixture.directory.join("logs").exists());
    let database = fixture.database().await;
    let tables: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('transfer_accounts', 'transfer_records', 'transfer_audits', '_sqlx_migrations')")
        .fetch_one(&database).await.unwrap();
    let indexes: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_transfer_records_created_at', 'idx_transfer_records_request_id', 'idx_transfer_audits_record_id')")
        .fetch_one(&database).await.unwrap();
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM transfer_accounts")
        .fetch_one(&database)
        .await
        .unwrap();
    assert_eq!((tables, indexes, accounts), (4, 3, 2));
    database.close().await;
}

#[tokio::test]
async fn init_db_preserves_legacy_data_and_is_repeatable() {
    let fixture = Fixture::new();
    let database = fixture.database().await;
    let legacy_schema = include_str!("../migrations/0001_create_transfer_tables.sql")
        .replace("    request_id TEXT,\n", "");
    sqlx::raw_sql(sqlx::AssertSqlSafe(legacy_schema))
        .execute(&database)
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO transfer_accounts VALUES ('acc_alice', 'Alice', 42, 7, 123);\
         INSERT INTO transfer_records VALUES ('legacy', 'acc_alice', 'acc_bob', 10, 42, 99, NULL, 123);\
         INSERT INTO transfer_audits VALUES ('audit', 'legacy', 'transfer', 'retained', 123);",
    ).execute(&database).await.unwrap();
    database.close().await;

    for _ in 0..2 {
        let output = run(&mut fixture.command(&["--init-db"]));
        assert!(output.status.success(), "{output:?}");
    }

    let database = fixture.database().await;
    let alice: (i64, i64, i64) = sqlx::query_as(
        "SELECT balance_cents, version, updated_at FROM transfer_accounts WHERE id = 'acc_alice'",
    )
    .fetch_one(&database)
    .await
    .unwrap();
    assert_eq!(alice, (42, 7, 123));
    let record: (Option<String>, i64, i64, i64) = sqlx::query_as(
        "SELECT request_id, amount_cents, from_balance_after_cents, to_balance_after_cents FROM transfer_records WHERE id = 'legacy'",
    ).fetch_one(&database).await.unwrap();
    assert_eq!(record, (None, 10, 42, 99));
    let detail: String =
        sqlx::query_scalar("SELECT detail FROM transfer_audits WHERE id = 'audit'")
            .fetch_one(&database)
            .await
            .unwrap();
    assert_eq!(detail, "retained");
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM transfer_accounts), (SELECT count(*) FROM transfer_records), (SELECT count(*) FROM transfer_audits)",
    ).fetch_one(&database).await.unwrap();
    assert_eq!(counts, (2, 1, 1));
    sqlx::query("UPDATE transfer_records SET request_id = 'duplicate' WHERE id = 'legacy'")
        .execute(&database)
        .await
        .unwrap();
    let conflict = sqlx::query("INSERT INTO transfer_records (id, request_id, from_account_id, to_account_id, amount_cents, from_balance_after_cents, to_balance_after_cents, remark, created_at) SELECT 'other', request_id, from_account_id, to_account_id, amount_cents, from_balance_after_cents, to_balance_after_cents, remark, created_at FROM transfer_records")
        .execute(&database).await.unwrap_err();
    assert!(conflict.as_database_error().unwrap().is_unique_violation());
    database.close().await;
}

#[test]
fn init_db_rejects_transient_databases_and_connection_failures() {
    for url in [
        "sqlite::memory:",
        "sqlite://:memory:",
        "sqlite://?mode=memory",
        "sqlite://named?mode=memory&cache=shared",
        "sqlite://file::memory:",
        "sqlite://%3Amemory%3A",
    ] {
        let fixture = Fixture::new();
        let output = run(fixture.command(&["--init-db"]).env("DATABASE_URL", url));
        assert!(!output.status.success(), "{url}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("in-memory"),
            "{url}: {output:?}"
        );
        assert!(output.stdout.is_empty());
        assert!(!fixture.directory.join("logs").exists());
    }
    let fixture = Fixture::new();
    let url = "sqlite://missing/private-database-location.db?mode=rw";
    let output = run(fixture.command(&["--init-db"]).env("DATABASE_URL", url));
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("database connection failed"), "{error}");
    assert!(!error.contains("private-database-location"), "{error}");
    assert!(output.stdout.is_empty());
}

#[tokio::test]
async fn init_db_reports_schema_failure_without_success_message() {
    let fixture = Fixture::new();
    let database = fixture.database().await;
    sqlx::query("CREATE TABLE transfer_accounts (id TEXT PRIMARY KEY)")
        .execute(&database)
        .await
        .unwrap();
    database.close().await;
    let output = run(&mut fixture.command(&["--init-db"]));
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to seed"),
        "{output:?}"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn help_and_invalid_arguments_do_not_initialize_resources() {
    for args in [
        &["--help"][..],
        &["--unknown"],
        &["--init-db", "--init-db"],
        &["--init-db", "--check-config"],
        &["--check-config", "--help"],
        &["--export-openapi", "--init-db"],
        &["--check-config", "--export-openapi"],
        &["--help", "--export-openapi"],
    ] {
        let fixture = Fixture::new();
        let output = run(fixture.command(args).env_remove("JWT_SECRET"));
        assert_eq!(output.status.success(), args == ["--help"], "{output:?}");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!text.contains("missing required environment"), "{text}");
        if args == ["--help"] {
            for detail in ["Migrate", "seed", "JWT", "not atomic", "nonzero"] {
                assert!(text.contains(detail), "{text}");
            }
        }
        assert!(!fixture.directory.join("application.db").exists());
        assert!(!fixture.directory.join("logs").exists());
    }
}

#[test]
fn init_db_preserves_runtime_guard() {
    let fixture = Fixture::new();
    let output = run(fixture.command(&["--init-db"]).env("APP_ENV", "production"));
    assert_eq!(
        output.status.success(),
        !cfg!(debug_assertions),
        "{output:?}"
    );
    if cfg!(debug_assertions) {
        assert!(String::from_utf8_lossy(&output.stderr).contains("release build"));
        assert!(!fixture.directory.join("application.db").exists());
    }
}

#[test]
fn check_config_does_not_access_runtime_resources() {
    let fixture = Fixture::new();
    fs::write(
        fixture.directory.join("blocked-log-path"),
        b"existing log file",
    )
    .unwrap();
    fs::write(
        fixture.directory.join("application.db"),
        b"existing database bytes",
    )
    .unwrap();
    for url in [
        "sqlite://missing/read-only.db?mode=ro",
        "sqlite://application.db?mode=rwc",
    ] {
        let output = run(fixture
            .command(&["--check-config"])
            .env("DATABASE_URL", url)
            .env("LOG_OUT_DIR", "blocked-log-path"));
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Static configuration valid"));
        assert!(!fixture.directory.join("missing").exists());
        assert!(!fixture.directory.join("logs").exists());
        assert_eq!(
            fs::read(fixture.directory.join("application.db")).unwrap(),
            b"existing database bytes"
        );
        assert_eq!(
            fs::read(fixture.directory.join("blocked-log-path")).unwrap(),
            b"existing log file"
        );
    }
}

#[test]
fn check_config_and_startup_share_static_rejections() {
    for (key, value, expected) in [
        ("JWT_SECRET", "short", "JWT_SECRET"),
        ("SERVER_HOST", "not-an-ip", "SERVER_HOST"),
        ("MIDDLEWARE_MAX_CONCURRENCY", "0", "capacity"),
        (
            "LOG_FILTER",
            "private-filter-do-not-print=notalevel",
            "LOG_FILTER",
        ),
        ("LOG_UTC_OFFSET_MINUTE", "99", "timezone offset"),
        (
            "DATABASE_URL",
            "sqlite::memory:?cache=private",
            "shared cache",
        ),
        (
            "DATABASE_URL",
            "private-database-url-do-not-print",
            "DATABASE_URL",
        ),
        (
            "DATABASE_URL",
            "sqlite://?mode=memory&cache=shared",
            "shared file: URI",
        ),
        (
            "DATABASE_URL",
            "sqlite://named?mode=memory&cache=shared",
            "shared file: URI",
        ),
        (
            "DATABASE_URL",
            "sqlite://%3Amemory%3A?cache=shared",
            "shared file: URI",
        ),
    ] {
        for args in [&["--check-config"][..], &[]] {
            let fixture = Fixture::new();
            let output = run(fixture.command(args).env(key, value));
            assert!(!output.status.success(), "{key}: {output:?}");
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains(expected), "{key}: {output:?}");
            assert!(!error.contains("do-not-print"), "{output:?}");
            assert!(output.stdout.is_empty(), "{output:?}");
            assert!(!fixture.directory.join("application.db").exists());
            assert!(!fixture.directory.join("logs").exists());
        }
    }
    let fixture = Fixture::new();
    let output = run(fixture
        .command(&["--check-config"])
        .env_remove("JWT_SECRET"));
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("JWT_SECRET"));
}

#[cfg(unix)]
#[test]
fn check_config_does_not_echo_non_unicode_secret() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let fixture = Fixture::new();
    let mut secret = b"private-key-bytes-do-not-print".to_vec();
    secret.push(0xff);
    let output = run(fixture
        .command(&["--check-config"])
        .env("JWT_SECRET", OsString::from_vec(secret)));
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("JWT_SECRET"), "{error}");
    assert!(!error.contains("do-not-print"), "{error}");
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 0);
}

#[test]
fn check_config_accepts_shared_memory_and_preserves_build_guard() {
    for url in [
        "sqlite::memory:",
        "sqlite:file:cli-check?mode=memory&cache=shared",
        "sqlite::memory:?cache=private&mode=memory",
    ] {
        let fixture = Fixture::new();
        let output = run(fixture
            .command(&["--check-config"])
            .env("DATABASE_URL", url));
        assert!(output.status.success(), "{url}: {output:?}");
        assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 0);
    }
    let fixture = Fixture::new();
    let output = run(fixture
        .command(&["--check-config"])
        .env("APP_ENV", "production"));
    assert_eq!(
        output.status.success(),
        !cfg!(debug_assertions),
        "{output:?}"
    );
    assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 0);
}

#[test]
fn check_config_loads_dotenv_with_environment_precedence() {
    let fixture = Fixture::new();
    fs::write(
        fixture.directory.join(".env"),
        "JWT_SECRET=dotenv-config-test-signing-key-long-enough\nLOG_UTC_OFFSET_MINUTE=99\n",
    )
    .unwrap();
    let output = run(fixture
        .command(&["--check-config"])
        .env_remove("JWT_SECRET"));
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("timezone offset"));
    let output = run(fixture
        .command(&["--check-config"])
        .env_remove("JWT_SECRET")
        .env("LOG_UTC_OFFSET_MINUTE", "0"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 1);
}

#[cfg(feature = "docs")]
#[tokio::test]
async fn export_openapi_without_config_matches_http_document() {
    use axum::{Router, body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    let fixture = Fixture::new();
    let output = run(fixture
        .command(&["--export-openapi"])
        .env_remove("JWT_SECRET")
        .env("APP_ENV", "production")
        .env("DATABASE_URL", "invalid-database-url")
        .env("SERVER_HOST", "invalid-server-address")
        .env("LOG_FILTER", "invalid=not-a-level"));
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let exported: Value = serde_json::from_slice(&output.stdout).unwrap();
    let response = axum_service_scaffold::docs::mount(Router::new())
        .oneshot(
            Request::builder()
                .uri("/api-doc/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let served: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(exported, served);
    assert_eq!(exported["info"]["version"], env!("CARGO_PKG_VERSION"));
    for path in [
        "/api/v1/auth/dev-login",
        "/api/v1/transactions/dev-transfer",
    ] {
        assert_eq!(
            exported["paths"].get(path).is_some(),
            cfg!(debug_assertions),
            "{path}"
        );
    }
    assert!(exported["paths"].get("/api/v1/system/health").is_some());
    assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 0);
}

#[cfg(not(feature = "docs"))]
#[test]
fn export_openapi_requires_docs_before_loading_configuration() {
    let fixture = Fixture::new();
    let output = run(fixture
        .command(&["--export-openapi"])
        .env_remove("JWT_SECRET")
        .env("APP_ENV", "production")
        .env("DATABASE_URL", "invalid-database-url"));
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("--export-openapi requires the docs feature"),
        "{error}"
    );
    assert!(!error.contains("JWT_SECRET"), "{error}");
    assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 0);
}

#[cfg(all(feature = "docs", unix))]
#[test]
fn export_openapi_reports_stdout_failure() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};

    let fixture = Fixture::new();
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let descriptor: OwnedFd = writer.into();
    let output = run(fixture
        .command(&["--export-openapi"])
        .env_remove("JWT_SECRET")
        .stdout(Stdio::from(descriptor)));
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("failed to write OpenAPI document"),
        "{error}"
    );
    assert!(!error.contains("panicked"), "{error}");
    assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn normal_start_serves_http_and_shuts_down_on_signals() {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    for signal in ["-TERM", "-INT"] {
        let fixture = Fixture::new();
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserved.local_addr().unwrap();
        drop(reserved);
        let mut child = fixture
            .command(&[])
            .env("SERVER_PORT", address.port().to_string())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let response = loop {
            if let Ok(mut socket) = TcpStream::connect(address) {
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                socket.write_all(b"GET /api/v1/system/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
                let mut response = String::new();
                let result = socket.read_to_string(&mut response);
                break result.map(|_| response);
            }
            if child.try_wait().unwrap().is_some() || started.elapsed() > Duration::from_secs(60) {
                let _ = child.kill();
                panic!(
                    "normal service did not start: {:?}",
                    child.wait_with_output().unwrap()
                );
            }
            thread::sleep(Duration::from_millis(10));
        };
        let signal_status = Command::new("kill")
            .args([signal, &child.id().to_string()])
            .status()
            .unwrap();
        let shutdown_started = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if shutdown_started.elapsed() > Duration::from_secs(60) {
                child.kill().unwrap();
                panic!(
                    "service did not stop: {:?}",
                    child.wait_with_output().unwrap()
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(signal_status.success());
        assert!(response.unwrap().starts_with("HTTP/1.1 200"), "{output:?}");
        assert!(output.status.success(), "{output:?}");
        assert!(fixture.directory.join("application.db").exists());
        assert!(fixture.directory.join("logs").exists());
    }
}
