//! In-memory adapters for local development, tests and demos.
//!
//! These are deliberately simple. They implement the same optimistic
//! concurrency contract as the Postgres adapter so the same tests can run
//! against both.

use foundry_app::{
    AccountFilter, AccountRepository, EventError, EventSink, IssueFilter, IssueRepository,
    ProjectFilter, ProjectRepository, RepoError, RequirementFilter, RequirementQuery,
    RequirementRepository, SessionRepository,
};
use foundry_domain::{Account, DomainEvent, Issue, Project, Requirement, Session, TaskDAO};
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

mod task;
mod workflow;

pub use task::*;
use workflow::MemoryWorkflows;

#[derive(Default)]
pub struct InMemoryStore {
    requirements: RwLock<HashMap<Uuid, Requirement>>,
    issues: RwLock<HashMap<Uuid, Issue>>,
    accounts: RwLock<HashMap<Uuid, Account>>,
    projects: RwLock<HashMap<Uuid, Project>>,
    sessions: RwLock<HashMap<Uuid, Session>>,
    pub(crate) tasks: RwLock<MemoryTasks>,
    pub(crate) workflows: RwLock<MemoryWorkflows>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.requirements.read().expect("lock poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn issue_count(&self) -> usize {
        self.issues.read().expect("lock poisoned").len()
    }

    pub fn account_count(&self) -> usize {
        self.accounts.read().expect("lock poisoned").len()
    }

    pub fn project_count(&self) -> usize {
        self.projects.read().expect("lock poisoned").len()
    }
}

#[async_trait::async_trait]
impl RequirementRepository for InMemoryStore {
    async fn insert(&self, requirement: &Requirement) -> Result<(), RepoError> {
        let mut requirements = self.requirements.write().expect("lock poisoned");
        if requirements.contains_key(&requirement.id) {
            return Err(RepoError::Backend(format!(
                "requirement {} already exists",
                requirement.id
            )));
        }
        requirements.insert(requirement.id, requirement.clone());
        Ok(())
    }

