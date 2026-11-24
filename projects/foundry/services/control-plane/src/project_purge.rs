//! Permanent project deletion.
//!
//! Deleting a project removes its agents, workspaces, logs, capsules, SSH
//! access, Arche repository and source cache. The database delete is
//! synchronous so the project disappears from the UI immediately; stopping the
//! remaining processes and removing the filesystem resources run in the
//! background.

use crate::arche_ssh::ArcheSsh;
use crate::supervisor::AgentSupervisor;
use foundry_adapter_postgres::PostgresStore;
use foundry_api_http::ProjectPurger as ProjectPurgerPort;
use foundry_app::{ProjectUseCases, ServiceError};
use foundry_domain::{Actor, AgentDefinition};
use foundry_shared::AgentLogFiles;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub struct ProjectPurger {
    projects: Arc<dyn ProjectUseCases>,
    store: Arc<PostgresStore>,
    supervisor: Arc<AgentSupervisor>,
    design_root: PathBuf,
    agent_logs: Arc<AgentLogFiles>,
    arche_ssh: Arc<ArcheSsh>,
    foundry_dir: PathBuf,
}

impl ProjectPurger {
    pub fn new(
        projects: Arc<dyn ProjectUseCases>,
        store: Arc<PostgresStore>,
        supervisor: Arc<AgentSupervisor>,
        design_root: PathBuf,
        agent_logs: Arc<AgentLogFiles>,
        arche_ssh: Arc<ArcheSsh>,
    ) -> Self {
        Self {
            projects,
            store,
            supervisor,
            design_root,
            agent_logs,
            arche_ssh,
            foundry_dir: foundry_shared::default_foundry_dir(),
        }
    }

    async fn purge_inner(&self, actor: Actor, project_id: Uuid) -> Result<(), ServiceError> {
        self.projects.get(actor.clone(), project_id).await?;
        let definitions = self
            .store
            .list_project_agent_definitions(project_id)
            .await
            .map_err(|error| ServiceError::Internal(error.to_string()))?;

        // Capture pids before the database rows disappear, then tell the
        // agents to stop. Waiting for them and removing resources happens in
        // the background.
        let pids = self.project_processes(&definitions, project_id).await;
        for definition in &definitions {
            if let Some(pid) = pids.get(&definition.id) {
                self.supervisor.stop_pid(definition.id, *pid).await;
            }
        }

        // Delete the database rows now so the project leaves the UI. If this
        // fails, the project still exists and nothing is cleaned up.
        self.projects.delete(actor, project_id).await?;

        let cleanup = PurgeCleanup {
            store: Arc::clone(&self.store),
            design_root: self.design_root.clone(),
            agent_logs: Arc::clone(&self.agent_logs),
            arche_ssh: Arc::clone(&self.arche_ssh),
            foundry_dir: self.foundry_dir.clone(),
            project_id,
            definitions,
            pids,
        };
        tokio::spawn(async move { cleanup.run().await });
        Ok(())
    }

    async fn project_processes(
        &self,
        definitions: &[AgentDefinition],
        project_id: Uuid,
    ) -> HashMap<Uuid, i32> {
        let processes = match self.store.list_agent_processes().await {
            Ok(processes) => processes,
            Err(error) => {
                tracing::warn!(
                    project_id = %project_id,
                    %error,
                    "cannot list project agent processes"
                );
                return HashMap::new();
            }
        };
        let ids = definitions
            .iter()
            .map(|definition| definition.id)
            .collect::<Vec<_>>();
        processes
            .into_iter()
            .filter(|process| ids.contains(&process.agent_definition_id))
            .filter_map(|process| process.pid.map(|pid| (process.agent_definition_id, pid)))
            .collect()
    }
}

struct PurgeCleanup {
    store: Arc<PostgresStore>,
    design_root: PathBuf,
    agent_logs: Arc<AgentLogFiles>,
    arche_ssh: Arc<ArcheSsh>,
    foundry_dir: PathBuf,
    project_id: Uuid,
    definitions: Vec<AgentDefinition>,
    pids: HashMap<Uuid, i32>,
}

