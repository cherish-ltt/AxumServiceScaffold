use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

use crate::error::AppError;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    if password.trim().is_empty() {
        return Err(AppError::bad_request("密码不能为空"));
    }

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hashed| hashed.to_string())
        .map_err(|error| AppError::internal(format!("Argon2 哈希失败: {error}")))
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<bool, AppError> {
    if password.trim().is_empty() {
        return Err(AppError::bad_request("密码不能为空"));
    }

    let parsed_hash = PasswordHash::new(password_hash)
        .map_err(|error| AppError::bad_request(format!("密码哈希格式无效: {error}")))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::{hash_password, verify_password};

    #[test]
    fn password_roundtrip_works() {
        let password = "S3cure-Password!";
        let hashed = hash_password(password).expect("hash password");

        assert_ne!(hashed, password);
        assert!(verify_password(password, &hashed).expect("verify password"));
        assert!(!verify_password("wrong-password", &hashed).expect("verify wrong password"));
    }
}
