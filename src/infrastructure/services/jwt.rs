use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};

use crate::{
    domain::{
        error::AppError,
        models::auth::{AccessClaims, AccessToken},
    },
    infrastructure::config::JwtConfig,
};

#[derive(Clone)]
pub struct JwtService {
    encoding_key: EncodingKey,
    signing_kid: Option<String>,
    signing_accept_until: Option<i64>,
    verification_keys: VerificationKeys,
    issuer: String,
    audience: String,
    access_token_ttl_minutes: i64,
}

#[derive(Clone)]
enum VerificationKeys {
    Single(DecodingKey),
    Rotating {
        keys: BTreeMap<String, (DecodingKey, Option<i64>)>,
        legacy_kid: Option<String>,
    },
}

impl JwtService {
    pub fn new(config: JwtConfig) -> Result<Self> {
        config.validate_keys(false)?;
        let (encoding_key, signing_kid, signing_accept_until, verification_keys) =
            if let Some(key_set) = config.key_set {
                let current = key_set
                    .keys
                    .iter()
                    .find(|key| key.kid == key_set.current_kid)
                    .ok_or_else(|| {
                        anyhow!("JWT_KEY_SET current_kid must identify a configured key")
                    })?;
                let encoding_key = EncodingKey::from_secret(current.secret.as_bytes());
                let signing_accept_until = current.accept_until;
                let keys = key_set
                    .keys
                    .into_iter()
                    .map(|key| {
                        (
                            key.kid,
                            (
                                DecodingKey::from_secret(key.secret.as_bytes()),
                                key.accept_until,
                            ),
                        )
                    })
                    .collect();
                (
                    encoding_key,
                    Some(key_set.current_kid),
                    signing_accept_until,
                    VerificationKeys::Rotating {
                        keys,
                        legacy_kid: key_set.legacy_kid,
                    },
                )
            } else {
                (
                    EncodingKey::from_secret(config.secret.as_bytes()),
                    None,
                    None,
                    VerificationKeys::Single(DecodingKey::from_secret(config.secret.as_bytes())),
                )
            };

        Ok(Self {
            encoding_key,
            signing_kid,
            signing_accept_until,
            verification_keys,
            issuer: config.issuer,
            audience: config.audience,
            access_token_ttl_minutes: config.access_token_ttl_minutes,
        })
    }

    pub fn issue_access_token(
        &self,
        user_id: &str,
        username: &str,
        roles: &[String],
    ) -> Result<AccessToken> {
        self.issue_access_token_at(user_id, username, roles, Utc::now())
    }

    fn issue_access_token_at(
        &self,
        user_id: &str,
        username: &str,
        roles: &[String],
        now: DateTime<Utc>,
    ) -> Result<AccessToken> {
        if self
            .signing_accept_until
            .is_some_and(|cutoff| now.timestamp() >= cutoff)
        {
            return Err(anyhow!(
                "current JWT signing key acceptance window has ended"
            ));
        }
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

        let mut header = Header::new(Algorithm::HS256);
        header.kid = self.signing_kid.clone();
        let access_token = encode(&header, &claims, &self.encoding_key)?;

        Ok(AccessToken {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in_seconds: self.access_token_ttl_minutes * 60,
        })
    }

    pub fn verify_access_token(&self, token: &str) -> Result<AccessClaims, AppError> {
        self.verify_access_token_at(token, Utc::now().timestamp())
    }

    fn verify_access_token_at(&self, token: &str, now: i64) -> Result<AccessClaims, AppError> {
        let invalid = || AppError::unauthorized("access token is invalid or expired");
        let decoding_key = match &self.verification_keys {
            VerificationKeys::Single(key) => key,
            VerificationKeys::Rotating { keys, legacy_kid } => {
                let header = decode_header(token).map_err(|_| invalid())?;
                let kid = header
                    .kid
                    .as_ref()
                    .or(legacy_kid.as_ref())
                    .ok_or_else(invalid)?;
                let (key, accept_until) = keys.get(kid).ok_or_else(invalid)?;
                if accept_until.is_some_and(|cutoff| now >= cutoff) {
                    return Err(invalid());
                }
                key
            },
        };
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&[self.audience.as_str()]);
        validation.set_issuer(&[self.issuer.as_str()]);