    async fn save(
        &self,
        requirement: &Requirement,
        expected_version: u32,
    ) -> Result<(), RepoError> {
        let mut requirements = self.requirements.write().expect("lock poisoned");
        let stored = requirements
            .get(&requirement.id)
            .ok_or(RepoError::NotFound)?;
        if stored.version != expected_version {
            return Err(RepoError::Conflict {
                expected: expected_version,
                actual: stored.version,
            });
        }
        requirements.insert(requirement.id, requirement.clone());
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Requirement>, RepoError> {
        Ok(self
            .requirements
            .read()
            .expect("lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, RepoError> {
        let requirements = self.requirements.read().expect("lock poisoned");
        let mut result: Vec<Requirement> = requirements
            .values()
            .filter(|requirement| requirement.project_id == project_id)
            .filter(|requirement| {
                filter
                    .status
                    .is_none_or(|status| requirement.status == status)
                    && filter
                        .owner
                        .as_ref()
                        .is_none_or(|owner| requirement.owner.id() == owner)
                    && filter.text.as_ref().is_none_or(|text| {
                        let needle = text.to_lowercase();
                        requirement.title.to_lowercase().contains(&needle)
                            || requirement.problem.to_lowercase().contains(&needle)
                    })
            })
            .cloned()
            .collect();
        result.sort_by_key(|requirement| requirement.created_at);
        Ok(result)
    }
}

#[async_trait::async_trait]
impl IssueRepository for InMemoryStore {
    async fn insert(&self, issue: &Issue) -> Result<(), RepoError> {
        let mut issues = self.issues.write().expect("lock poisoned");
        if issues.contains_key(&issue.id) {
            return Err(RepoError::Backend(format!(
                "issue {} already exists",
                issue.id
            )));
        }
        issues.insert(issue.id, issue.clone());
        Ok(())
    }

    async fn save(&self, issue: &Issue, expected_version: u32) -> Result<(), RepoError> {
        let mut issues = self.issues.write().expect("lock poisoned");
        let stored = issues.get(&issue.id).ok_or(RepoError::NotFound)?;
        if stored.version != expected_version {
            return Err(RepoError::Conflict {
                expected: expected_version,
                actual: stored.version,
            });
        }
        issues.insert(issue.id, issue.clone());
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Issue>, RepoError> {
        Ok(self.issues.read().expect("lock poisoned").get(&id).cloned())
    }

    async fn list(&self, project_id: Uuid, filter: IssueFilter) -> Result<Vec<Issue>, RepoError> {
        let issues = self.issues.read().expect("lock poisoned");
        let mut result: Vec<Issue> = issues
            .values()
            .filter(|issue| issue.project_id == project_id)
            .filter(|issue| {
                filter.status.is_none_or(|status| issue.status == status)
                    && filter.kind.is_none_or(|kind| issue.kind == kind)
                    && filter
                        .source
                        .is_none_or(|source| issue.source.kind == source)
                    && filter.assignee.as_ref().is_none_or(|assignee| {
                        issue.assignee.as_ref().is_some_and(|a| a.id() == assignee)
                    })
                    && filter.text.as_ref().is_none_or(|text| {
                        let needle = text.to_lowercase();
                        issue.title.to_lowercase().contains(&needle)
                            || issue.description.to_lowercase().contains(&needle)
                    })
            })
            .cloned()
            .collect();
        // Newest first: the issue inbox is a queue, not an archive.
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result)
    }
}

#[async_trait::async_trait]
impl AccountRepository for InMemoryStore {
    async fn insert(&self, account: &Account) -> Result<(), RepoError> {
        let mut accounts = self.accounts.write().expect("lock poisoned");
        if accounts.contains_key(&account.id) {
            return Err(RepoError::Duplicate(format!("account id {}", account.id)));
        }
        if accounts
            .values()
            .any(|stored| stored.handle.eq_ignore_ascii_case(&account.handle))
        {
            return Err(RepoError::Duplicate(format!(
                "account handle {}",
                account.handle
            )));
        }
        accounts.insert(account.id, account.clone());
        Ok(())
    }

    async fn save(&self, account: &Account, expected_version: u32) -> Result<(), RepoError> {
        let mut accounts = self.accounts.write().expect("lock poisoned");
        if accounts
            .values()
            .any(|stored| stored.id != account.id && stored.handle == account.handle)
        {
            return Err(RepoError::Duplicate(format!(
                "account handle {}",
                account.handle
            )));
        }
        let stored = accounts.get(&account.id).ok_or(RepoError::NotFound)?;
        if stored.version != expected_version {
            return Err(RepoError::Conflict {
                expected: expected_version,
                actual: stored.version,
            });
        }
        accounts.insert(account.id, account.clone());
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Account>, RepoError> {
        Ok(self
            .accounts
            .read()
            .expect("lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn get_by_handle(&self, handle: &str) -> Result<Option<Account>, RepoError> {
        let handle = handle.trim().to_lowercase();
        Ok(self
            .accounts
            .read()
            .expect("lock poisoned")
            .values()
            .find(|account| account.handle == handle)
            .cloned())
    }

    async fn list(&self, filter: AccountFilter) -> Result<Vec<Account>, RepoError> {
        let accounts = self.accounts.read().expect("lock poisoned");
        let mut result: Vec<Account> = accounts
            .values()
            .filter(|account| {
                filter.kind.is_none_or(|kind| account.kind == kind)
                    && filter.status.is_none_or(|status| account.status == status)
                    && filter.text.as_ref().is_none_or(|text| {
                        let needle = text.to_lowercase();
                        account.handle.contains(&needle)
                            || account.display_name.to_lowercase().contains(&needle)
                    })
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| a.handle.cmp(&b.handle));
        Ok(result)
    }
}

#[async_trait::async_trait]
impl ProjectRepository for InMemoryStore {
    async fn insert(&self, project: &Project) -> Result<(), RepoError> {
        let mut projects = self.projects.write().expect("lock poisoned");
        if projects.contains_key(&project.id) {
            return Err(RepoError::Duplicate(format!("project id {}", project.id)));
        }
        projects.insert(project.id, project.clone());
        Ok(())
    }

    async fn insert_with_root(
        &self,
        project: &Project,
        root: &TaskDAO,
        events: &[DomainEvent],
    ) -> Result<(), RepoError> {
        let mut projects = self.projects.write().expect("lock poisoned");
        if projects.contains_key(&project.id) {
            return Err(RepoError::Duplicate(format!("project id {}", project.id)));
        }
        let mut tasks = self.tasks.write().expect("lock poisoned");
        tasks.insert(root, &[], events)?;
        projects.insert(project.id, project.clone());
        Ok(())
    }

    async fn save(&self, project: &Project, expected_version: u32) -> Result<(), RepoError> {
        let mut projects = self.projects.write().expect("lock poisoned");
        let stored = projects.get(&project.id).ok_or(RepoError::NotFound)?;
        if stored.version != expected_version {
            return Err(RepoError::Conflict {
                expected: expected_version,
                actual: stored.version,
            });
        }
        projects.insert(project.id, project.clone());
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Project>, RepoError> {
        Ok(self
            .projects
            .read()
            .expect("lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn list(&self, filter: ProjectFilter) -> Result<Vec<Project>, RepoError> {
        let projects = self.projects.read().expect("lock poisoned");
        let mut result: Vec<Project> = projects
            .values()
            .filter(|project| {
                filter.status.is_none_or(|status| project.status == status)
                    && filter.text.as_ref().is_none_or(|text| {
                        let needle = text.to_lowercase();
                        project.name.to_lowercase().contains(&needle)
                    })
            })
            .cloned()
            .collect();
        result.sort_by_key(|project| project.created_at);
        Ok(result)
    }
}

#[async_trait::async_trait]
impl SessionRepository for InMemoryStore {
    async fn insert(&self, session: &Session) -> Result<(), RepoError> {
        let mut sessions = self.sessions.write().expect("lock poisoned");
        if sessions.contains_key(&session.id) {
            return Err(RepoError::Duplicate(format!("session id {}", session.id)));
        }
        sessions.insert(session.id, session.clone());
        Ok(())
    }

    async fn get_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, RepoError> {
        Ok(self
            .sessions
            .read()
            .expect("lock poisoned")
            .values()
            .find(|session| session.token_hash == token_hash)
            .cloned())
    }

    async fn save(&self, session: &Session) -> Result<(), RepoError> {
        let mut sessions = self.sessions.write().expect("lock poisoned");
        if !sessions.contains_key(&session.id) {
            return Err(RepoError::NotFound);
        }
        sessions.insert(session.id, session.clone());
        Ok(())
    }
}

#[async_trait::async_trait]
impl RequirementQuery for InMemoryStore {
    async fn get(&self, id: Uuid) -> Result<Option<Requirement>, RepoError> {
        RequirementRepository::get(self, id).await
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, RepoError> {
        RequirementRepository::list(self, project_id, filter).await
    }
}

#[derive(Default)]
pub struct InMemoryEventSink {
    events: RwLock<Vec<DomainEvent>>,
}

impl InMemoryEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<DomainEvent> {
        self.events.read().expect("lock poisoned").clone()
    }
}

#[async_trait::async_trait]
impl EventSink for InMemoryEventSink {
    async fn publish(&self, events: &[DomainEvent]) -> Result<(), EventError> {
        self.events
            .write()
            .expect("lock poisoned")
            .extend_from_slice(events);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_app::{IssueRepository, RequirementRepository};
    use foundry_domain::{
        AcceptanceScenario, AccountKind, AccountRole, Actor, IssueKind, IssueSource,
        IssueSourceKind, Priority, Project, ProjectRole, Requirement, RiskClass,
    };

    fn project() -> Project {
        let owner = Account::new(
            "alice",
            "Alice",
            AccountKind::Human,
            AccountRole::Admin,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        Project::new("Foundry", None, owner.id).unwrap()
    }

    fn requirement(project_id: Uuid) -> Requirement {
        Requirement::new(
            project_id,
            "退款",
            "用户只能全额退款",
            vec![AcceptanceScenario {
                id: "S-1".to_string(),
                title: "部分退款".to_string(),
                given: vec![],
                when: vec![],
                then: vec!["退款金额等于请求金额".to_string()],
            }],
            Actor::human("alice"),
            Priority::P1,
            RiskClass::Medium,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn save_rejects_stale_version() {
        let store = InMemoryStore::new();
        let project = project();
        let original = requirement(project.id);
        RequirementRepository::insert(&store, &original)
            .await
            .unwrap();

        let mut updated = original.clone();
        updated.title = "新标题".to_string();
        updated.version = 2;
        RequirementRepository::save(&store, &updated, 1)
            .await
            .unwrap();

        let stale = RequirementRepository::save(&store, &updated, 1).await;
        assert!(matches!(
            stale,
            Err(RepoError::Conflict {
                expected: 1,
                actual: 2
            })
        ));
    }

    #[tokio::test]
    async fn list_filters_by_project_text_and_status() {
        let store = InMemoryStore::new();
        let project = project();
        let other_project = Project::new("Other", None, Uuid::now_v7()).unwrap();
        let first = requirement(project.id);
        let mut second = requirement(project.id);
        second.title = "导出报表".to_string();
        let foreign = requirement(other_project.id);
        RequirementRepository::insert(&store, &first).await.unwrap();
        RequirementRepository::insert(&store, &second)
            .await
            .unwrap();
        RequirementRepository::insert(&store, &foreign)
            .await
            .unwrap();

        let found = RequirementRepository::list(
            &store,
            project.id,
            RequirementFilter {
                text: Some("导出".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "导出报表");

        let visible = RequirementRepository::list(&store, project.id, RequirementFilter::default())
            .await
            .unwrap();
        assert_eq!(visible.len(), 2);
    }

    fn issue(project_id: Uuid, title: &str) -> Issue {
        Issue::new(
            project_id,
            title,
            "描述",
            IssueKind::Feedback,
            IssueSource {
                kind: IssueSourceKind::Customer,
                reference: None,
            },
            Actor::human("reporter"),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn issue_save_rejects_stale_version() {
        let store = InMemoryStore::new();
        let project = project();
        let original = issue(project.id, "退款太慢");
        IssueRepository::insert(&store, &original).await.unwrap();

        let mut updated = original.clone();
        updated.title = "退款很慢".to_string();
        updated.version = 2;
        IssueRepository::save(&store, &updated, 1).await.unwrap();

        let stale = IssueRepository::save(&store, &updated, 1).await;
        assert!(matches!(
            stale,
            Err(RepoError::Conflict {
                expected: 1,
                actual: 2
            })
        ));
    }

    #[tokio::test]
    async fn issue_list_filters_and_sorts_newest_first() {
        let store = InMemoryStore::new();
        let project = project();
        let first = issue(project.id, "第一条");
        let mut second = issue(project.id, "第二条");
        second.kind = IssueKind::Bug;
        IssueRepository::insert(&store, &first).await.unwrap();
        IssueRepository::insert(&store, &second).await.unwrap();

        let bugs = IssueRepository::list(
            &store,
            project.id,
            IssueFilter {
                kind: Some(IssueKind::Bug),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(bugs.len(), 1);
        assert_eq!(bugs[0].title, "第二条");
    }

    #[tokio::test]
    async fn duplicate_handles_and_project_ids_are_rejected() {
        let store = InMemoryStore::new();
        let first = Account::new(
            "alice",
            "Alice",
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        AccountRepository::insert(&store, &first).await.unwrap();

        let duplicate = Account::new(
            "Alice",
            "Another Alice",
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        assert!(matches!(
            AccountRepository::insert(&store, &duplicate).await,
            Err(RepoError::Duplicate(_))
        ));

        let project = Project::new("Foundry", None, first.id).unwrap();
        ProjectRepository::insert(&store, &project).await.unwrap();
        let duplicate_project = project.clone();
        assert!(matches!(
            ProjectRepository::insert(&store, &duplicate_project).await,
            Err(RepoError::Duplicate(_))
        ));
    }

    #[tokio::test]
    async fn project_membership_is_persisted() {
        let store = InMemoryStore::new();
        let owner = Account::new(
            "alice",
            "Alice",
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        let project = Project::new("Foundry", None, owner.id).unwrap();
        ProjectRepository::insert(&store, &project).await.unwrap();
        let loaded = ProjectRepository::get(&store, project.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.role_of(owner.id), Some(ProjectRole::Owner));
    }
}
