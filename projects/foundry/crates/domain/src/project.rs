use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Active,
    Archived,
}

impl ProjectStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    Viewer,
    Member,
    Maintainer,
    Owner,
}

impl ProjectRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Maintainer => "maintainer",
            Self::Owner => "owner",
        }
    }

    pub fn can_contribute(self) -> bool {
        self >= Self::Member
    }

    pub fn can_dispose(self) -> bool {
        self >= Self::Maintainer
    }

    pub fn can_manage_members(self) -> bool {
        self == Self::Owner
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMember {
    pub account_id: Uuid,
    pub role: ProjectRole,
    #[serde(with = "time::serde::rfc3339")]
    pub added_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: ProjectStatus,
    #[serde(default)]
    pub root_task_id: Option<Uuid>,
    #[serde(default)]
    pub members: Vec<ProjectMember>,
    pub version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("project validation failed: {}", .0.join("; "))]
pub struct ProjectValidationError(pub Vec<String>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectError {
    #[error("account {0} is already a member")]
    AlreadyMember(Uuid),
    #[error("account {0} is not a member")]
    NotAMember(Uuid),
    #[error("a project must keep at least one owner")]
    LastOwner,
    #[error("project is archived")]
    Archived,
}

impl Project {
    pub fn new(
        name: impl Into<String>,
        description: Option<String>,
        owner_id: Uuid,
    ) -> Result<Self, ProjectValidationError> {
        let now = OffsetDateTime::now_utc();
        let project = Self {
            id: Uuid::now_v7(),
            name: name.into().trim().to_string(),
            description,
            status: ProjectStatus::Active,
            root_task_id: None,
            members: vec![ProjectMember {
                account_id: owner_id,
                role: ProjectRole::Owner,
                added_at: now,
            }],
            version: 1,
            created_at: now,
            updated_at: now,
        };
        project.validate()?;
        Ok(project)
    }

    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        let mut errors = Vec::new();
        if self.name.trim().is_empty() {
            errors.push("name is required".to_string());
        }
        if self.members.is_empty() {
            errors.push("project must have at least one member".to_string());
        }
        if !self
            .members
            .iter()
            .any(|member| member.role == ProjectRole::Owner)
        {
            errors.push("project must have at least one owner".to_string());
        }
        let mut ids: Vec<Uuid> = self
            .members
            .iter()
            .map(|member| member.account_id)
            .collect();
        ids.sort();
        ids.dedup();
        if ids.len() != self.members.len() {
            errors.push("an account may appear only once in a project".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ProjectValidationError(errors))
        }
    }

    pub fn member(&self, account_id: Uuid) -> Option<&ProjectMember> {
        self.members
            .iter()
            .find(|member| member.account_id == account_id)
    }

    pub fn role_of(&self, account_id: Uuid) -> Option<ProjectRole> {
        self.member(account_id).map(|member| member.role)
    }

    pub fn is_archived(&self) -> bool {
        self.status == ProjectStatus::Archived
    }

    pub fn update(
        &mut self,
        name: Option<String>,
        description: Option<Option<String>>,
    ) -> Result<(), ProjectValidationError> {
        if self.is_archived() {
            return Err(ProjectValidationError(vec![
                "archived projects cannot be edited".to_string(),
            ]));
        }
        if let Some(name) = name {
            self.name = name.trim().to_string();
        }
        if let Some(description) = description {
            self.description = description.map(|value| value.trim().to_string());
        }
        self.touch();
        self.validate()
    }

    pub fn archive(&mut self) -> Result<(), ProjectError> {
        if self.is_archived() {
            return Ok(());
        }
        self.status = ProjectStatus::Archived;
        self.touch();
        Ok(())
    }

    pub fn activate(&mut self) -> Result<(), ProjectError> {
        self.status = ProjectStatus::Active;
        self.touch();
        Ok(())
    }

    pub fn add_member(&mut self, account_id: Uuid, role: ProjectRole) -> Result<(), ProjectError> {
        if self.is_archived() {
            return Err(ProjectError::Archived);
        }
        if self.member(account_id).is_some() {
            return Err(ProjectError::AlreadyMember(account_id));
        }
        self.members.push(ProjectMember {
            account_id,
            role,
            added_at: OffsetDateTime::now_utc(),
        });
        self.touch();
        Ok(())
    }

    pub fn set_member_role(
        &mut self,
        account_id: Uuid,
        role: ProjectRole,
    ) -> Result<(), ProjectError> {
        if self.is_archived() {
            return Err(ProjectError::Archived);
        }
        let index = self
            .members
            .iter()
            .position(|member| member.account_id == account_id)
            .ok_or(ProjectError::NotAMember(account_id))?;
        if self.members[index].role == ProjectRole::Owner
            && role != ProjectRole::Owner
            && self.owner_count() == 1
        {
            return Err(ProjectError::LastOwner);
        }
        self.members[index].role = role;
        self.touch();
        Ok(())
    }

    pub fn remove_member(&mut self, account_id: Uuid) -> Result<(), ProjectError> {
        if self.is_archived() {
            return Err(ProjectError::Archived);
        }
        let index = self
            .members
            .iter()
            .position(|member| member.account_id == account_id)
            .ok_or(ProjectError::NotAMember(account_id))?;
        if self.members[index].role == ProjectRole::Owner && self.owner_count() == 1 {
            return Err(ProjectError::LastOwner);
        }
        self.members.remove(index);
        self.touch();
        Ok(())
    }

    fn owner_count(&self) -> usize {
        self.members
            .iter()
            .filter(|member| member.role == ProjectRole::Owner)
            .count()
    }

    fn touch(&mut self) {
        self.version = self.version.saturating_add(1);
        self.updated_at = OffsetDateTime::now_utc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project::new("Payments", None, Uuid::now_v7()).unwrap()
    }

    #[test]
    fn new_project_has_an_owner() {
        let project = project();
        let owner = project.members[0].account_id;
        assert_eq!(project.role_of(owner), Some(ProjectRole::Owner));
        assert_eq!(project.status, ProjectStatus::Active);
    }

    #[test]
    fn cannot_remove_last_owner() {
        let mut project = project();
        let owner = project.members[0].account_id;
        assert!(matches!(
            project.remove_member(owner),
            Err(ProjectError::LastOwner)
        ));
    }

    #[test]
    fn owner_can_be_transferred() {
        let mut project = project();
        let first = project.members[0].account_id;
        let second = Uuid::now_v7();
        project.add_member(second, ProjectRole::Member).unwrap();
        project.set_member_role(second, ProjectRole::Owner).unwrap();
        project
            .set_member_role(first, ProjectRole::Maintainer)
            .unwrap();
        project.remove_member(first).unwrap();
        assert_eq!(project.role_of(second), Some(ProjectRole::Owner));
    }

    #[test]
    fn archived_project_is_read_only() {
        let mut project = project();
        project.archive().unwrap();
        assert!(matches!(
            project.add_member(Uuid::now_v7(), ProjectRole::Member),
            Err(ProjectError::Archived)
        ));
    }
}
