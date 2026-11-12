use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone)]
pub struct Podman {
    program: PathBuf,
}

impl Podman {
    pub fn discover() -> Result<Self> {
        if let Some(path) = std::env::var_os("ANDROID_CAPSULE_PODMAN") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(Self { program: path });
            }
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let candidate = directory.join("podman");
                if candidate.is_file() {
                    return Ok(Self { program: candidate });
                }
            }
        }
        for candidate in ["/usr/bin/podman", "/usr/local/bin/podman", "/bin/podman"] {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return Ok(Self { program: path });
            }
        }
        Err(Error::Unavailable(
            "install Podman (run install.sh) or set ANDROID_CAPSULE_PODMAN".into(),
        ))
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    async fn output(&self, args: &[String]) -> Result<std::process::Output> {
        let output = self.output_allow_failure(args).await?;
        if !output.status.success() {
            return Err(Error::Backend(format!(
                "{}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(output)
    }

    async fn output_allow_failure(&self, args: &[String]) -> Result<std::process::Output> {
        Command::new(&self.program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|error| Error::Backend(format!("failed to run podman: {error}")))
    }

    pub async fn create(&self, args: &[String]) -> Result<()> {
        self.output(args).await?;
        Ok(())
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        self.output(&["start".into(), name.into()]).await?;
        Ok(())
    }

    pub async fn stop(&self, name: &str) -> Result<()> {
        let _ = self
            .output_allow_failure(&["stop".into(), "-t".into(), "2".into(), name.into()])
            .await?;
        Ok(())
    }

    pub async fn force_remove(&self, name: &str) -> Result<()> {
        let _ = self
            .output_allow_failure(&["rm".into(), "-f".into(), name.into()])
            .await?;
        Ok(())
    }

    pub async fn logs(&self, name: &str) -> Result<String> {
        let output = self
            .output_allow_failure(&["logs".into(), name.into()])
            .await?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            if !text.ends_with('\n') && !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&stderr);
        }
        Ok(text)
    }

    pub async fn ensure_image(&self, image: &str) -> Result<()> {
        let exists = self
            .output_allow_failure(&["image".into(), "exists".into(), image.into()])
            .await?;
        if !exists.status.success() {
            return Err(Error::Unavailable(format!(
                "image {image} is missing; run install.sh or `podman pull {image}`"
            )));
        }
        Ok(())
    }
}