impl PurgeCleanup {
    async fn run(self) {
        self.wait_for_agents().await;
        self.remove_design_space();
        self.remove_source_cache();
        for definition in &self.definitions {
            self.remove_workspace(definition);
            self.remove_log(definition);
            self.remove_capsules(definition.id);
            self.remove_agent_account(definition).await;
        }
    }

    async fn wait_for_agents(&self) {
        for (id, pid) in &self.pids {
            for _ in 0..50 {
                if !process_alive(*pid) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if process_alive(*pid) {
                tracing::warn!(agent_id = %id, pid, "agent did not stop during project cleanup");
            }
        }
    }

    fn remove_design_space(&self) {
        let repo = self.design_root.join(self.project_id.to_string());
        if !repo.starts_with(&self.design_root) {
            tracing::warn!(path = %repo.display(), "refusing to remove design space outside root");
            return;
        }
        match std::fs::remove_dir_all(&repo) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(path = %repo.display(), %error, "cannot remove design space")
            }
        }
    }

    fn remove_source_cache(&self) {
        let cache = self.foundry_dir.join("source-cache");
        let prefix = format!("{}-", self.project_id);
        let Ok(entries) = std::fs::read_dir(&cache) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with(&prefix) {
                continue;
            }
            let path = entry.path();
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(error) = result {
                tracing::warn!(path = %path.display(), %error, "cannot remove source cache");
            }
        }
    }

    fn remove_workspace(&self, definition: &AgentDefinition) {
        let workspace = foundry_shared::default_agent_workspace_dir(&definition.id.to_string())
            .join(self.project_id.to_string());
        match std::fs::remove_dir_all(&workspace) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(path = %workspace.display(), %error, "cannot remove agent workspace")
            }
        }
    }

    fn remove_log(&self, definition: &AgentDefinition) {
        let path = self.agent_logs.path(&definition.id.to_string());
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(path = %path.display(), %error, "cannot remove agent log"),
        }
    }

    fn remove_capsules(&self, agent_id: Uuid) {
        let program = std::env::var("PODMAN").unwrap_or_else(|_| "podman".to_string());
        let output = Command::new(&program)
            .args([
                "ps",
                "-a",
                "--filter",
                &format!("label=foundry.agent_id={agent_id}"),
                "-q",
            ])
            .output();
        let Ok(output) = output else {
            tracing::warn!(agent_id = %agent_id, "cannot list agent capsules");
            return;
        };
        if !output.status.success() {
            tracing::warn!(
                agent_id = %agent_id,
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "cannot list agent capsules"
            );
            return;
        }
        let ids = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return;
        }
        let mut command = Command::new(&program);
        command.arg("rm").arg("-f");
        command.args(&ids);
        if let Ok(output) = command.output()
            && !output.status.success()
        {
            tracing::warn!(
                agent_id = %agent_id,
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "cannot remove agent capsules"
            );
        }
    }

    async fn remove_agent_account(&self, definition: &AgentDefinition) {
        // Only remove an agent account that is not shared with another
        // definition. The project's definitions are already deleted.
        let deleted = sqlx::query(
            "delete from accounts a \
             where a.id = $1 and a.kind = 'agent' \
               and not exists (select 1 from agent_definitions d where d.account_id = a.id)",
        )
        .bind(definition.account_id)
        .execute(self.store.pool())
        .await;
        match deleted {
            Ok(result) if result.rows_affected() > 0 => {}
            Ok(_) => {}
            Err(error) => tracing::warn!(
                account_id = %definition.account_id,
                %error,
                "cannot remove agent account"
            ),
        }
        if let Err(error) = self.arche_ssh.revoke_account(definition.account_id).await {
            tracing::warn!(
                account_id = %definition.account_id,
                %error,
                "cannot revoke agent SSH access"
            );
        }
    }
}

fn process_alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[async_trait::async_trait]
impl ProjectPurgerPort for ProjectPurger {
    async fn purge(&self, actor: Actor, project_id: Uuid) -> Result<(), ServiceError> {
        self.purge_inner(actor, project_id).await
    }
}
