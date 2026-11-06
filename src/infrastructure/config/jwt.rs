use std::fmt;

use anyhow::{Result, anyhow};
use chrono::DateTime;
use serde::Deserialize;

#[derive(Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub key_set: Option<JwtKeySet>,
    pub issuer: String,
    pub audience: String,
    pub access_token_ttl_minutes: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtKeySet {
    pub current_kid: String,
    pub legacy_kid: Option<String>,
    pub keys: Vec<JwtKeyConfig>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtKeyConfig {
    pub kid: String,
    pub secret: String,
    pub accept_until: Option<i64>,
}

impl fmt::Debug for JwtConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JwtConfig")
            .field("secret", &"[REDACTED]")
            .field("key_set", &self.key_set)
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("access_token_ttl_minutes", &self.access_token_ttl_minutes)
            .finish()
    }
}

impl fmt::Debug for JwtKeyConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JwtKeyConfig")
            .field("kid", &self.kid)
            .field("secret", &"[REDACTED]")
            .field("accept_until", &self.accept_until)
            .finish()
    }
}

impl JwtConfig {
    pub(crate) fn validate_keys(&self, production: bool) -> Result<()> {
        match &self.key_set {
            Some(keys) => {
                if !self.secret.is_empty() {
                    return Err(anyhow!("JWT_KEY_SET and JWT_SECRET cannot both be set"));
                }
                keys.validate(production)
            },
            None => {
                if self.secret.is_empty() {
                    return Err(anyhow!("missing required environment variable: JWT_SECRET"));
                }
                validate_secret(&self.secret, "JWT_SECRET", production)
            },
        }
    }
}

impl JwtKeySet {
    fn validate(&self, production: bool) -> Result<()> {
        if self.keys.is_empty() || self.keys.len() > 4 {
            return Err(anyhow!("JWT_KEY_SET must contain between 1 and 4 keys"));
        }
        for (index, key) in self.keys.iter().enumerate() {
            key.validate(production)?;
            if self.keys[..index].iter().any(|other| other.kid == key.kid) {
                return Err(anyhow!("JWT_KEY_SET contains duplicate kid values"));
            }
            if key.kid != self.current_kid && key.accept_until.is_none() {
                return Err(anyhow!("JWT_KEY_SET non-current keys require accept_until"));
            }
        }
        if !self.keys.iter().any(|key| key.kid == self.current_kid) {
            return Err(anyhow!(
                "JWT_KEY_SET current_kid must identify a configured key"
            ));
        }
        if let Some(legacy_kid) = &self.legacy_kid
            && !self
                .keys
                .iter()
                .any(|key| key.kid == *legacy_kid && key.accept_until.is_some())
        {
            return Err(anyhow!(
                "JWT_KEY_SET legacy_kid must identify a key with accept_until"
            ));
        }
        Ok(())
    }
}

impl JwtKeyConfig {
    fn validate(&self, production: bool) -> Result<()> {
        if self.kid.is_empty()
            || self.kid.len() > 64
            || !self
                .kid
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return Err(anyhow!(
                "JWT_KEY_SET kid must be 1-64 ASCII letters, digits, -, _ or ."
            ));
        }
        validate_secret(&self.secret, "JWT_KEY_SET secret", production)?;
        if self.accept_until.is_some_and(|timestamp| {
            timestamp <= 0 || DateTime::from_timestamp(timestamp, 0).is_none()
        }) {
            return Err(anyhow!(
                "JWT_KEY_SET accept_until must be a positive UTC Unix timestamp in seconds"
            ));
        }
        Ok(())
    }
}

fn validate_secret(secret: &str, field: &str, production: bool) -> Result<()> {
    if secret.len() < 32 {
        return Err(anyhow!("{field} must be at least 32 characters long"));
    }
    if production && secret == "change-me-to-a-random-string-with-at-least-32-characters" {
        return Err(anyhow!(
            "example {field} is not allowed in production environment"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{JwtConfig, JwtKeyConfig, JwtKeySet};

    #[test]
    fn debug_redacts_both_secret_sources() {
        let mut config = JwtConfig {
            secret: "single-secret-must-not-appear-in-debug".to_string(),
            key_set: None,
            issuer: "issuer".to_string(),
            audience: "audience".to_string(),
            access_token_ttl_minutes: 120,
        };
        assert!(!format!("{config:?}").contains(&config.secret));
        config.key_set = Some(JwtKeySet {
            current_kid: "A".to_string(),
            legacy_kid: None,
            keys: vec![JwtKeyConfig {
                kid: "A".to_string(),
                secret: "rotating-secret-must-not-appear-in-debug".to_string(),
                accept_until: None,
            }],
        });
        let debug = format!("{config:?}");
        assert!(!debug.contains(&config.secret));
        assert!(!debug.contains(&config.key_set.as_ref().unwrap().keys[0].secret));
        assert!(debug.contains("[REDACTED]"));
    }
}
