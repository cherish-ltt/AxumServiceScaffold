use anyhow::{Result, anyhow};
use chrono::{Duration, Utc};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode,
};
use serde::{Deserialize, Serialize};

use crate::{config::JwtConfig, error::AppError};

use super::dto::AccessTokenResponse;

/// 访问令牌中的声明字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: String,
    pub username: String,
    pub roles: Vec<String>,
    pub iss: String,
    pub aud: String,
    pub iat: usize,
    pub exp: usize,
}

/// JWT 服务。
///
/// 统一负责签发与校验令牌，避免业务模块各自处理加解密细节。
#[derive(Clone)]
pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    issuer: String,
    audience: String,
    access_token_ttl_minutes: i64,
}

impl JwtService {
    /// 根据配置创建 JWT 服务。
    pub fn new(config: JwtConfig) -> Result<Self> {
        if config.secret.len() < 32 {
            return Err(anyhow!("JWT_SECRET 长度至少需要 32 个字符"));
        }

        Ok(Self {
            encoding_key: EncodingKey::from_secret(config.secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(config.secret.as_bytes()),
            issuer: config.issuer,
            audience: config.audience,
            access_token_ttl_minutes: config.access_token_ttl_minutes,
        })
    }

    /// 签发访问令牌。
    pub fn issue_access_token(
        &self,
        user_id: &str,
        username: &str,
        roles: &[String],
    ) -> Result<AccessTokenResponse> {
        let now = Utc::now();
        let expires_at = now + Duration::minutes(self.access_token_ttl_minutes);
        let claims = AccessClaims {
            sub: user_id.to_string(),
            username: username.to_string(),
            roles: roles.to_vec(),
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            iat: now.timestamp() as usize,
            exp: expires_at.timestamp() as usize,
        };

        let access_token = encode(&Header::default(), &claims, &self.encoding_key)?;

        Ok(AccessTokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in_seconds: self.access_token_ttl_minutes * 60,
        })
    }

    /// 校验访问令牌并返回解析后的声明。
    pub fn verify_access_token(&self, token: &str) -> Result<AccessClaims, AppError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&[self.audience.as_str()]);
        validation.set_issuer(&[self.issuer.as_str()]);

        decode::<AccessClaims>(token, &self.decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(|_| AppError::unauthorized("访问令牌无效或已过期"))
    }
}
