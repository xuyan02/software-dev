//! Design-space gateway for a remote ds-server.
//!
//! The server exposes immutable files and refs over HTTP. The gateway mirrors
//! the immutable file set into a local cache so the design-space crate can
//! load commits and catalogs without executing anything.

use async_trait::async_trait;
use design_space::catalog::Catalog;
use design_space::model::{CommitHash, DocComment, Hash32, Kind, ObjectPayload};
use design_space::store::Store;
use foundry_app::{DesignSpaceGateway, DesignSpaceGatewayError};
use foundry_domain::{MergeRequestChange, MergeRequestChangeStatus, Project};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct RemoteDesignSpaceGateway {
    base: String,
    token: Option<String>,
    cache_root: PathBuf,
    client: reqwest::Client,
    sync: Arc<Mutex<()>>,
}

impl RemoteDesignSpaceGateway {
    pub fn new(
        base: impl Into<String>,
        token: Option<String>,
        cache_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            token,
            cache_root: cache_root.into(),
            client: reqwest::Client::new(),
            sync: Arc::new(Mutex::new(())),
        }
    }

    fn repo_name(project: &Project) -> Result<String, DesignSpaceGatewayError> {
        let value = project.design_space_url.trim().trim_end_matches('/');
        let name = value.rsplit('/').next().unwrap_or_default();
        let name = name.split(['?', '#']).next().unwrap_or_default();
        if name.is_empty() || name.contains("..") || name.starts_with('.') {
            return Err(DesignSpaceGatewayError::Backend(format!(
                "invalid design-space address: {}",
                project.design_space_url
            )));
        }
        Ok(name.to_string())
    }

    fn url(&self, project: &Project, path: &str) -> Result<String, DesignSpaceGatewayError> {
        Ok(format!("{}/{}{path}", self.base, Self::repo_name(project)?))
    }

    async fn request(
        &self,
        method: reqwest::Method,
        project: &Project,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response, DesignSpaceGatewayError> {
        let mut request = self.client.request(method, self.url(project, path)?);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        request
            .send()
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))
    }

    async fn get_text(
        &self,
        project: &Project,
        path: &str,
    ) -> Result<String, DesignSpaceGatewayError> {
        let response = self
            .request(reqwest::Method::GET, project, path, None)
            .await?;
        if !response.status().is_success() {
            return Err(DesignSpaceGatewayError::Backend(format!(
                "ds-server GET {path}: {}",
                response.status()
            )));
        }
        response
            .text()
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))
    }

    async fn get_bytes(
        &self,
        project: &Project,
        path: &str,
    ) -> Result<Vec<u8>, DesignSpaceGatewayError> {
        let response = self
            .request(reqwest::Method::GET, project, path, None)
            .await?;
        if !response.status().is_success() {
            return Err(DesignSpaceGatewayError::Backend(format!(
                "ds-server GET {path}: {}",
                response.status()
            )));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))
    }

    async fn refs(&self, project: &Project) -> Result<BTreeMap<String, String>, DesignSpaceGatewayError> {
        let text = self.get_text(project, "/refs").await?;
        let mut refs = BTreeMap::new();
        for line in text.lines().filter(|line| !line.is_empty()) {
            let Some((branch, commit)) = line.split_once('\t') else {
                continue;
            };
            refs.insert(branch.to_string(), commit.to_string());
        }
        Ok(refs)
    }

    async fn mirror(&self, project: &Project) -> Result<PathBuf, DesignSpaceGatewayError> {
        let _guard = self.sync.lock().await;
        let repo = Self::repo_name(project)?;
        let root = self.cache_root.join(&repo);
        std::fs::create_dir_all(&root)
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        let listing = self.get_text(project, "/files").await?;
        for relative in listing.lines().filter(|line| !line.is_empty()) {
            let target = root.join(relative);
            if target.is_file() {
                continue;
            }
            let bytes = self
                .get_bytes(project, &format!("/files/{relative}"))
                .await?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
            }
            std::fs::write(&target, bytes)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        }
        Ok(root)
    }

    async fn open_store(&self, project: &Project) -> Result<(PathBuf, Store), DesignSpaceGatewayError> {
        let root = self.mirror(project).await?;
        let store = Store::open(&root)
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        Ok((root, store))
    }

    fn commit(hex: &str) -> Result<CommitHash, DesignSpaceGatewayError> {
        Hash32::from_hex(hex)
            .map(CommitHash)
            .ok_or_else(|| DesignSpaceGatewayError::Backend(format!("invalid commit {hex}")))
    }

    fn is_ancestor_store(
        store: &Store,
        ancestor: CommitHash,
        descendant: CommitHash,
    ) -> Result<bool, DesignSpaceGatewayError> {
        let mut current = descendant;
        let mut steps = 0u32;
        while !current.is_zero() {
            if current == ancestor {
                return Ok(true);
            }
            if steps > 1_000_000 {
                return Err(DesignSpaceGatewayError::Conflict(
                    "commit history is too deep".to_string(),
                ));
            }
            current = store
                .get_commit(current)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
                .parent;
            steps += 1;
        }
        Ok(false)
    }

    fn diff_store(
        store: &Store,
        from: CommitHash,
        to: CommitHash,
    ) -> Result<Vec<MergeRequestChange>, DesignSpaceGatewayError> {
        let from_commit = store
            .get_commit(from)
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        let to_commit = store
            .get_commit(to)
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        let from_catalog = Catalog::load(store, from_commit.view)
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        let to_catalog = Catalog::load(store, to_commit.view)
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        let mut names = BTreeSet::new();
        names.extend(from_catalog.iter().map(|(name, _)| name));
        names.extend(to_catalog.iter().map(|(name, _)| name));
        let mut changes = Vec::new();
        for qname in names {
            let qname_text = store
                .get_symbol(qname)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
            match (from_catalog.get(qname), to_catalog.get(qname)) {
                (None, Some(object)) => changes.push(MergeRequestChange {
                    status: MergeRequestChangeStatus::Added,
                    qname: qname_text,
                    kind: kind_name(object.kind).to_string(),
                }),
                (Some(object), None) => changes.push(MergeRequestChange {
                    status: MergeRequestChangeStatus::Removed,
                    qname: qname_text,
                    kind: kind_name(object.kind).to_string(),
                }),
                (Some(before), Some(after)) if before != after => {
                    changes.push(MergeRequestChange {
                        status: MergeRequestChangeStatus::Modified,
                        qname: qname_text,
                        kind: kind_name(after.kind).to_string(),
                    });
                }
                _ => {}
            }
        }
        changes.sort_by(|left, right| left.qname.cmp(&right.qname));
        Ok(changes)
    }
}

