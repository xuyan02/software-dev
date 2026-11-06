use crate::Actor;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    #[default]
    Human,
    Agent,
}

impl AccountKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountRole {
    Admin,
    #[default]
    Member,
}

impl AccountRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    pub fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    #[default]
    Active,
    Suspended,
}

impl AccountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
        }
    }
}

/// A human or agent identity.
///
/// `password_hash` is never serialized to API or web responses. Persistence
/// adapters store it as a separate column or keep it in the aggregate snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub kind: AccountKind,
    pub role: AccountRole,
    pub status: AccountStatus,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default, skip_serializing)]
    pub password_hash: Option<String>,
    pub version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("account validation failed: {}", .0.join("; "))]
pub struct AccountValidationError(pub Vec<String>);

impl Account {
    pub fn new(
        handle: impl Into<String>,
        display_name: impl Into<String>,
        kind: AccountKind,
        role: AccountRole,
        email: Option<String>,
        password_hash: Option<String>,
    ) -> Result<Self, AccountValidationError> {
        let now = OffsetDateTime::now_utc();
        let account = Self {
            id: Uuid::now_v7(),
            handle: handle.into().trim().to_lowercase(),
            display_name: display_name.into().trim().to_string(),
            kind,
            role,
            status: AccountStatus::Active,
            email: email.map(|value| value.trim().to_string()),
            password_hash,
            version: 1,
            created_at: now,
            updated_at: now,
        };
        account.validate()?;
        Ok(account)
    }

    pub fn validate(&self) -> Result<(), AccountValidationError> {
        let mut errors = Vec::new();
        if !valid_handle(&self.handle) {
            errors.push(
                "handle must be 3-32 lowercase letters, digits, '-' or '_', starting with a letter"
                    .to_string(),
            );
        }
        if self.display_name.trim().is_empty() {
            errors.push("display name is required".to_string());
        }
        if self
            .email
            .as_deref()
            .is_some_and(|email| !email.is_empty() && !email.contains('@'))
        {
            errors.push("email must contain '@'".to_string());
        }
        if self.password_hash.is_none() && self.kind == AccountKind::Human {
            errors.push("human accounts require a password".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AccountValidationError(errors))
        }
    }

    pub fn actor(&self) -> Actor {
        match self.kind {
            AccountKind::Human => Actor::Human {
                id: self.handle.clone(),
                name: Some(self.display_name.clone()),
            },
            AccountKind::Agent => Actor::agent(self.handle.clone(), "agent"),
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == AccountStatus::Active
    }

    pub fn is_admin(&self) -> bool {
        self.role.is_admin()
    }

    pub fn update_profile(
        &mut self,
        display_name: Option<String>,
        email: Option<Option<String>>,
    ) -> Result<(), AccountValidationError> {
        if let Some(display_name) = display_name {
            self.display_name = display_name.trim().to_string();
        }
        if let Some(email) = email {
            self.email = email.map(|value| value.trim().to_string());
        }
        self.touch();
        self.validate()
    }

    /// Apply an administrative update in one version bump.
    pub fn apply_update(
        &mut self,
        display_name: Option<String>,
        email: Option<Option<String>>,
        role: Option<AccountRole>,
        status: Option<AccountStatus>,
        password_hash: Option<String>,
    ) -> Result<(), AccountValidationError> {
        if let Some(display_name) = display_name {
            self.display_name = display_name.trim().to_string();
        }
        if let Some(email) = email {
            self.email = email.map(|value| value.trim().to_string());
        }
        if let Some(role) = role {
            self.role = role;
        }
        if let Some(status) = status {
            self.status = status;
        }
        if let Some(password_hash) = password_hash {
            self.password_hash = Some(password_hash);
        }
        self.touch();
        self.validate()
    }

    pub fn set_role(&mut self, role: AccountRole) {
        self.role = role;
        self.touch();
    }

    pub fn set_status(&mut self, status: AccountStatus) {
        self.status = status;
        self.touch();
    }

    pub fn set_password_hash(&mut self, password_hash: String) {
        self.password_hash = Some(password_hash);
        self.touch();
    }

    fn touch(&mut self) {
        self.version = self.version.saturating_add(1);
        self.updated_at = OffsetDateTime::now_utc();
    }
}

fn valid_handle(handle: &str) -> bool {
    let mut chars = handle.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    if !(3..=32).contains(&handle.len()) {
        return false;
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

/// A browser or bearer session for an account.
///
/// The raw token is only returned once at login; storage keeps a SHA-256 hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub account_id: Uuid,
    pub token_hash: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
}

impl Session {
    pub fn new(account_id: Uuid, token_hash: String, ttl_hours: i64) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: Uuid::now_v7(),
            account_id,
            token_hash,
            created_at: now,
            expires_at: now + time::Duration::hours(ttl_hours.max(1)),
            revoked_at: None,
        }
    }

    pub fn is_valid(&self, now: OffsetDateTime) -> bool {
        self.revoked_at.is_none() && self.expires_at > now
    }

    pub fn revoke(&mut self, now: OffsetDateTime) {
        self.revoked_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_account_requires_password() {
        let error = Account::new(
            "alice",
            "Alice",
            AccountKind::Human,
            AccountRole::Member,
            None,
            None,
        )
        .expect_err("human accounts need a password");
        assert!(error.to_string().contains("password"));
    }

    #[test]
    fn handle_is_normalized_and_validated() {
        let account = Account::new(
            " Alice ",
            "Alice",
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        assert_eq!(account.handle, "alice");
        assert!(account.is_active());
        assert!(!account.is_admin());
    }

    #[test]
    fn agent_account_may_have_a_service_secret() {
        let account = Account::new(
            "review-bot",
            "Review Bot",
            AccountKind::Agent,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        assert!(account.actor().is_agent());
    }

    #[test]
    fn session_expires() {
        let session = Session::new(Uuid::now_v7(), "hash".to_string(), 1);
        assert!(session.is_valid(OffsetDateTime::now_utc()));
        assert!(!session.is_valid(session.expires_at + time::Duration::seconds(1)));
    }
}
