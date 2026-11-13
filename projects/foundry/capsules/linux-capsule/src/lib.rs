//! A small, opinionated library for running a Linux application inside an
//! isolated, rootless Podman capsule.
//!
//! Desktop mode supplies Xvfb, a lightweight window manager and the
//! screenshot/input sidecar. Headless mode skips X entirely. Both modes expose
//! only the minimal surface: create, screenshot/input, exec, upload/download and
//! stop.
//!
//! It is not a hard security boundary: containers share the host kernel. Use a
//! microVM if the workload is adversarial.

mod podman;
mod sidecar_client;

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Default base image built by `install.sh`.
const DEFAULT_IMAGE: &str = "localhost/linux-capsule:1";
/// Default Podman network created by `install.sh`.
const DEFAULT_NETWORK: &str = "linux-capsule";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("capsule unavailable: {0}")]
    Unavailable(String),

    #[error("capsule did not become ready: {0}")]
    NotReady(String),

    #[error("this capsule is headless and has no desktop UI")]
    NoDesktop,

    #[error("backend error: {0}")]
    Backend(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy)]
struct Screen {
    pub width: u16,
    pub height: u16,
    pub depth: u8,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            depth: 24,
        }
    }
}

impl Screen {
    fn xvfb_spec(&self) -> String {
        format!("{}x{}x{}", self.width, self.height, self.depth)
    }
}

#[derive(Debug, Clone)]
struct NetworkSpec {
    pub name: String,
    /// Add `host.containers.internal` pointing at the bridge gateway.
    pub host_access: bool,
}

impl Default for NetworkSpec {
    fn default() -> Self {
        Self {
            name: DEFAULT_NETWORK.to_string(),
            host_access: true,
        }
    }
}

#[derive(Debug, Clone)]
struct PortSpec {
    /// Host port, bound on 127.0.0.1.
    pub host: u16,
    /// Container port.
    pub container: u16,
}

/// A file written into the session directory and mounted read-only at
/// `/config` inside the container.  `path` is relative to that mount.
#[derive(Debug, Clone)]
struct FileSpec {
    pub path: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum LinuxCapsuleMode {
    /// Xvfb + openbox + sidecar.  Screenshot and input are available.
    #[default]
    Desktop,
    /// No X server and no sidecar.  Only exec/copy/logs are available.
    Headless,
}

impl LinuxCapsuleMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Headless => "headless",
        }
    }

    fn has_desktop(self) -> bool {
        matches!(self, Self::Desktop)
    }
}

#[derive(Debug, Clone)]
struct LinuxCapsuleSpec {
    pub mode: LinuxCapsuleMode,
    pub image: String,
    pub app: Vec<String>,
    pub env: Vec<(String, String)>,
    pub files: Vec<FileSpec>,
    pub screen: Screen,
    pub network: NetworkSpec,
    /// Additional 127.0.0.1 host ports published to the container.
    pub ports: Vec<PortSpec>,
    /// Optional shell script run inside the container before the app starts.
    /// It may install packages, create users or edit files in the writable layer.
    pub setup: Option<String>,
    pub startup_timeout: Duration,
    /// Extra time to let the application map its first window after the
    /// sidecar and setup are ready.  Defaults to 250 ms; set to zero if the
    /// caller prefers to poll the screen itself.
    pub settle: Duration,
}

impl LinuxCapsuleSpec {
    fn validate(&self) -> Result<()> {
        if self.image.trim().is_empty() {
            return Err(Error::InvalidRequest("image must not be empty".into()));
        }
        if self.screen.width == 0 || self.screen.height == 0 {
            return Err(Error::InvalidRequest(
                "screen dimensions must be > 0".into(),
            ));
        }
        if !matches!(self.screen.depth, 16 | 24 | 32) {
            return Err(Error::InvalidRequest(
                "screen depth must be 16, 24 or 32".into(),
            ));
        }
        if self.network.name.trim().is_empty() {
            return Err(Error::InvalidRequest(
                "network name must not be empty".into(),
            ));
        }
        if self.startup_timeout.is_zero() {
            return Err(Error::InvalidRequest("startup_timeout must be > 0".into()));
        }
        for port in &self.ports {
            if port.host == 0 || port.container == 0 {
                return Err(Error::InvalidRequest(
                    "port numbers must be non-zero".into(),
                ));
            }
        }
        for file in &self.files {
            validate_relative(&file.path)?;
        }
        Ok(())
    }
}

impl Default for LinuxCapsuleSpec {
    fn default() -> Self {
        Self {
            mode: LinuxCapsuleMode::default(),
            image: DEFAULT_IMAGE.to_string(),
            app: Vec::new(),
            env: Vec::new(),
            files: Vec::new(),
            screen: Screen::default(),
            network: NetworkSpec::default(),
            ports: Vec::new(),
            setup: None,
            startup_timeout: Duration::from_secs(30),
            settle: Duration::from_millis(250),
        }
    }
}

