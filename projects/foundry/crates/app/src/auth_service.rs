use crate::auth::{hash_token, new_session_token, verify_password};
use crate::error::ServiceError;
use crate::ports::{AccountRepository, SessionRepository};
use foundry_domain::{Account, Session};
use std::sync::Arc;
use time::OffsetDateTime;

pub struct AuthService<A, S> {
    accounts: Arc<A>,
    sessions: Arc<S>,
    session_ttl_hours: i64,
}

#[derive(Debug, Clone)]
pub struct LoginResult {
    pub token: String,
    pub expires_at: OffsetDateTime,
    pub account: Account,
}

impl<A, S> AuthService<A, S>
where
    A: AccountRepository,
    S: SessionRepository,
{
    pub fn new(accounts: Arc<A>, sessions: Arc<S>, session_ttl_hours: i64) -> Self {
        Self {
            accounts,
            sessions,
            session_ttl_hours,
        }
    }

    pub async fn login(&self, handle: &str, password: &str) -> Result<LoginResult, ServiceError> {
        let account = self
            .accounts
            .get_by_handle(handle.trim())
            .await?
            .ok_or_else(invalid_credentials)?;

        let Some(password_hash) = account.password_hash.as_deref() else {
            return Err(invalid_credentials());
        };
        if !verify_password(password, password_hash) {
            return Err(invalid_credentials());
        }
        if !account.is_active() {
            return Err(ServiceError::Unauthorized(
                "account is suspended".to_string(),
            ));
        }

        let token = new_session_token();
        let session = Session::new(account.id, hash_token(&token), self.session_ttl_hours);
        self.sessions.insert(&session).await?;
        Ok(LoginResult {
            token,
            expires_at: session.expires_at,
            account,
        })
    }

    pub async fn authenticate(&self, token: &str) -> Result<Option<Account>, ServiceError> {
        if token.trim().is_empty() {
            return Ok(None);
        }
        let Some(session) = self.sessions.get_by_token_hash(&hash_token(token)).await? else {
            return Ok(None);
        };
        if !session.is_valid(OffsetDateTime::now_utc()) {
            return Ok(None);
        }
        let Some(account) = self.accounts.get(session.account_id).await? else {
            return Ok(None);
        };
        if !account.is_active() {
            return Ok(None);
        }
        Ok(Some(account))
    }

    pub async fn logout(&self, token: &str) -> Result<(), ServiceError> {
        if token.trim().is_empty() {
            return Ok(());
        }
        let token_hash = hash_token(token);
        let Some(mut session) = self.sessions.get_by_token_hash(&token_hash).await? else {
            return Ok(());
        };
        if session.revoked_at.is_none() {
            session.revoke(OffsetDateTime::now_utc());
            self.sessions.save(&session).await?;
        }
        Ok(())
    }
}

fn invalid_credentials() -> ServiceError {
    ServiceError::Unauthorized("invalid handle or password".to_string())
}

#[async_trait::async_trait]
pub trait AuthUseCases: Send + Sync {
    async fn login(&self, handle: &str, password: &str) -> Result<LoginResult, ServiceError>;
    async fn authenticate(&self, token: &str) -> Result<Option<Account>, ServiceError>;
    async fn logout(&self, token: &str) -> Result<(), ServiceError>;
}

#[async_trait::async_trait]
impl<A, S> AuthUseCases for AuthService<A, S>
where
    A: AccountRepository,
    S: SessionRepository,
{
    async fn login(&self, handle: &str, password: &str) -> Result<LoginResult, ServiceError> {
        AuthService::login(self, handle, password).await
    }

    async fn authenticate(&self, token: &str) -> Result<Option<Account>, ServiceError> {
        AuthService::authenticate(self, token).await
    }

    async fn logout(&self, token: &str) -> Result<(), ServiceError> {
        AuthService::logout(self, token).await
    }
}