        decode::<AccessClaims>(token, decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::JwtService;
    use crate::{
        domain::error::AppError,
        infrastructure::config::{JwtConfig, JwtKeyConfig, JwtKeySet},
    };
    use chrono::{DateTime, Utc};
    use jsonwebtoken::{Algorithm, EncodingKey, Header, decode_header, encode};

    const TEST_SECRET: &str = "jwt-unit-test-secret-that-is-long-enough";
    const NEW_SECRET: &str = "jwt-unit-test-new-secret-that-is-long-enough";

    fn config(secret: &str, ttl_minutes: i64) -> JwtConfig {
        JwtConfig {
            secret: secret.to_string(),
            key_set: None,
            issuer: "test-issuer".to_string(),
            audience: "test-audience".to_string(),
            access_token_ttl_minutes: ttl_minutes,
        }
    }

    #[test]
    fn short_secret_is_rejected() {
        assert!(JwtService::new(config("short", 120)).is_err());
    }

    fn rotating_config(current: &str, cutoff: i64) -> JwtConfig {
        let mut config = config("", 120);
        config.key_set = Some(JwtKeySet {
            current_kid: current.to_string(),
            legacy_kid: Some("A".to_string()),
            keys: vec![
                JwtKeyConfig {
                    kid: "A".to_string(),
                    secret: TEST_SECRET.to_string(),
                    accept_until: Some(cutoff),
                },
                JwtKeyConfig {
                    kid: "B".to_string(),
                    secret: NEW_SECRET.to_string(),
                    accept_until: Some(cutoff),
                },
            ],
        });
        config
    }

    fn signed_token(
        kid: Option<&str>,
        secret: &str,
        algorithm: Algorithm,
        claims: &serde_json::Value,
    ) -> String {
        let mut header = Header::new(algorithm);
        header.kid = kid.map(str::to_string);
        encode(
            &header,
            claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    fn claims(expiration: i64) -> serde_json::Value {
        serde_json::json!({
            "sub": "user-1", "username": "alice", "roles": [],
            "iss": "test-issuer", "aud": "test-audience", "iat": 1, "exp": expiration,
        })
    }

    #[test]
    fn prepare_switch_and_retire_preserve_only_trusted_tokens() {
        let cutoff = Utc::now().timestamp() + 3600;
        let legacy = JwtService::new(config(TEST_SECRET, 120))
            .unwrap()
            .issue_access_token("user-1", "alice", &[])
            .unwrap()
            .access_token;
        assert!(decode_header(&legacy).unwrap().kid.is_none());
        let prepare = JwtService::new(rotating_config("A", cutoff)).unwrap();
        let switch = JwtService::new(rotating_config("B", cutoff)).unwrap();
        let old = prepare
            .issue_access_token("user-1", "alice", &[])
            .unwrap()
            .access_token;
        let new = switch
            .issue_access_token("user-1", "alice", &[])
            .unwrap()
            .access_token;
        assert_eq!(decode_header(&old).unwrap().kid.as_deref(), Some("A"));
        assert_eq!(decode_header(&new).unwrap().kid.as_deref(), Some("B"));
        for service in [&prepare, &switch] {
            for token in [&legacy, &old, &new] {
                assert!(service.verify_access_token(token).is_ok());
            }
        }
        let mut retired = rotating_config("B", cutoff);
        let keys = retired.key_set.as_mut().unwrap();
        keys.legacy_kid = None;
        keys.keys.retain(|key| key.kid == "B");
        keys.keys[0].accept_until = None;
        let retired = JwtService::new(retired).unwrap();
        assert!(retired.verify_access_token(&old).is_err());
        assert!(retired.verify_access_token(&legacy).is_err());
        assert!(retired.verify_access_token(&new).is_ok());
    }

    #[test]
    fn acceptance_cutoff_is_exact_and_independent_of_expiry_leeway() {
        let cutoff = Utc::now().timestamp();
        let verifier = JwtService::new(rotating_config("B", cutoff)).unwrap();
        for expiration in [cutoff + 3600, cutoff - 30] {
            let claims = claims(expiration);
            for kid in [Some("A"), None] {
                let token = signed_token(kid, TEST_SECRET, Algorithm::HS256, &claims);
                assert!(verifier.verify_access_token_at(&token, cutoff - 1).is_ok());
                assert!(verifier.verify_access_token_at(&token, cutoff).is_err());
                assert!(verifier.verify_access_token_at(&token, cutoff + 1).is_err());
            }
        }
        let expired = signed_token(
            Some("A"),
            TEST_SECRET,
            Algorithm::HS256,
            &claims(cutoff - 300),
        );
        assert!(
            verifier
                .verify_access_token_at(&expired, cutoff - 1)
                .is_err()
        );
    }

    #[test]
    fn current_key_cutoff_also_stops_signing() {
        let cutoff = Utc::now().timestamp() + 3600;
        let service = JwtService::new(rotating_config("A", cutoff)).unwrap();
        let before = DateTime::from_timestamp(cutoff - 1, 0).unwrap();
        let token = service
            .issue_access_token_at("user-1", "alice", &[], before)
            .unwrap()
            .access_token;
        assert!(service.verify_access_token_at(&token, cutoff - 1).is_ok());
        for now in [cutoff, cutoff + 1] {
            assert!(
                service
                    .issue_access_token_at(
                        "user-1",
                        "alice",
                        &[],
                        DateTime::from_timestamp(now, 0).unwrap()
                    )
                    .is_err()
            );
            assert!(service.verify_access_token_at(&token, now).is_err());
        }
    }

    #[test]
    fn selected_key_never_falls_back_and_algorithm_stays_fixed() {
        let cutoff = Utc::now().timestamp() + 3600;
        let service = JwtService::new(rotating_config("B", cutoff)).unwrap();
        let claims = claims(cutoff);
        for (kid, secret, algorithm) in [
            (Some("unknown"), TEST_SECRET, Algorithm::HS256),
            (Some(""), TEST_SECRET, Algorithm::HS256),
            (Some("B"), TEST_SECRET, Algorithm::HS256),
            (None, NEW_SECRET, Algorithm::HS256),
            (Some("A"), TEST_SECRET, Algorithm::HS384),
        ] {
            let token = signed_token(kid, secret, algorithm, &claims);
            assert!(service.verify_access_token(&token).is_err());
        }
        assert!(service.verify_access_token("invalid.token.data").is_err());
        let mut no_legacy = rotating_config("B", cutoff);
        no_legacy.key_set.as_mut().unwrap().legacy_kid = None;
        let no_legacy = JwtService::new(no_legacy).unwrap();
        let token = signed_token(None, TEST_SECRET, Algorithm::HS256, &claims);
        assert!(no_legacy.verify_access_token(&token).is_err());
    }

    #[test]
    fn single_secret_preserves_verification_of_tokens_with_a_kid() {
        let service = JwtService::new(config(TEST_SECRET, 120)).unwrap();
        let token = signed_token(
            Some("externally-issued"),
            TEST_SECRET,
            Algorithm::HS256,
            &claims(Utc::now().timestamp() + 3600),
        );
        assert!(service.verify_access_token(&token).is_ok());
    }

    #[test]
    fn rotating_keys_still_validate_issuer_audience_and_expiration() {
        let cutoff = Utc::now().timestamp() + 3600;
        let service = JwtService::new(rotating_config("B", cutoff)).unwrap();
        for kid in ["A", "B"] {
            let secret = if kid == "A" { TEST_SECRET } else { NEW_SECRET };
            for field in ["iss", "aud", "exp"] {
                let mut invalid = claims(cutoff);
                invalid[field] = if field == "exp" {
                    serde_json::json!(1)
                } else {
                    serde_json::json!("wrong")
                };
                let token = signed_token(Some(kid), secret, Algorithm::HS256, &invalid);
                assert!(service.verify_access_token(&token).is_err());
                invalid.as_object_mut().unwrap().remove(field);
                let token = signed_token(Some(kid), secret, Algorithm::HS256, &invalid);
                assert!(service.verify_access_token(&token).is_err());
            }
        }
    }

    #[test]
    fn direct_construction_validates_rotation_keys() {
        let mut invalid = rotating_config("missing", 1);
        assert!(JwtService::new(invalid.clone()).is_err());
        invalid.key_set.as_mut().unwrap().current_kid = "B".to_string();
        invalid.key_set.as_mut().unwrap().keys[0].secret = "short".to_string();
        assert!(JwtService::new(invalid.clone()).is_err());
        invalid.key_set.as_mut().unwrap().keys[0].secret = TEST_SECRET.to_string();
        invalid.secret = TEST_SECRET.to_string();
        assert!(JwtService::new(invalid).is_err());
    }

    #[test]
    fn issued_token_verifies_roundtrip() {
        let service = JwtService::new(config(TEST_SECRET, 120)).expect("构建 JWT 服务");
        let token = service
            .issue_access_token("user-1", "alice", &["admin".to_string()])
            .expect("签发令牌");

        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.expires_in_seconds, 120 * 60);

        let claims = service
            .verify_access_token(&token.access_token)
            .expect("校验令牌");
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.username, "alice");
        assert_eq!(claims.roles, vec!["admin".to_string()]);
        assert_eq!(claims.iss, "test-issuer");
        assert_eq!(claims.aud, "test-audience");
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn invalid_token_is_unauthorized() {
        let service = JwtService::new(config(TEST_SECRET, 120)).expect("构建 JWT 服务");

        let error = service
            .verify_access_token("not-a-jwt")
            .expect_err("无效令牌应被拒绝");
        assert!(matches!(error, AppError::Unauthorized(_)));
    }

    #[test]
    fn token_signed_with_other_secret_is_rejected() {
        let service = JwtService::new(config(TEST_SECRET, 120)).expect("构建 JWT 服务");
        let other = JwtService::new(config("another-secret-that-is-long-enough!!!", 120))
            .expect("构建另一个 JWT 服务");
        let token = other
            .issue_access_token("user-1", "alice", &[])
            .expect("签发令牌");

        assert!(service.verify_access_token(&token.access_token).is_err());
    }

    #[test]
    fn expired_token_is_rejected() {
        // 负 TTL 生成已过期令牌（jsonwebtoken 默认 60 秒 leeway，因此回退 5 分钟）。
        let service = JwtService::new(config(TEST_SECRET, -5)).expect("构建 JWT 服务");
        let token = service
            .issue_access_token("user-1", "alice", &[])
            .expect("签发令牌");

        let error = service
            .verify_access_token(&token.access_token)
            .expect_err("过期令牌应被拒绝");
        assert!(matches!(error, AppError::Unauthorized(_)));
    }
}