fn document_comments(object: &design_space::model::DesignObject) -> &[DocComment] {
    match &object.payload {
        ObjectPayload::DocumentDirectory(p) => &p.comments,
        ObjectPayload::Document(p) => &p.comments,
        ObjectPayload::DocumentSection(p) => &p.comments,
        ObjectPayload::DocumentList(p) => &p.comments,
        ObjectPayload::DocumentText(p) => &p.comments,
        ObjectPayload::DocumentTopic(p) => &p.comments,
        _ => &[],
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::View => "view",
        Kind::Directory => "directory",
        Kind::File => "file",
        Kind::Namespace => "namespace",
        Kind::Class => "class",
        Kind::InnerClass => "inner-class",
        Kind::Interface => "interface",
        Kind::InnerInterface => "inner-interface",
        Kind::Method => "method",
        Kind::Function => "function",
        Kind::Part => "part",
        Kind::Mixin => "mixin",
        Kind::DocumentDirectory => "document-directory",
        Kind::Document => "document",
        Kind::DocumentSection => "document-section",
        Kind::DocumentList => "document-list",
        Kind::DocumentText => "document-text",
        Kind::DocumentTopic => "document-topic",
        Kind::DesignStructure => "design-structure",
        Kind::System => "system",
        Kind::Module => "module",
    }
}

