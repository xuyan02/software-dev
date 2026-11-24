use async_trait::async_trait;
use foundry_app::ports::DesignSpaceProvisioner;
use std::path::PathBuf;

pub fn host_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            Ok(socket.local_addr()?.ip().to_string())
        })
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Provision new projects as Arche repositories.
///
/// Arche stores one repository per project under the collection root the
/// console reads.
pub struct ArcheDesignSpaceProvisioner {
    root: PathBuf,
    ssh_base: String,
}

impl ArcheDesignSpaceProvisioner {
    pub fn new(root: PathBuf, ssh_base: impl Into<String>) -> Self {
        Self {
            root,
            ssh_base: ssh_base.into().trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
impl DesignSpaceProvisioner for ArcheDesignSpaceProvisioner {
    async fn create_design_space(
        &self,
        repo: &str,
        branch: &str,
        source_url: &str,
    ) -> Result<String, String> {
        if repo.is_empty() || repo.contains('/') || repo.contains("..") {
            return Err(format!("invalid Arche repository name: {repo}"));
        }
        let root = self.root.clone();
        let name = repo.to_owned();
        let branch = branch.to_owned();
        let source_url = source_url.to_owned();
        let url = format!("{}/{}", self.ssh_base, repo);
        tokio::task::spawn_blocking(move || {
            let path = root.join(&name);
            if path.join(".arche").is_dir() {
                return Ok(url);
            }
            if path.exists() {
                return Err(format!(
                    "{} already exists and is not an Arche repository",
                    path.display()
                ));
            }
            std::fs::create_dir_all(&root)
                .map_err(|error| format!("create {}: {error}", root.display()))?;
            let repo = arche_core::Repository::init_with_source(&path, Some(&source_url))
                .map_err(|error| format!("create Arche repository {}: {error}", path.display()))?;
            let current = repo
                .current_branch()
                .map_err(|error| format!("read Arche branch {}: {error}", path.display()))?;
            let exists = repo
                .branch_list()
                .map_err(|error| format!("list Arche branches {}: {error}", path.display()))?
                .iter()
                .any(|candidate| candidate == &branch);
            if !branch.trim().is_empty() && branch != current && !exists {
                repo.branch_create(&branch)
                    .map_err(|error| format!("create Arche branch {branch}: {error}"))?;
            }
            Ok(url)
        })
        .await
        .map_err(|error| error.to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provisions_an_arche_repository() {
        let temp = tempfile::TempDir::new().unwrap();
        let provisioner = ArcheDesignSpaceProvisioner::new(
            temp.path().to_path_buf(),
            "ssh://foundry-arche@example.test",
        );
        let id = "01900000-0000-7000-8000-000000000001";
        let url = provisioner
            .create_design_space(id, "source", "https://example.test/repo.git")
            .await
            .unwrap();
        assert_eq!(url, format!("ssh://foundry-arche@example.test/{id}"));

        let path = temp.path().join(id);
        assert!(path.join(".arche").is_dir());
        let repo = arche_core::Repository::open(&path).unwrap();
        assert!(repo.branch_list().unwrap().contains(&"source".to_owned()));

        // Provisioning the same project again is idempotent.
        let again = provisioner
            .create_design_space(id, "source", "https://example.test/repo.git")
            .await
            .unwrap();
        assert_eq!(again, url);
    }
}
