use anyhow::{Result, anyhow};

use super::env::{get_env_or, get_required_env, parse_env_or};

/// JWT 签发与校验参数。
#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub issuer: String,
    pub audience: String,
    pub access_token_ttl_minutes: i64,
}

impl JwtConfig {
    pub fn from_env(app_env: &str) -> Result<Self> {
        let config = Self {
            secret: get_required_env("JWT_SECRET")?,
            issuer: get_env_or("JWT_ISSUER", "axum-service-scaffold"),
            audience: get_env_or("JWT_AUDIENCE", "axum-service-clients"),
            access_token_ttl_minutes: parse_env_or("JWT_ACCESS_TOKEN_TTL_MINUTES", 120_i64)?,
        };
        config.validate(app_env)?;

        Ok(config)
    }

    fn validate(&self, app_env: &str) -> Result<()> {
        if self.secret.len() < 32 {
            return Err(anyhow!("JWT_SECRET must be at least 32 characters long"));
        }
        if app_env.eq_ignore_ascii_case("production")
            && self.secret == "change-me-to-a-random-string-with-at-least-32-characters"
        {
            return Err(anyhow!(
                "example JWT_SECRET is not allowed in production environment"
            ));
        }
        if self.access_token_ttl_minutes <= 0 {
            return Err(anyhow!(
                "JWT_ACCESS_TOKEN_TTL_MINUTES must be greater than 0"
            ));
        }

        Ok(())
    }
}
