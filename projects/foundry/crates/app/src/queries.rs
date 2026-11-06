use crate::ports::RequirementFilter;
use foundry_domain::Requirement;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetRequirement {
    pub id: Uuid,
}

#[derive(Debug, Clone, Default)]
pub struct ListRequirements {
    pub status: Option<foundry_domain::RequirementStatus>,
    pub owner: Option<String>,
    pub text: Option<String>,
}

impl From<ListRequirements> for RequirementFilter {
    fn from(query: ListRequirements) -> Self {
        Self {
            status: query.status,
            owner: query.owner,
            text: query.text,
        }
    }
}

/// Read-side port. It can be implemented by a projection/database directly;
/// read models are allowed to bypass the write-side aggregates.
#[async_trait::async_trait]
pub trait RequirementQuery: Send + Sync {
    async fn get(&self, id: Uuid) -> Result<Option<Requirement>, crate::error::RepoError>;
    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, crate::error::RepoError>;
}