fn validate_relative(path: &str) -> Result<()> {
    if path.trim().is_empty() {
        return Err(Error::InvalidRequest("file path must not be empty".into()));
    }
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::InvalidRequest(format!(
            "file path must stay under /config: {}",
            path.display()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Input {
    Move {
        x: i16,
        y: i16,
    },
    Click {
        x: i16,
        y: i16,
        #[serde(default = "default_button")]
        button: u8,
    },
    Scroll {
        x: i16,
        y: i16,
        delta: i32,
    },
    Key {
        key: String,
    },
    Text {
        text: String,
    },
}

fn default_button() -> u8 {
    1
}

#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecOutput {
    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_string(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
}

pub struct LinuxCapsule {
    name: String,
    podman: podman::Podman,
    sidecar: Option<sidecar_client::SidecarClient>,
    session_dir: PathBuf,
    closed: bool,
}

impl LinuxCapsule {
    /// Desktop capsule with a terminal window (`xterm`) already running.
    pub async fn desktop() -> Result<Self> {
        Self::create(LinuxCapsuleSpec {
            mode: LinuxCapsuleMode::Desktop,
            app: vec!["xterm".to_string()],
            ..LinuxCapsuleSpec::default()
        })
        .await
    }

    /// Headless capsule kept alive with `sleep infinity`.
    pub async fn headless() -> Result<Self> {
        Self::create(LinuxCapsuleSpec {
            mode: LinuxCapsuleMode::Headless,
            app: Vec::new(),
            ..LinuxCapsuleSpec::default()
        })
        .await
    }

    /// Create a capsule and wait until it is ready.
    ///
    /// This single call:
    /// - ensures the Podman network exists;
    /// - verifies the base image is present;
    /// - writes `spec.files` into a private session directory and mounts it at
    ///   `/config` (read-only);
    /// - creates and starts the container with isolation defaults;
    /// - Desktop: waits for the sidecar to answer `/health`;
    /// - Headless: waits for the container state to become `running`;
    /// - both: waits for the setup sentinel when `spec.setup` is set.
    ///
    /// On failure every partial resource is removed before the error returns.
    async fn create(spec: LinuxCapsuleSpec) -> Result<Self> {
        spec.validate()?;
        let podman = podman::Podman::discover()?;
        podman.ensure_network(&spec.network.name).await?;
        podman.ensure_image(&spec.image).await?;

        let gateway = if spec.network.host_access {
            podman
                .network_gateway(&spec.network.name)
                .await
                .ok()
                .flatten()
        } else {
            None
        };

        let id = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("linux-capsule-{}", &id[..12]);
        let token = uuid::Uuid::new_v4().simple().to_string();
        let session_dir = std::env::temp_dir().join(format!("linux-capsule-{id}"));
        tokio::fs::create_dir_all(&session_dir).await?;

        for file in &spec.files {
            let destination = session_dir.join(&file.path);
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&destination, &file.content).await?;
        }

        let host_port = if spec.mode.has_desktop() {
            Some(free_port()?)
        } else {
            None
        };
        let args = build_create_args(
            &spec,
            &name,
            &token,
            host_port,
            &session_dir,
            &id,
            gateway.as_deref(),
        );

        if let Err(error) = podman.create(&args).await {
            let _ = tokio::fs::remove_dir_all(&session_dir).await;
            return Err(error);
        }
        if let Err(error) = podman.start(&name).await {
            let _ = podman.force_remove(&name).await;
            let _ = tokio::fs::remove_dir_all(&session_dir).await;
            return Err(error);
        }

        let sidecar = if spec.mode.has_desktop() {
            let client = sidecar_client::SidecarClient::new(
                host_port.expect("desktop capsule has a sidecar port"),
                token,
            );
            if let Err(error) = client.wait_ready(spec.startup_timeout).await {
                let logs = podman.logs(&name).await.unwrap_or_default();
                let _ = podman.force_remove(&name).await;
                let _ = tokio::fs::remove_dir_all(&session_dir).await;
                return Err(Error::NotReady(format!(
                    "{error}\n--- container logs ---\n{logs}"
                )));
            }
            Some(client)
        } else {
            if let Err(error) = wait_running(&podman, &name, spec.startup_timeout).await {
                let logs = podman.logs(&name).await.unwrap_or_default();
                let _ = podman.force_remove(&name).await;
                let _ = tokio::fs::remove_dir_all(&session_dir).await;
                return Err(Error::NotReady(format!(
                    "{error}\n--- container logs ---\n{logs}"
                )));
            }
            None
        };

        // The entrypoint starts the sidecar before the app command, so the
        // sidecar can be healthy while a setup script is still running.  The
        // setup wrapper writes a sentinel file when it is done; wait for it so
        // that create() returns a configured, ready capsule.
        if spec.setup.is_some() {
            let deadline = Instant::now() + spec.startup_timeout;
            loop {
                let done = podman
                    .exec(&name, &["test", "-f", "/run/linux-capsule-setup-done"])
                    .await;
                if matches!(done, Ok(ref output) if output.success()) {
                    break;
                }
                if Instant::now() >= deadline {
                    let logs = podman.logs(&name).await.unwrap_or_default();
                    let _ = podman.force_remove(&name).await;
                    let _ = tokio::fs::remove_dir_all(&session_dir).await;
                    return Err(Error::NotReady(format!(
                        "setup did not finish within {:?}\n--- container logs ---\n{logs}",
                        spec.startup_timeout
                    )));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        if !spec.settle.is_zero() {
            tokio::time::sleep(spec.settle).await;
        }

        Ok(Self {
            name,
            podman,
            sidecar,
            session_dir,
            closed: false,
        })
    }

    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        self.sidecar
            .as_ref()
            .ok_or(Error::NoDesktop)?
            .screenshot()
            .await
    }

    /// Send one mouse or keyboard event. This is the only input method;
    /// the [`Input`] variants cover move, click, scroll, key and text.
    pub async fn input(&self, input: Input) -> Result<()> {
        self.sidecar
            .as_ref()
            .ok_or(Error::NoDesktop)?
            .input(&input)
            .await
    }

    /// Run a shell script inside the capsule with `sh -c`.
    pub async fn exec(&self, script: &str) -> Result<ExecOutput> {
        self.podman.exec(&self.name, &["sh", "-c", script]).await
    }

    pub async fn upload(&self, local: &Path, container_path: &str) -> Result<()> {
        let source = local
            .to_str()
            .ok_or_else(|| Error::InvalidRequest("local path is not UTF-8".into()))?;
        let destination = format!("{}:{container_path}", self.name);
        self.podman.copy(source, &destination).await
    }

    pub async fn download(&self, container_path: &str, local: &Path) -> Result<()> {
        let destination = local
            .to_str()
            .ok_or_else(|| Error::InvalidRequest("local path is not UTF-8".into()))?;
        let source = format!("{}:{container_path}", self.name);
        self.podman.copy(&source, destination).await
    }

    /// Close the capsule: stop and remove the container, then delete the
    /// session directory.
    pub async fn close(mut self) -> Result<()> {
        self.podman.stop(&self.name).await?;
        self.podman.force_remove(&self.name).await?;
        let _ = tokio::fs::remove_dir_all(&self.session_dir).await;
        self.closed = true;
        Ok(())
    }
}

impl Drop for LinuxCapsule {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let name = self.name.clone();
        let program = self.podman.program().to_string_lossy().into_owned();
        std::thread::spawn(move || {
            let _ = std::process::Command::new(program)
                .args(["rm", "-f", &name])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        });
        let _ = std::fs::remove_dir_all(&self.session_dir);
    }
}

fn build_create_args(
    spec: &LinuxCapsuleSpec,
    name: &str,
    token: &str,
    host_port: Option<u16>,
    session_dir: &Path,
    id: &str,
    gateway: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "create".to_string(),
        "--name".to_string(),
        name.to_string(),
        "--label".to_string(),
        "linux-capsule.managed=true".to_string(),
        "--label".to_string(),
        format!("linux-capsule.session={id}"),
        "--network".to_string(),
        spec.network.name.clone(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "--pids-limit".to_string(),
        "512".to_string(),
        "--memory".to_string(),
        "2048m".to_string(),
        "--cpus".to_string(),
        "2".to_string(),
        "--tmpfs".to_string(),
        "/tmp:rw,size=64m".to_string(),
        "--tmpfs".to_string(),
        "/run:rw,size=64m".to_string(),
        "-e".to_string(),
        format!("LINUX_CAPSULE_MODE={}", spec.mode.as_str()),
    ];

    if spec.mode.has_desktop() {
        args.push("-e".to_string());
        args.push("DISPLAY=:99".to_string());
        args.push("-e".to_string());
        args.push(format!("LINUX_CAPSULE_SCREEN={}", spec.screen.xvfb_spec()));
        args.push("-e".to_string());
        args.push(format!("LINUX_CAPSULE_TOKEN={token}"));
        if let Some(port) = host_port {
            args.push("-p".to_string());
            args.push(format!("127.0.0.1:{port}:8099"));
        }
    }

    if spec.network.host_access {
        args.push("--add-host".to_string());
        args.push(format!(
            "host.containers.internal:{}",
            gateway.unwrap_or("host-gateway")
        ));
    }
    for (key, value) in &spec.env {
        args.push("-e".to_string());
        args.push(format!("{key}={value}"));
    }
    for port in &spec.ports {
        args.push("-p".to_string());
        args.push(format!("127.0.0.1:{}:{}", port.host, port.container));
    }
    if !spec.files.is_empty() {
        args.push("-v".to_string());
        args.push(format!("{}:/config:ro", session_dir.display()));
    }

    let app: Vec<String> = if spec.app.is_empty() {
        vec!["sleep".to_string(), "infinity".to_string()]
    } else {
        spec.app.clone()
    };

    args.push(spec.image.clone());
    if let Some(setup) = &spec.setup {
        args.push("sh".to_string());
        args.push("-c".to_string());
        args.push(format!(
            "set -e\n{setup}\ntouch /run/linux-capsule-setup-done\nexec \"$@\""
        ));
        args.push("linux-capsule-app".to_string());
        args.extend(app);
    } else {
        args.extend(app);
    }
    args
}

async fn wait_running(podman: &podman::Podman, name: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let last = match podman.status(name).await {
            Ok(state) if state == "running" => return Ok(()),
            Ok(state) if state == "exited" || state == "dead" => {
                return Err(Error::NotReady(format!(
                    "capsule exited during startup (state {state})"
                )));
            }
            Ok(state) => state,
            Err(error) => error.to_string(),
        };
        if Instant::now() >= deadline {
            return Err(Error::NotReady(format!(
                "capsule did not reach running within {timeout:?} (last state: {last})"
            )));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_escape_config() {
        assert!(validate_relative("../etc/passwd").is_err());
        assert!(validate_relative("/etc/passwd").is_err());
        assert!(validate_relative("").is_err());
    }

    #[test]
    fn accepts_nested_relative_paths() {
        assert!(validate_relative("conf/app.json").is_ok());
        assert!(validate_relative("a/b/c").is_ok());
    }

    #[test]
    fn empty_app_is_allowed() {
        assert!(LinuxCapsuleSpec::default().validate().is_ok());
    }

    #[test]
    fn create_args_have_isolation_and_network_defaults() {
        let spec = LinuxCapsuleSpec {
            app: vec!["xterm".into()],
            ..LinuxCapsuleSpec::default()
        };
        let args = build_create_args(
            &spec,
            "linux-capsule-test",
            "token",
            Some(1234),
            Path::new("/tmp/session"),
            "id",
            Some("10.89.0.1"),
        );
        let joined = args.join(" ");
        assert!(joined.contains("LINUX_CAPSULE_MODE=desktop"));
        assert!(joined.contains("--cap-drop ALL"));
        assert!(joined.contains("--security-opt no-new-privileges"));
        assert!(joined.contains("--pids-limit 512"));
        assert!(joined.contains("--memory 2048m"));
        assert!(joined.contains("--cpus 2"));
        assert!(joined.contains("127.0.0.1:1234:8099"));
        assert!(joined.contains("host.containers.internal:10.89.0.1"));
        assert!(joined.contains("LINUX_CAPSULE_TOKEN=token"));
        assert_eq!(args.last().unwrap(), "xterm");
    }

    #[test]
    fn headless_args_have_no_display_or_sidecar_port() {
        let spec = LinuxCapsuleSpec {
            mode: LinuxCapsuleMode::Headless,
            ..LinuxCapsuleSpec::default()
        };
        let args = build_create_args(
            &spec,
            "linux-capsule-test",
            "token",
            None,
            Path::new("/tmp/session"),
            "id",
            None,
        );
        let joined = args.join(" ");
        assert!(joined.contains("LINUX_CAPSULE_MODE=headless"));
        assert!(!joined.contains("DISPLAY="));
        assert!(!joined.contains("8099"));
        assert!(!joined.contains("LINUX_CAPSULE_TOKEN="));
        assert_eq!(args.last().unwrap(), "infinity");
    }

    #[test]
    fn setup_wraps_the_app() {
        let spec = LinuxCapsuleSpec {
            app: vec!["xterm".into()],
            setup: Some("echo setup".into()),
            ..LinuxCapsuleSpec::default()
        };
        let args = build_create_args(
            &spec,
            "name",
            "token",
            Some(1234),
            Path::new("/tmp/session"),
            "id",
            None,
        );
        let joined = args.join(" ");
        assert!(joined.contains("exec \"$@\""));
        assert!(joined.contains("linux-capsule-app"));
        assert_eq!(args.last().unwrap(), "xterm");
    }
}
