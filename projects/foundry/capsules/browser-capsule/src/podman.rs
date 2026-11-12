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
        if let Some(path) = std::env::var_os("BROWSER_CAPSULE_PODMAN") {
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
            "install Podman (run install.sh) or set BROWSER_CAPSULE_PODMAN".into(),
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
                redact(args),
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

    pub async fn ensure_network(&self, name: &str) -> Result<()> {
        let exists = self
            .output_allow_failure(&["network".into(), "exists".into(), name.into()])
            .await?;
        if !exists.status.success() {
            self.output(&["network".into(), "create".into(), name.into()])
                .await?;
        }
        Ok(())
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

    pub async fn network_gateway(&self, name: &str) -> Result<Option<String>> {
        let output = self
            .output_allow_failure(&["network".into(), "inspect".into(), name.into()])
            .await?;
        if !output.status.success() {
            return Ok(None);
        }
        let value: serde_json::Value = match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        Ok(find_gateway(&value))
    }
}

fn redact(args: &[String]) -> String {
    let mut out = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            out.push("***".to_string());
            redact_next = false;
            continue;
        }
        if arg == "-e" || arg == "--env" {
            out.push(arg.clone());
            redact_next = true;
            continue;
        }
        out.push(arg.clone());
    }
    out.join(" ")
}

fn find_gateway(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(gateway)) = map.get("gateway") {
                if gateway.parse::<std::net::Ipv4Addr>().is_ok() {
                    return Some(gateway.clone());
                }
            }
            for value in map.values() {
                if let Some(found) = find_gateway(value) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for value in values {
                if let Some(found) = find_gateway(value) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_gateway() {
        let value = serde_json::json!([{
            "subnets": [{ "subnet": "10.89.0.0/24", "gateway": "10.89.0.1" }]
        }]);
        assert_eq!(find_gateway(&value), Some("10.89.0.1".to_string()));
    }

    #[test]
    fn redacts_env_values() {
        let args = vec![
            "create".to_string(),
            "-e".to_string(),
            "SECRET=value".to_string(),
        ];
        assert_eq!(redact(&args), "create -e ***");
    }
}
