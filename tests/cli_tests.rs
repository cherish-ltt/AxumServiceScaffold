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
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
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
    for args in [&["--help"][..], &["--unknown"], &["--init-db", "--init-db"]] {
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
