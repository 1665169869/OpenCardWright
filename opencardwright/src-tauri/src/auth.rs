//! 账号、角色与密码。
//!
//! 三条原则：
//! * 没有默认口令 —— 首次启动必须由人设置管理员密码，代码里不存在"万能密码"；
//! * 口令只存 Argon2id 哈希，永不回传前端；
//! * 会话只存在内存里，程序重启即失效（现场机器多人共用，宁可多登一次）。

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use serde::Serialize;

/// 密码规则的**展示文案**在前端 `src/utils/password.ts`（`PASSWORD_RULES`），
/// 校验逻辑在下面的 `validate_password`。两处必须同步改。
pub const MIN_PASSWORD_LEN: usize = 8;
pub const MAX_PASSWORD_LEN: usize = 128;

/// 超管账号固定为 `admin`（首次初始化时创建）。
pub const SUPER_ADMIN_LOGIN: &str = "admin";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Frontdesk,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Self::Admin),
            "frontdesk" => Some(Self::Frontdesk),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Frontdesk => "frontdesk",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub operator_id: i64,
    pub login: String,
    pub display_name: String,
    pub role: Role,
    /// 部门只是归属标签，用于展示与审计。
    pub department_name: Option<String>,
}

/// 校验密码规则。规则与 `PASSWORD_RULES` 一一对应，改这里就要改那里。
pub fn validate_password(password: &str, login: &str) -> Result<(), String> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(format!("密码至少 {MIN_PASSWORD_LEN} 位"));
    }
    if len > MAX_PASSWORD_LEN {
        return Err(format!("密码最长 {MAX_PASSWORD_LEN} 位"));
    }
    if !password.chars().any(|c| c.is_ascii_alphabetic()) {
        return Err("密码必须包含字母".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        return Err("密码必须包含数字".to_string());
    }
    let login = login.trim();
    if !login.is_empty() && password.eq_ignore_ascii_case(login) {
        return Err("密码不能与登录名相同".to_string());
    }
    Ok(())
}

/// 生成 Argon2id 哈希。规则不满足直接拒绝。
pub fn hash_password(password: &str, login: &str) -> Result<String, String> {
    validate_password(password, login)?;
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("密码哈希失败: {e}"))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_roundtrip() {
        let hash = hash_password("CorrectHorse1", "admin").expect("应能哈希");
        assert!(hash.starts_with("$argon2id$"), "必须是 Argon2id: {hash}");
        assert!(verify_password("CorrectHorse1", &hash));
    }

    #[test]
    fn wrong_password_is_rejected() {
        let hash = hash_password("CorrectHorse1", "admin").unwrap();
        assert!(!verify_password("wrong password!!", &hash));
    }

    #[test]
    fn short_password_is_rejected() {
        let err = hash_password("Abc123", "admin").unwrap_err();
        assert!(err.contains("至少"), "{err}");
    }

    #[test]
    fn password_needs_letter_and_digit() {
        assert!(validate_password("abcdefgh", "u").unwrap_err().contains("数字"));
        assert!(validate_password("12345678", "u").unwrap_err().contains("字母"));
        assert!(validate_password("abc12345", "u").is_ok());
    }

    #[test]
    fn password_must_differ_from_login() {
        // 长度和组成都合格，单纯因为与登录名相同而被拒
        let err = validate_password("admin123", "admin123").unwrap_err();
        assert!(err.contains("登录名"), "{err}");
        // 大小写不敏感
        assert!(validate_password("ADMIN123", "admin123").is_err());
        // 与登录名不同则放行
        assert!(validate_password("admin124", "admin123").is_ok());
    }

    #[test]
    fn overlong_password_is_rejected() {
        let long = format!("a1{}", "x".repeat(MAX_PASSWORD_LEN));
        assert!(validate_password(&long, "u").unwrap_err().contains("最长"));
    }

    #[test]
    fn malformed_hash_never_panics_or_accepts() {
        assert!(!verify_password("anything", ""));
        assert!(!verify_password("anything", "not-a-hash"));
    }

    #[test]
    fn role_roundtrip() {
        assert_eq!(Role::parse("admin"), Some(Role::Admin));
        assert_eq!(Role::parse("frontdesk"), Some(Role::Frontdesk));
        assert_eq!(Role::parse("root"), None);
        assert_eq!(Role::Admin.as_str(), "admin");
    }
}
