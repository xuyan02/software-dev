//! A small, opinionated browser capsule.
//!
//! Chromium runs inside a rootless Podman container with remote debugging
//! enabled. The host library talks to it over the Chrome DevTools Protocol for
//! navigation, screenshots and input.
//!
//! It is not a hard security boundary: containers share the host kernel. Use a
//! microVM if the workload is adversarial.

mod cdp;
mod podman;

use std::time::{Duration, Instant};

const DEFAULT_IMAGE: &str = "localhost/browser-capsule:1";
const DEFAULT_NETWORK: &str = "browser-capsule";
const VIEWPORT: &str = "1280x800";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const PAGE_LOAD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("capsule unavailable: {0}")]
    Unavailable(String),

    #[error("capsule did not become ready: {0}")]
    NotReady(String),

    #[error("backend error: {0}")]
    Backend(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub enum Input {
    Move { x: i32, y: i32 },
    Click { x: i32, y: i32, button: u8 },
    Scroll { x: i32, y: i32, delta: i32 },
    Key { key: String },
    Text { text: String },
}

pub struct BrowserCapsule {
    name: String,
    podman: podman::Podman,
    cdp: cdp::Cdp,
    closed: bool,
}

impl BrowserCapsule {
    /// Start Chromium in a fresh container. It begins at `about:blank`.
    pub async fn create() -> Result<Self> {
        let podman = podman::Podman::discover()?;
        podman.ensure_network(DEFAULT_NETWORK).await?;
        podman.ensure_image(DEFAULT_IMAGE).await?;

        let gateway = podman.network_gateway(DEFAULT_NETWORK).await.ok().flatten();
        let id = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("browser-capsule-{}", &id[..12]);
        let port = free_port()?;
        let args = build_create_args(&name, port, gateway.as_deref());

        podman.create(&args).await?;
        if let Err(error) = podman.start(&name).await {
            let _ = podman.force_remove(&name).await;
            return Err(error);
        }

        let ready = wait_for_cdp(port, STARTUP_TIMEOUT).await;
        if let Err(error) = ready {
            let logs = podman.logs(&name).await.unwrap_or_default();
            let _ = podman.force_remove(&name).await;
            return Err(Error::NotReady(format!(
                "{error}\n--- container logs ---\n{logs}"
            )));
        }

        let cdp = match cdp::Cdp::connect(port).await {
            Ok(cdp) => cdp,
            Err(error) => {
                let logs = podman.logs(&name).await.unwrap_or_default();
                let _ = podman.force_remove(&name).await;
                return Err(Error::NotReady(format!(
                    "{error}\n--- container logs ---\n{logs}"
                )));
            }
        };

        if let Err(error) = cdp.send("Page.enable", serde_json::json!({})).await {
            let _ = podman.force_remove(&name).await;
            return Err(error);
        }

        Ok(Self {
            name,
            podman,
            cdp,
            closed: false,
        })
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.cdp.navigate(url).await
    }

    /// PNG screenshot of the current viewport.
    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        self.cdp.screenshot().await
    }

    pub async fn input(&self, input: Input) -> Result<()> {
        self.cdp.input(input).await
    }

    /// Stop and remove the browser container.
    pub async fn close(mut self) -> Result<()> {
        self.podman.stop(&self.name).await?;
        self.podman.force_remove(&self.name).await?;
        self.closed = true;
        Ok(())
    }
}

impl Drop for BrowserCapsule {
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
    }
}

fn build_create_args(name: &str, host_port: u16, gateway: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "create".to_string(),
        "--name".to_string(),
        name.to_string(),
        "--label".to_string(),
        "browser-capsule.managed=true".to_string(),
        "--network".to_string(),
        DEFAULT_NETWORK.to_string(),
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
        "--shm-size".to_string(),
        "1g".to_string(),
        "--tmpfs".to_string(),
        "/tmp:rw,size=256m".to_string(),
        "--tmpfs".to_string(),
        "/run:rw,size=64m".to_string(),
        "-e".to_string(),
        format!("BROWSER_CAPSULE_VIEWPORT={VIEWPORT}"),
        "-p".to_string(),
        format!("127.0.0.1:{host_port}:9222"),
        "--add-host".to_string(),
        format!(
            "host.containers.internal:{}",
            gateway.unwrap_or("host-gateway")
        ),
        DEFAULT_IMAGE.to_string(),
    ];
    args.shrink_to_fit();
    args
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn wait_for_cdp(port: u16, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let url = format!("http://127.0.0.1:{port}/json/version");
    loop {
        match reqwest::get(&url).await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => {
                if Instant::now() >= deadline {
                    return Err(Error::NotReady(format!(
                        "CDP returned {}",
                        response.status()
                    )));
                }
            }
            Err(_) => {
                if Instant::now() >= deadline {
                    return Err(Error::NotReady("CDP endpoint did not become ready".into()));
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_have_isolation_and_cdp_port() {
        let args = build_create_args("browser-capsule-test", 1234, Some("10.89.0.1"));
        let joined = args.join(" ");
        assert!(joined.contains("--cap-drop ALL"));
        assert!(joined.contains("--security-opt no-new-privileges"));
        assert!(joined.contains("--shm-size 1g"));
        assert!(joined.contains("127.0.0.1:1234:9222"));
        assert!(joined.contains("host.containers.internal:10.89.0.1"));
        assert_eq!(args.last().unwrap(), DEFAULT_IMAGE);
    }
}
