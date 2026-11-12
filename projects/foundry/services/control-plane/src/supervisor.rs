//! Supervises one long-lived agentd process per agent definition.
//!
//! The supervisor lives in the control plane for the single-machine
//! deployment. It mints a short-lived session for the agent account, starts
//! `foundry-agentd` as a child process, and records process status in
//! `agent_processes`. The agentd process itself owns the run loop and
//! heartbeats.

use foundry_adapter_postgres::PostgresStore;
use foundry_app::{SessionRepository, hash_token, new_session_token};
use foundry_domain::Session;
use foundry_shared::AgentLogFiles;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Old process output is dropped when a new process starts above this size.
const MAX_AGENT_LOG_BYTES: u64 = 5 * 1024 * 1024;

pub struct AgentSupervisor {
    store: Arc<PostgresStore>,
    database_url: String,
    foundry_url: String,
    session_ttl_hours: i64,
    /// Platform model API key from config/env. Never persisted.
    api_key: String,
    agent_logs: Arc<AgentLogFiles>,
    running: Mutex<HashSet<Uuid>>,
    stopping: Mutex<HashSet<Uuid>>,
}

impl AgentSupervisor {
    pub fn new(
        store: Arc<PostgresStore>,
        database_url: String,
        foundry_url: String,
        session_ttl_hours: i64,
        api_key: String,
        agent_logs: Arc<AgentLogFiles>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            database_url,
            foundry_url,
            session_ttl_hours,
            api_key,
            agent_logs,
            running: Mutex::new(HashSet::new()),
            stopping: Mutex::new(HashSet::new()),
        })
    }

    /// Start every enabled definition. Called once at control-plane startup.
    pub async fn auto_start(self: &Arc<Self>) {
        let definitions = match self.store.list_agent_definitions().await {
            Ok(definitions) => definitions,
            Err(error) => {
                tracing::warn!(error = %error, "cannot list agent definitions");
                return;
            }
        };
        for definition in definitions
            .into_iter()
            .filter(|definition| definition.enabled)
        {
            if let Err(error) = self.start(definition.id).await {
                tracing::warn!(agent = %definition.name, error = %error, "agent auto-start failed");
            }
        }
    }

    pub async fn start(self: &Arc<Self>, id: Uuid) -> Result<(), String> {
        let definition = self
            .store
            .agent_definition(id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "agent definition not found".to_string())?;
        if !definition.enabled {
            tracing::warn!(agent_id = %id, "agent start rejected: disabled");
            return Err("agent is disabled".to_string());
        }
        if self.running.lock().await.contains(&id) {
            return Ok(());
        }
        if self.api_key.is_empty() {
            tracing::warn!(
                agent_id = %id,
                "agent start rejected: model API key is not configured"
            );
            return Err(
                "model API key is not configured in the Foundry config file or environment"
                    .to_string(),
            );
        }

        // A previous control-plane process may have left an agentd behind.
        if let Some(pid) = self.process_pid(id).await
            && process_alive(pid)
        {
            terminate(pid);
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        let token = new_session_token();
        let session = Session::new(
            definition.account_id,
            hash_token(&token),
            self.session_ttl_hours,
        );
        SessionRepository::insert(self.store.as_ref(), &session)
            .await
            .map_err(|error| error.to_string())?;

        self.agent_logs
            .ensure_dir()
            .map_err(|error| format!("cannot create agent log directory: {error}"))?;
        let agent_key = id.to_string();
        if let Err(error) = self
            .agent_logs
            .rotate_if_larger(&agent_key, MAX_AGENT_LOG_BYTES)
        {
            tracing::warn!(agent_id = %id, error = %error, "cannot rotate old agent log");
        }
        let log_path = self.agent_logs.path(&agent_key);

        let binary = agentd_binary();
        let mut command = tokio::process::Command::new(&binary);
        command
            .env("DATABASE_URL", &self.database_url)
            .env("FOUNDRY_URL", &self.foundry_url)
            .env("FOUNDRY_TOKEN", &token)
            .env("FOUNDRY_AGENT_ID", id.to_string())
            .env("FOUNDRY_AGENT_WORKER_ID", format!("agentd-{id}"))
            .env(&definition.api_key_env, &self.api_key)
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let message = format!("cannot start {}: {error}", binary.display());
                tracing::warn!(agent_id = %id, error = %message, "agentd spawn failed");
                return Err(message);
            }
        };
        if let Some(stdout) = child.stdout.take() {
            pipe_to_log(stdout, log_path.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            pipe_to_log(stderr, log_path);
        }

        let pid = child.id().unwrap_or(0) as i32;
        self.store
            .agent_process_started(id, pid)
            .await
            .map_err(|error| error.to_string())?;
        self.running.lock().await.insert(id);
        tracing::info!(agent_id = %id, pid, "agentd started");

        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            let status = child.wait().await;
            supervisor.running.lock().await.remove(&id);
            let deliberate = supervisor.stopping.lock().await.remove(&id);
            if deliberate {
                let _ = supervisor.store.agent_process_stopped(id).await;
                tracing::info!(agent_id = %id, "agentd stopped");
            } else {
                let message = match status {
                    Ok(status) => format!("agentd exited with {status}"),
                    Err(error) => error.to_string(),
                };
                let _ = supervisor.store.agent_process_failed(id, &message).await;
                tracing::warn!(agent_id = %id, error = %message, "agentd exited unexpectedly");
            }
        });
        Ok(())
    }

    pub async fn stop(self: &Arc<Self>, id: Uuid) -> Result<(), String> {
        self.stopping.lock().await.insert(id);
        if let Some(pid) = self.process_pid(id).await {
            terminate(pid);
        }
        if !self.running.lock().await.contains(&id) {
            // The child-exit task removes this entry before logging. Only log
            // here when that task has not handled the stop.
            let pending = self.stopping.lock().await.remove(&id);
            self.store
                .agent_process_stopped(id)
                .await
                .map_err(|error| error.to_string())?;
            if pending {
                tracing::info!(agent_id = %id, "agent stopped");
            }
        }
        Ok(())
    }

    pub async fn restart(self: &Arc<Self>, id: Uuid) -> Result<(), String> {
        self.stop(id).await?;
        for _ in 0..30 {
            if !self.running.lock().await.contains(&id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.start(id).await
    }

    async fn process_pid(&self, id: Uuid) -> Option<i32> {
        self.store
            .list_agent_processes()
            .await
            .ok()?
            .into_iter()
            .find(|process| process.agent_definition_id == id)
            .and_then(|process| process.pid)
    }
}

fn agentd_binary() -> PathBuf {
    if let Ok(value) = std::env::var("FOUNDRY_AGENTD_BIN") {
        return PathBuf::from(value);
    }
    let current =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("foundry-control-plane"));
    current
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("foundry-agentd")
}

/// Append the child's process output to its plain-text log file.
fn pipe_to_log<R>(mut reader: R, path: PathBuf)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "cannot open agent log");
                // Keep draining the pipe so the child never blocks on a full
                // buffer when the log file is unavailable.
                let _ = tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
                return;
            }
        };
        // Write every chunk and flush it immediately. The log viewer reads the
        // file from disk, so buffering here would delay live logs.
        let mut buffer = vec![0u8; 8192];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => {
                    if let Err(error) = file.write_all(&buffer[..read]).await {
                        tracing::warn!(path = %path.display(), error = %error, "agent log capture failed");
                        break;
                    }
                    if let Err(error) = file.flush().await {
                        tracing::warn!(path = %path.display(), error = %error, "agent log flush failed");
                        break;
                    }
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), error = %error, "agent log capture failed");
                    break;
                }
            }
        }
    });
}

fn process_alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn terminate(pid: i32) {
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}
