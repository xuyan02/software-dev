//! Separate capsules for services that the test client must reach.
//!
//! A web/desktop/API artifact often has a server component and a client
//! component. The test agent's own capsule is the client; these tools start
//! the server in its own capsule on the same Podman network, so the browser or
//! desktop input tools can reach it by container name.

use async_trait::async_trait;
use foundry_agent_runtime::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use linux_capsule::LinuxCapsule;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ServiceCapsules {
    agent_id: String,
    image: String,
    workspace: PathBuf,
    services: Mutex<HashMap<String, LinuxCapsule>>,
}

impl ServiceCapsules {
    pub fn new(agent_id: String, image: String, workspace: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            agent_id,
            image,
            workspace,
            services: Mutex::new(HashMap::new()),
        })
    }

    async fn start(
        &self,
        project_id: Option<&str>,
        name: &str,
        command: &str,
        port: Option<u16>,
        cwd: Option<&str>,
        image: Option<&str>,
    ) -> Result<Value, String> {
        let handle = service_handle(name)?;
        let mut services = self.services.lock().await;
        if services.contains_key(&handle) {
            return Err(format!("service {handle} already exists"));
        }
        let image = image.unwrap_or(self.image.as_str()).to_string();
        let capsule = LinuxCapsule::headless_with_image_workspace_and_labels(
            image,
            self.workspace.clone(),
            vec![
                ("foundry.agent_id".to_string(), self.agent_id.clone()),
                ("foundry.service".to_string(), handle.clone()),
                (
                    "foundry.project".to_string(),
                    project_id.unwrap_or_default().to_string(),
                ),
            ],
        )
        .await
        .map_err(|error| error.to_string())?;
        let container = capsule.name().to_string();
        let cwd = cwd
            .map(str::to_string)
            .or_else(|| project_id.map(|project| format!("/work/{project}/repo")))
            .unwrap_or_else(|| "/work".to_string());
        let log = format!("/tmp/foundry-services/{handle}.log");
        let script = format!(
            "mkdir -p /tmp/foundry-services; cd {cwd}; nohup sh -c {command} > {log} 2>&1 & pid=$!; echo \"$pid\" > /tmp/foundry-services/{handle}.pid; sleep 0.3; if kill -0 \"$pid\" 2>/dev/null; then echo \"started pid=$pid\"; else echo 'service failed to start'; tail -n 100 {log} 2>/dev/null || true; exit 1; fi",
            cwd = quote(&cwd),
            command = quote(command),
            log = quote(&log),
            handle = handle,
        );
        let output = capsule
            .exec(&script)
            .await
            .map_err(|error| error.to_string())?;
        if !output.success() {
            let _ = capsule.close().await;
            return Err(format!(
                "service {handle} failed to start:\n{}\n{}",
                output.stdout_string(),
                output.stderr_string()
            ));
        }
        services.insert(handle.clone(), capsule);
        Ok(json!({
            "handle": handle,
            "container": container,
            "host": container,
            "port": port,
            "url": port.map(|port| format!("http://{container}:{port}")),
            "log": log,
            "pid_output": output.stdout_string(),
        }))
    }

    async fn status(&self, name: &str) -> Result<String, String> {
        let handle = service_handle(name)?;
        let services = self.services.lock().await;
        let capsule = services
            .get(&handle)
            .ok_or_else(|| format!("unknown service {handle}"))?;
        let output = capsule
            .exec(&format!(
                "if [ -f /tmp/foundry-services/{handle}.pid ]; then pid=$(cat /tmp/foundry-services/{handle}.pid); if kill -0 \"$pid\" 2>/dev/null; then echo \"running pid=$pid\"; else echo \"stopped pid=$pid\"; fi; else echo not_started; fi; echo '--- log ---'; tail -n 80 /tmp/foundry-services/{handle}.log 2>/dev/null || true"
            ))
            .await
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            output.stdout_string(),
            output.stderr_string()
        ))
    }

    async fn logs(&self, name: &str, lines: u64) -> Result<String, String> {
        let handle = service_handle(name)?;
        let services = self.services.lock().await;
        let capsule = services
            .get(&handle)
            .ok_or_else(|| format!("unknown service {handle}"))?;
        let output = capsule
            .exec(&format!(
                "tail -n {} /tmp/foundry-services/{handle}.log 2>/dev/null || echo 'no log yet'",
                lines.clamp(1, 2000)
            ))
            .await
            .map_err(|error| error.to_string())?;
        Ok(output.stdout_string())
    }

    async fn stop(&self, name: &str) -> Result<String, String> {
        let handle = service_handle(name)?;
        let mut services = self.services.lock().await;
        let capsule = services
            .remove(&handle)
            .ok_or_else(|| format!("unknown service {handle}"))?;
        let container = capsule.name().to_string();
        capsule.close().await.map_err(|error| error.to_string())?;
        Ok(format!("stopped service {handle} ({container})"))
    }
}

