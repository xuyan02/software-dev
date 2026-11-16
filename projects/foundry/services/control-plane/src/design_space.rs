use async_trait::async_trait;
use foundry_app::ports::DesignSpaceProvisioner;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

/// Start the single local `ds-server` when `design_space_url` is not configured.
///
/// The child is returned so the caller keeps it alive for the process
/// lifetime. The root, listen address and token file come from the Foundry
/// config file. The root contains one ds repository per project.
pub async fn start_local(
    root: PathBuf,
    listen: String,
    token_file: PathBuf,
) -> Result<(tokio::process::Child, String, u16, String), String> {
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("create design-space root {}: {error}", root.display()))?;
    let token = ensure_token(&token_file)?;

    let listen = normalize_listen(&listen)?;
    let port = listen_port(&listen)?;

    let binary = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("ds-server")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ds-server"));

    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("--root")
        .arg(&root)
        .arg("--listen")
        .arg(&listen)
        .arg("--token-file")
        .arg(&token_file);
    command.kill_on_drop(true);
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        format!(
            "failed to start {}: {error}; build it with `cargo build --bins` or set design_space_url",
            binary.display()
        )
    })?;

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("ds-server exited during startup: {status}"));
        }
        let request = client
            .get(format!("{base}/repos"))
            .header("Authorization", format!("Bearer {token}"));
        if request.send().await.is_ok() {
            return Ok((child, base, port, token));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = child.kill().await;
    Err(
        "ds-server did not answer within 5s; rebuild it with `cargo build --bins` and retry"
            .to_string(),
    )
}

fn ensure_token(path: &PathBuf) -> Result<String, String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let token = text.trim().to_owned();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::now_v7().simple(),
        uuid::Uuid::now_v7().simple()
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create token directory {}: {error}", parent.display()))?;
    }
    std::fs::write(path, format!("{token}\n"))
        .map_err(|error| format!("write design-space token {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("chmod design-space token: {error}"))?;
    }
    Ok(token)
}

fn normalize_listen(listen: &str) -> Result<String, String> {
    let listen = listen.trim();
    let addr: std::net::SocketAddr = listen
        .parse()
        .map_err(|_| format!("invalid design_space_listen: {listen}"))?;
    if addr.port() == 0 {
        let listener = std::net::TcpListener::bind(addr)
            .map_err(|error| format!("bind design_space_listen {listen}: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        return Ok(format!("{}:{port}", addr.ip()));
    }
    Ok(addr.to_string())
}

fn listen_port(listen: &str) -> Result<u16, String> {
    listen
        .parse::<std::net::SocketAddr>()
        .map(|addr| addr.port())
        .map_err(|error| error.to_string())
}

pub fn host_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            Ok(socket.local_addr()?.ip().to_string())
        })
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn read_token(path: &PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

pub struct HttpDesignSpaceProvisioner {
    base: String,
    agent_base: String,
    client: reqwest::Client,
    token: Option<String>,
}

impl HttpDesignSpaceProvisioner {
    pub fn new(
        base: impl Into<String>,
        agent_base: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            agent_base: agent_base.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token,
        }
    }
}

#[derive(Deserialize)]
struct CreatedRepo {
    name: String,
}

#[async_trait]
impl DesignSpaceProvisioner for HttpDesignSpaceProvisioner {
    async fn create_design_space(&self, repo: &str, branch: &str) -> Result<String, String> {
        let mut request = self
            .client
            .post(format!("{}/repos", self.base))
            .json(&serde_json::json!({ "name": repo, "branch": branch }));
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
        let created: CreatedRepo = response
            .json()
            .await
            .map_err(|error| format!("design space response: {error}"))?;
        Ok(format!("{}/{}", self.agent_base, created.name))
    }
}
