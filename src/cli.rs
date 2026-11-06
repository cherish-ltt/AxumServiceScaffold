use anyhow::{Result, anyhow};
use axum_service_scaffold::infrastructure::{
    config::AppConfig,
    databases::{connect_database, run_migrations},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Serve,
    InitDb,
    Help,
}

impl Command {
    pub fn parse() -> Result<Self> {
        let mut arguments = std::env::args_os().skip(1);
        let argument = arguments.next();
        let command = match argument.as_deref() {
            None => Self::Serve,
            Some(value) if value == "--init-db" => Self::InitDb,
            Some(value) if value == "--help" || value == "-h" => Self::Help,
            _ => return Err(anyhow!("unknown argument; use --help")),
        };
        anyhow::ensure!(
            arguments.next().is_none(),
            "only one command is allowed; use --help"
        );
        Ok(command)
    }
}

pub fn print_help() {
    println!(
        "Usage: axum-service-scaffold [--init-db | --help]\n\nWithout arguments, start the HTTP server.\n  --init-db  Migrate a persistent SQLite database, repair legacy schema/indexes,\n             seed missing demo accounts, then close the pool and exit.\n             Requires full valid configuration, including JWT settings.\n             Modifies the database; the entire process is not atomic.\n             Exits 0 on success and nonzero on failure; does not start HTTP.\n  --help     Show this help"
    );
}

pub async fn init_database(config: &AppConfig) -> Result<()> {
    anyhow::ensure!(
        !config.database.is_in_memory()?,
        "--init-db does not support in-memory SQLite"
    );
    let database = connect_database(&config.database)
        .await
        .map_err(|_| anyhow!("database connection failed"))?;
    let initialized = async {
        let filename: String =
            sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
                .fetch_one(database.get_sqlite_connection_pool())
                .await
                .map_err(|_| anyhow!("failed to inspect database persistence"))?;
        anyhow::ensure!(
            !filename.is_empty(),
            "--init-db does not support in-memory SQLite"
        );
        run_migrations(&database)
            .await
            .map_err(|error| anyhow!("{error}"))
    }
    .await;
    let closed = database.close().await;
    match (initialized, closed) {
        (Err(error), Err(_)) => {
            return Err(error.context("database initialization and connection pool closure failed"));
        },
        (Err(error), Ok(())) => return Err(error),
        (Ok(()), Err(_)) => return Err(anyhow!("failed to close database connection pool")),
        (Ok(()), Ok(())) => {},
    }
    println!("Database initialization completed.");
    Ok(())
}
