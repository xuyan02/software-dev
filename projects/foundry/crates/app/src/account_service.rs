use crate::auth::hash_password;
use crate::commands::{CreateAccount, UpdateAccount};
use crate::error::ServiceError;
use crate::ports::{AccountFilter, AccountRepository, EventSink};
use foundry_domain::{Account, AccountStatus, Actor, DomainEvent, DomainEventKind};
use std::sync::Arc;
use uuid::Uuid;

pub struct AccountService<A, E> {
    accounts: Arc<A>,
    events: Arc<E>,
}

impl<A, E> AccountService<A, E>
where
    A: AccountRepository,
    E: EventSink,
{
    pub fn new(accounts: Arc<A>, events: Arc<E>) -> Self {
        Self { accounts, events }
    }

    pub async fn create(
        &self,
        actor: Actor,
        command: CreateAccount,
    ) -> Result<Account, ServiceError> {
        let admin = self.require_admin(&actor).await?;
        let password_hash = hash_password(&command.password)?;
        let account = Account::new(
            command.handle,
            command.display_name,
            command.kind,
            command.role,
            command.email,
            Some(password_hash),
        )
        .map_err(|error| ServiceError::Validation(error.to_string()))?;

        self.accounts.insert(&account).await?;
        self.emit(DomainEventKind::AccountCreated, &account, admin.actor())
            .await?;
        Ok(account)
    }

    pub async fn update(
        &self,
        actor: Actor,
        command: UpdateAccount,
    ) -> Result<Account, ServiceError> {
        let admin = self.require_admin(&actor).await?;
        let mut account = self
            .accounts
            .get(command.id)
            .await?
            .ok_or_else(|| ServiceError::not_found("account", command.id))?;

        if account.is_admin()
            && (command.role.is_some_and(|role| !role.is_admin())
                || command.status == Some(AccountStatus::Suspended))
        {
            let admins = self
                .accounts
                .list(AccountFilter {
                    status: Some(AccountStatus::Active),
                    ..Default::default()
                })
                .await?;
            let remaining = admins
                .iter()
                .filter(|candidate| candidate.is_admin() && candidate.id != account.id)
                .count();
            if remaining == 0 {
                return Err(ServiceError::Validation(
                    "cannot demote or suspend the last active admin".to_string(),
                ));
            }
        }

        let password_hash = command
            .new_password
            .as_deref()
            .map(hash_password)
            .transpose()?;
        let previous_version = account.version;
        account
            .apply_update(
                command.display_name,
                command.email,
                command.role,
                command.status,
                password_hash,
            )
            .map_err(|error| ServiceError::Validation(error.to_string()))?;

        self.accounts.save(&account, previous_version).await?;
        self.emit(DomainEventKind::AccountUpdated, &account, admin.actor())
            .await?;
        Ok(account)
    }

    pub async fn get(&self, actor: Actor, id: Uuid) -> Result<Account, ServiceError> {
        let viewer = self.require_actor(&actor).await?;
        if viewer.id != id && !viewer.is_admin() {
            return Err(ServiceError::Forbidden(
                "only admins can view other accounts".to_string(),
            ));
        }
        self.accounts
            .get(id)
            .await?
            .ok_or_else(|| ServiceError::not_found("account", id))
    }

    pub async fn list(
        &self,
        actor: Actor,
        filter: AccountFilter,
    ) -> Result<Vec<Account>, ServiceError> {
        self.require_admin(&actor).await?;
        Ok(self.accounts.list(filter).await?)
    }

    /// True while no accounts exist. The HTTP layer serves the first-run setup
    /// page in this state and blocks every other route until setup completes.
    pub async fn setup_required(&self) -> Result<bool, ServiceError> {
        Ok(self
            .accounts
            .list(AccountFilter::default())
            .await?
            .is_empty())
    }

    /// Creates the first administrator from the setup form.
    ///
    /// This is the only unauthenticated account write in the system and it is
    /// valid only while the account table is empty. Once any account exists it
    /// refuses, so it can never create or elevate accounts later.
    pub async fn create_first_admin(
        &self,
        handle: &str,
        display_name: &str,
        password: &str,
    ) -> Result<Account, ServiceError> {
        if !self.setup_required().await? {
            return Err(ServiceError::Forbidden(
                "setup is already complete".to_string(),
            ));
        }
        let password_hash = hash_password(password)?;
        let account = Account::new(
            handle,
            display_name,
            foundry_domain::AccountKind::Human,
            foundry_domain::AccountRole::Admin,
            None,
            Some(password_hash),
        )
        .map_err(|error| ServiceError::Validation(error.to_string()))?;
        self.accounts.insert(&account).await?;
        self.emit(DomainEventKind::AccountCreated, &account, Actor::System)
            .await?;
        Ok(account)
    }

    /// Returns an active administrator when accounts already exist.
    pub async fn require_active_admin(&self) -> Result<Account, ServiceError> {
        let accounts = self.accounts.list(AccountFilter::default()).await?;
        accounts
            .into_iter()
            .find(|account| account.is_admin() && account.is_active())
            .ok_or_else(|| {
                ServiceError::Unauthorized(
                    "accounts exist but none is an active administrator; restore an \
                     administrator account before starting the service"
                        .to_string(),
                )
            })
    }

    async fn require_actor(&self, actor: &Actor) -> Result<Account, ServiceError> {
        let account = self
            .accounts
            .get_by_handle(actor.id())
            .await?
            .ok_or_else(|| ServiceError::Unauthorized("unknown account".to_string()))?;
        if !account.is_active() {
            return Err(ServiceError::Unauthorized(
                "account is suspended".to_string(),
            ));
        }
        Ok(account)
    }

    async fn require_admin(&self, actor: &Actor) -> Result<Account, ServiceError> {
        let account = self.require_actor(actor).await?;
        if !account.is_admin() {
            return Err(ServiceError::Forbidden("admin role required".to_string()));
        }
        Ok(account)
    }

    async fn emit(
        &self,
        kind: DomainEventKind,
        account: &Account,
        actor: Actor,
    ) -> Result<(), ServiceError> {
        let payload = serde_json::to_value(account)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        self.events
            .publish(&[DomainEvent::new(kind, account.id, actor, payload)])
            .await?;
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait AccountUseCases: Send + Sync {
    async fn create(&self, actor: Actor, command: CreateAccount) -> Result<Account, ServiceError>;
    async fn update(&self, actor: Actor, command: UpdateAccount) -> Result<Account, ServiceError>;
    async fn get(&self, actor: Actor, id: Uuid) -> Result<Account, ServiceError>;
    async fn list(&self, actor: Actor, filter: AccountFilter)
    -> Result<Vec<Account>, ServiceError>;
    async fn setup_required(&self) -> Result<bool, ServiceError>;
    async fn create_first_admin(
        &self,
        handle: &str,
        display_name: &str,
        password: &str,
    ) -> Result<Account, ServiceError>;
}

#[async_trait::async_trait]
impl<A, E> AccountUseCases for AccountService<A, E>
where
    A: AccountRepository,
    E: EventSink,
{
    async fn create(&self, actor: Actor, command: CreateAccount) -> Result<Account, ServiceError> {
        AccountService::create(self, actor, command).await
    }

    async fn update(&self, actor: Actor, command: UpdateAccount) -> Result<Account, ServiceError> {
        AccountService::update(self, actor, command).await
    }

    async fn get(&self, actor: Actor, id: Uuid) -> Result<Account, ServiceError> {
        AccountService::get(self, actor, id).await
    }

    async fn list(
        &self,
        actor: Actor,
        filter: AccountFilter,
    ) -> Result<Vec<Account>, ServiceError> {
        AccountService::list(self, actor, filter).await
    }

    async fn setup_required(&self) -> Result<bool, ServiceError> {
        AccountService::setup_required(self).await
    }

    async fn create_first_admin(
        &self,
        handle: &str,
        display_name: &str,
        password: &str,
    ) -> Result<Account, ServiceError> {
        AccountService::create_first_admin(self, handle, display_name, password).await
    }
}
