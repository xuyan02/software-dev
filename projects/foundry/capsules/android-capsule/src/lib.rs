//! A small, opinionated Android capsule.
//!
//! The Android emulator runs inside a rootless Podman container with
//! `/dev/kvm` passed through. The host controls it with `adb`: screenshots,
//! touch/key input, APK install and app launch.
//!
//! It is not a hard security boundary: containers share the host kernel. Use a
//! microVM if the workload is adversarial.

mod adb;
mod podman;

use std::path::Path;
use std::time::Duration;

const DEFAULT_IMAGE: &str = "localhost/android-capsule:1";
const ADB_PORT: u16 = 5555;
const BOOT_TIMEOUT: Duration = Duration::from_secs(300);

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
    Tap {
        x: i32,
        y: i32,
    },
    Swipe {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        duration_ms: u32,
    },
    Text {
        text: String,
    },
    Key {
        key: String,
    },
}

pub struct AndroidCapsule {
    name: String,
    serial: String,
    podman: podman::Podman,
    closed: bool,
}

impl AndroidCapsule {
    /// Start the emulator and wait until Android reports boot completed.
    pub async fn create() -> Result<Self> {
        let podman = podman::Podman::discover()?;
        podman.ensure_image(DEFAULT_IMAGE).await?;

        let id = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("android-capsule-{}", &id[..12]);
        let port = free_port()?;
        let args = build_create_args(&name, port);

        podman.create(&args).await?;
        if let Err(error) = podman.start(&name).await {
            let _ = podman.force_remove(&name).await;
            return Err(error);
        }

        let serial = format!("127.0.0.1:{port}");
        if let Err(error) = adb::connect(&serial).await {
            let logs = podman.logs(&name).await.unwrap_or_default();
            let _ = podman.force_remove(&name).await;
            return Err(Error::NotReady(format!(
                "{error}\n--- container logs ---\n{logs}"
            )));
        }
        if let Err(error) = adb::wait_for_boot(&serial, BOOT_TIMEOUT).await {
            let logs = podman.logs(&name).await.unwrap_or_default();
            let _ = adb::disconnect(&serial).await;
            let _ = podman.force_remove(&name).await;
            return Err(Error::NotReady(format!(
                "{error}\n--- container logs ---\n{logs}"
            )));
        }

        Ok(Self {
            name,
            serial,
            podman,
            closed: false,
        })
    }

    /// PNG screenshot of the current Android screen.
    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        adb::screenshot(&self.serial).await
    }

    pub async fn input(&self, input: Input) -> Result<()> {
        match input {
            Input::Tap { x, y } => adb::tap(&self.serial, x, y).await,
            Input::Swipe {
                x1,
                y1,
                x2,
                y2,
                duration_ms,
            } => adb::swipe(&self.serial, x1, y1, x2, y2, duration_ms).await,
            Input::Text { text } => adb::text(&self.serial, &text).await,
            Input::Key { key } => adb::key(&self.serial, &key).await,
        }
    }

    /// Install an APK from the host (`adb install -r`).
    pub async fn install_apk(&self, local: &Path) -> Result<()> {
        adb::install_apk(&self.serial, local).await
    }

    /// Launch the default launcher activity of a package.
    pub async fn launch(&self, package: &str) -> Result<()> {
        adb::launch(&self.serial, package).await
    }

    /// Stop the emulator and remove the container.
    pub async fn close(mut self) -> Result<()> {
        let _ = adb::emu_kill(&self.serial).await;
        let _ = adb::disconnect(&self.serial).await;
        self.podman.stop(&self.name).await?;
        self.podman.force_remove(&self.name).await?;
        self.closed = true;
        Ok(())
    }
}

impl Drop for AndroidCapsule {
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

fn build_create_args(name: &str, host_port: u16) -> Vec<String> {
    vec![
        "create".to_string(),
        "--name".to_string(),
        name.to_string(),
        "--label".to_string(),
        "android-capsule.managed=true".to_string(),
        "--device".to_string(),
        "/dev/kvm".to_string(),
        "--shm-size".to_string(),
        "2g".to_string(),
        "--tmpfs".to_string(),
        "/tmp:rw,size=2g".to_string(),
        "--tmpfs".to_string(),
        "/run:rw,size=64m".to_string(),
        "--memory".to_string(),
        "4096m".to_string(),
        "--cpus".to_string(),
        "2".to_string(),
        "--pids-limit".to_string(),
        "1024".to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "-p".to_string(),
        format!("127.0.0.1:{host_port}:{ADB_PORT}"),
        DEFAULT_IMAGE.to_string(),
    ]
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
    fn build_args_pass_kvm_and_adb_port() {
        let args = build_create_args("android-capsule-test", 1234);
        let joined = args.join(" ");
        assert!(joined.contains("--device /dev/kvm"));
        assert!(joined.contains("--shm-size 2g"));
        assert!(joined.contains("127.0.0.1:1234:5555"));
        assert!(joined.contains("--cap-drop ALL"));
        assert_eq!(args.last().unwrap(), DEFAULT_IMAGE);
    }
}
