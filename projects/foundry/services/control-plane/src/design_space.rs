use async_trait::async_trait;
use foundry_app::ports::DesignSpaceProvisioner;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

/// Start a local `ds-server` when `DESIGN_SPACE_URL` is not configured.
///
/// The child is returned so the caller keeps it alive for the process
/// lifetime. The root defaults to `~/.foundry/design-spaces`; override it with
/// `DESIGN_SPACE_ROOT`. `DESIGN_SPACE_HOST` / `DESIGN_SPACE_USER` shape the
/// generated `ds://` addresses.
pub async fn start_local() -> Result<(tokio::process::Child, String, PathBuf), String> {
    let root = match std::env::var("DESIGN_SPACE_ROOT") {
        Ok(root) if !root.trim().is_empty() => PathBuf::from(root),
        _ => {
            let home = std::env::var("HOME")
                .map_err(|_| "HOME is not set; cannot choose a design-space root".to_string())?;
            PathBuf::from(home).join(".foundry").join("design-spaces")
        }
    };
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("create design-space root {}: {error}", root.display()))?;

    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("choose a design-space port: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);

    let host = std::env::var("DESIGN_SPACE_HOST")
        .ok()
        .filter(|host| !host.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    let user = std::env::var("DESIGN_SPACE_USER")
        .ok()
        .filter(|user| !user.trim().is_empty())
        .or_else(|| std::env::var("USER").ok());

    let binary = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("ds-server")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ds-server"));

    let listen = format!("127.0.0.1:{port}");
    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("serve")
        .arg("--root")
        .arg(&root)
        .arg("--listen")
        .arg(&listen)
        .arg("--host")
        .arg(&host);
    if let Some(user) = &user {
        command.arg("--user").arg(user);
    }
    command.kill_on_drop(true);
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        format!(
            "failed to start {}: {error}; build it with `cargo build --bins` or set DESIGN_SPACE_URL",
            binary.display()
        )
    })?;

    let base = format!("http://{listen}");
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("ds-server exited during startup: {status}"));
        }
        // Any HTTP response means the listener is up. Older ds-server builds
        // may not have /health yet; they answer 404 and are still usable.
        if client.get(format!("{base}/health")).send().await.is_ok() {
            return Ok((child, base, root));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = child.kill().await;
    Err(
        "ds-server did not answer within 5s; rebuild it with `cargo build --bins` and retry"
            .to_string(),
    )
}

pub struct HttpDesignSpaceProvisioner {
    base: String,
    client: reqwest::Client,
    token: Option<String>,
}

impl HttpDesignSpaceProvisioner {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token: std::env::var("DS_TOKEN")
                .ok()
                .filter(|token| !token.is_empty()),
        }
    }
}

#[derive(Deserialize)]
struct CreatedSpace {
    design_space: String,
}

#[async_trait]
impl DesignSpaceProvisioner for HttpDesignSpaceProvisioner {
    async fn create_design_space(&self, name: &str, source: &str) -> Result<String, String> {
        let mut request = self
            .client
            .post(format!("{}/v1/projects", self.base))
            .json(&serde_json::json!({ "name": name, "source": source }));
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("design space server: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("design space server {status}: {body}"));
        }
        let created: CreatedSpace = response
            .json()
            .await
            .map_err(|error| format!("design space response: {error}"))?;
        Ok(created.design_space)
    }
}