#[async_trait]
impl DesignSpaceGateway for RemoteDesignSpaceGateway {
    async fn branches(&self, project: &Project) -> Result<Vec<String>, DesignSpaceGatewayError> {
        Ok(self.refs(project).await?.into_keys().collect())
    }

    async fn branch_head(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<Option<String>, DesignSpaceGatewayError> {
        Ok(self.refs(project).await?.get(branch).cloned())
    }

    async fn is_ancestor(
        &self,
        project: &Project,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, DesignSpaceGatewayError> {
        let (_, store) = self.open_store(project).await?;
        let ancestor = Self::commit(ancestor)?;
        let descendant = Self::commit(descendant)?;
        tokio::task::spawn_blocking(move || Self::is_ancestor_store(&store, ancestor, descendant))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
    }

    async fn diff(
        &self,
        project: &Project,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<MergeRequestChange>, DesignSpaceGatewayError> {
        let (_, store) = self.open_store(project).await?;
        let from = Self::commit(from_commit)?;
        let to = Self::commit(to_commit)?;
        tokio::task::spawn_blocking(move || Self::diff_store(&store, from, to))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
    }

    async fn comment_count(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<usize, DesignSpaceGatewayError> {
        let (_, store) = self.open_store(project).await?;
        let commit = Self::commit(commit)?;
        tokio::task::spawn_blocking(move || {
            let commit = store
                .get_commit(commit)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
            let catalog = Catalog::load(&store, commit.view)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
            Ok(catalog
                .iter()
                .map(|(_, object)| document_comments(object).len())
                .sum())
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
    }

    async fn content_digest(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<String, DesignSpaceGatewayError> {
        let (_, store) = self.open_store(project).await?;
        let commit = Self::commit(commit)?;
        tokio::task::spawn_blocking(move || {
            let commit = store
                .get_commit(commit)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
            let catalog = Catalog::load(&store, commit.view)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
            catalog
                .content_digest()
                .map(|digest| digest.to_hex())
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
    }

    async fn fast_forward(
        &self,
        project: &Project,
        target_branch: &str,
        expected_target: &str,
        source_commit: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let (_, store) = self.open_store(project).await?;
        let refs = self.refs(project).await?;
        let current = refs.get(target_branch).ok_or_else(|| {
            DesignSpaceGatewayError::NotFound(format!("branch not found: {target_branch}"))
        })?;
        if current == source_commit {
            return Ok(());
        }
        if current != expected_target {
            return Err(DesignSpaceGatewayError::Conflict(
                "target branch moved after the merge request was created".to_string(),
            ));
        }
        let target = Self::commit(expected_target)?;
        let source = Self::commit(source_commit)?;
        let ancestor = tokio::task::spawn_blocking({
            let store = store.clone();
            move || Self::is_ancestor_store(&store, target, source)
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))??;
        if !ancestor {
            return Err(DesignSpaceGatewayError::Conflict(
                "target is not an ancestor of source".to_string(),
            ));
        }
        store
            .get_commit(source)
            .map_err(|error| DesignSpaceGatewayError::NotFound(error.to_string()))?;
        let body = format!("{target_branch}\t{source_commit}\n").into_bytes();
        let response = self
            .request(reqwest::Method::PUT, project, "/refs", Some(body))
            .await?;
        if !response.status().is_success() {
            return Err(DesignSpaceGatewayError::Backend(format!(
                "ds-server PUT /refs: {}",
                response.status()
            )));
        }
        Ok(())
    }

    async fn delete_branch(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let response = self
            .request(
                reqwest::Method::DELETE,
                project,
                &format!("/refs/{branch}"),
                None,
            )
            .await?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(DesignSpaceGatewayError::Backend(format!(
                "ds-server DELETE /refs/{branch}: {}",
                response.status()
            )))
        }
    }
}