pub struct ServiceStartTool {
    services: Arc<ServiceCapsules>,
}

impl ServiceStartTool {
    pub fn new(services: Arc<ServiceCapsules>) -> Self {
        Self { services }
    }
}

#[async_trait]
impl Tool for ServiceStartTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "service_start".to_string(),
            description: "Start a server/backend component in its own capsule on the shared network. Returns a hostname and URL the client capsule can reach."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["name", "command"],
                "properties": {
                    "name": { "type": "string", "description": "Service handle, e.g. backend" },
                    "command": { "type": "string", "description": "Command that starts the server; bind 0.0.0.0" },
                    "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
                    "cwd": { "type": "string", "description": "Defaults to /work/<project>/repo" },
                    "image": { "type": "string", "description": "Optional capsule image override" }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let name = required(&arguments, "name")?;
        let command = required(&arguments, "command")?;
        let port = arguments
            .get("port")
            .and_then(Value::as_u64)
            .map(|port| port as u16);
        let cwd = arguments.get("cwd").and_then(Value::as_str);
        let image = arguments.get("image").and_then(Value::as_str);
        let value = self
            .services
            .start(
                context.project_id.as_deref(),
                name,
                command,
                port,
                cwd,
                image,
            )
            .await
            .map_err(ToolError::Failed)?;
        Ok(ToolOutput::success(
            serde_json::to_string_pretty(&value).unwrap_or_default(),
        ))
    }
}

pub struct ServiceStatusTool {
    services: Arc<ServiceCapsules>,
}

impl ServiceStatusTool {
    pub fn new(services: Arc<ServiceCapsules>) -> Self {
        Self { services }
    }
}

#[async_trait]
impl Tool for ServiceStatusTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "service_status".to_string(),
            description: "Check a service capsule's process and log tail.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["name"],
                "properties": { "name": { "type": "string" } }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = required(&arguments, "name")?;
        let text = self
            .services
            .status(name)
            .await
            .map_err(ToolError::Failed)?;
        Ok(ToolOutput::success(text))
    }
}

pub struct ServiceLogsTool {
    services: Arc<ServiceCapsules>,
}

impl ServiceLogsTool {
    pub fn new(services: Arc<ServiceCapsules>) -> Self {
        Self { services }
    }
}

#[async_trait]
impl Tool for ServiceLogsTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "service_logs".to_string(),
            description: "Read a service capsule log tail.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": { "type": "string" },
                    "lines": { "type": "integer", "minimum": 1, "maximum": 2000, "default": 200 }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = required(&arguments, "name")?;
        let lines = arguments
            .get("lines")
            .and_then(Value::as_u64)
            .unwrap_or(200);
        Ok(ToolOutput::success(
            self.services
                .logs(name, lines)
                .await
                .map_err(ToolError::Failed)?,
        ))
    }
}

pub struct ServiceStopTool {
    services: Arc<ServiceCapsules>,
}

impl ServiceStopTool {
    pub fn new(services: Arc<ServiceCapsules>) -> Self {
        Self { services }
    }
}

#[async_trait]
impl Tool for ServiceStopTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "service_stop".to_string(),
            description: "Stop and remove a service capsule.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["name"],
                "properties": { "name": { "type": "string" } }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = required(&arguments, "name")?;
        Ok(ToolOutput::success(
            self.services.stop(name).await.map_err(ToolError::Failed)?,
        ))
    }
}

fn required<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn service_handle(name: &str) -> Result<String, String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!("invalid service name {name:?}"));
    }
    Ok(name.to_string())
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_handles_reject_paths_and_spaces() {
        assert_eq!(service_handle("backend").unwrap(), "backend");
        assert_eq!(service_handle("web-api").unwrap(), "web-api");
        assert!(service_handle("../backend").is_err());
        assert!(service_handle("web api").is_err());
        assert!(service_handle("").is_err());
    }
}
