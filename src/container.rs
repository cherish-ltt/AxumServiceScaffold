use std::sync::Arc;

use sea_orm::DatabaseConnection;

use anyhow::Result;

use crate::{
    domain::services::{
        auth::AuthUseCase, example::ExampleUseCase, system::SystemUseCase,
        transaction::TransferUseCase,
    },
    infrastructure::{
        config::AppConfig,
        databases::{connect_database, run_migrations},
        repositories::transaction::SeaOrmTransferRepository,
        services::jwt::JwtService,
    },
    metrics::MetricsHandle,
    services::{
        auth::AuthService, example::ExampleService, system::SystemService,
        transaction::TransferService,
    },
};

pub struct Container {
    pub config: Arc<AppConfig>,
    pub database: DatabaseConnection,
    /// 指标出口句柄；`METRICS_ENABLED=false` 时为 `None`，此时 `/metrics` 不挂载。
    pub metrics: Option<MetricsHandle>,
    pub auth_service: Arc<dyn AuthUseCase>,
    pub example_service: Arc<dyn ExampleUseCase>,
    pub system_service: Arc<dyn SystemUseCase>,
    pub transfer_service: Arc<dyn TransferUseCase>,
}

impl Container {
    pub async fn bootstrap(config: AppConfig) -> Result<Self> {
        let config = Arc::new(config);
        let metrics = MetricsHandle::install(&config.metrics)?;
        let database = connect_database(&config.database).await?;
        run_migrations(&database, &config.database.url).await?;
        let jwt_service = Arc::new(JwtService::new(config.jwt.clone())?);

        let auth_service: Arc<dyn AuthUseCase> = Arc::new(AuthService::new(jwt_service));
        let example_service: Arc<dyn ExampleUseCase> = Arc::new(ExampleService::new());
        let system_service: Arc<dyn SystemUseCase> =
            Arc::new(SystemService::new(config.clone(), database.clone()));
        let transfer_service: Arc<dyn TransferUseCase> = Arc::new(TransferService::new(
            database.clone(),
            Arc::new(SeaOrmTransferRepository),
        ));

        Ok(Self {
            config,
            database,
            metrics,
            auth_service,
            example_service,
            system_service,
            transfer_service,
        })
    }
}
