use std::sync::Arc;

use anyhow::Result;

use crate::{
    domain::services::{auth::AuthUseCase, example::ExampleUseCase, system::SystemUseCase},
    infrastructure::{config::AppConfig, databases::connect_database, services::jwt::JwtService},
    services::{auth::AuthService, example::ExampleService, system::SystemService},
};

pub struct Container {
    pub config: Arc<AppConfig>,
    pub auth_service: Arc<dyn AuthUseCase>,
    pub example_service: Arc<dyn ExampleUseCase>,
    pub system_service: Arc<dyn SystemUseCase>,
}

impl Container {
    pub async fn bootstrap(config: AppConfig) -> Result<Self> {
        let config = Arc::new(config);
        let database = connect_database(&config.database).await?;
        let jwt_service = Arc::new(JwtService::new(config.jwt.clone())?);

        let auth_service: Arc<dyn AuthUseCase> = Arc::new(AuthService::new(jwt_service));
        let example_service: Arc<dyn ExampleUseCase> = Arc::new(ExampleService::new());
        let system_service: Arc<dyn SystemUseCase> =
            Arc::new(SystemService::new(config.clone(), database));

        Ok(Self {
            config,
            auth_service,
            example_service,
            system_service,
        })
    }
}
