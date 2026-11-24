use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::codec::{Reader, Writer};
use crate::util::write_atomic;

const COMMENT_MAGIC: &[u8; 4] = b"CMT1";
const COMMENT_VERSION: u8 = 5;

/// A comment attached to a design object by qname.
///
/// Comments live outside the versioned object graph: they are not content
/// addressed, not part of commits or snapshots, and do not participate in the
/// property system. They are tagged with the branch they were written on so
/// push and fetch can synchronize a branch's review comments without mixing
/// comments from other branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: u64,
    pub branch: String,
    pub target: String,
    pub body: String,
    pub conclusion: String,
    pub resolved_by: String,
    pub resolved: bool,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Cleared comments stay as tombstones so the deletion is pushed and
    /// merged like any other comment state change.
    pub deleted: bool,
}

#[derive(Debug, Clone)]
pub struct CommentStore {
    path: PathBuf,
    tmp: PathBuf,
}

impl CommentStore {
    pub fn new(dir: &Path, tmp: PathBuf) -> Self {
        Self {
            path: dir.join("comments"),
            tmp,
        }
    }

    pub fn add(&self, branch: &str, target: &str, body: &str, created_by: &str) -> Result<Comment> {
        let mut comments = self.load()?;
        let id = next_id(&comments);
        let now = now();
        let comment = Comment {
            id,
            branch: branch.to_owned(),
            target: target.to_owned(),
            body: body.to_owned(),
            conclusion: String::new(),
            resolved_by: String::new(),
            resolved: false,
            created_by: created_by.to_owned(),
            created_at: now,
            updated_at: now,
            deleted: false,
        };
        comments.push(comment.clone());
        self.save(&comments)?;
        Ok(comment)
    }

    pub fn list(
        &self,
        branch: &str,
        target: Option<&str>,
        open_only: bool,
    ) -> Result<Vec<Comment>> {
        self.list_filtered(branch, open_only, |comment| {
            target.is_none_or(|target| comment.target == target)
        })
    }

    /// List comments on `target` and every qname below it, including its
    /// fields and anchored positions (`target/field`, `target/field#anchor`).
    pub fn list_subtree(
        &self,
        branch: &str,
        target: &str,
        open_only: bool,
    ) -> Result<Vec<Comment>> {
        let root = target.trim_end_matches('/');
        self.list_filtered(branch, open_only, |comment| {
            comment.target == root
                || comment
                    .target
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('/') || rest.starts_with('#'))
        })
    }

    fn list_filtered(
        &self,
        branch: &str,
        open_only: bool,
        matches_target: impl Fn(&Comment) -> bool,
    ) -> Result<Vec<Comment>> {
        Ok(self
            .load()?
            .into_iter()
            .filter(|comment| !comment.deleted)
            .filter(|comment| comment_on_branch(comment, branch))
            .filter(|comment| !open_only || !comment.resolved)
            .filter(|comment| matches_target(comment))
            .collect())
    }

    /// Remove comments for one target on a branch, or every comment on the
    /// branch when `target` is `None`.
    pub fn clear(&self, branch: &str, target: Option<&str>) -> Result<usize> {
        let mut comments = self.load()?;
        let now = now();
        let mut cleared = 0;
        for comment in &mut comments {
            if comment.deleted
                || !comment_on_branch(comment, branch)
                || target.is_some_and(|target| comment.target != target)
            {
                continue;
            }
            comment.deleted = true;
            comment.updated_at = now;
            cleared += 1;
        }
        self.save(&comments)?;
        Ok(cleared)
    }

    pub fn resolve(&self, id: u64, conclusion: &str, agent: &str) -> Result<Comment> {
        let conclusion = conclusion.trim();
        if conclusion.is_empty() {
            bail!("comment conclusion must not be empty");
        }
        let agent = agent.trim();
        if agent.is_empty() {
            bail!("comment resolver agent must not be empty");
        }
        let mut comments = self.load()?;
        let comment = comments
            .iter_mut()
            .find(|comment| comment.id == id)
            .with_context(|| format!("comment {id} not found"))?;
        comment.resolved = true;
        comment.conclusion = conclusion.to_owned();
        comment.resolved_by = agent.to_owned();
        comment.updated_at = now();
        let comment = comment.clone();
        self.save(&comments)?;
        Ok(comment)
    }

    /// Serialize every branch's comments for transfer.
    pub fn export(&self) -> Result<Vec<u8>> {
        Ok(encode_comments(&self.load()?))
    }

    /// Replace the local store with a complete remote snapshot. Used after a
    /// push, when the server returns the authoritative id assignment.
    pub fn replace(&self, bytes: &[u8]) -> Result<Vec<Comment>> {
        let comments = decode_comments(bytes)?;
        self.save(&comments)?;
        Ok(comments)
    }

    /// Merge a remote comment snapshot into the local store and return the
    /// merged store. `remote_authoritative` is true when the server's ids win
    /// (fetch); false when an incoming writer must be renumbered around ids
    /// already assigned by the server (push).
    pub fn merge(&self, bytes: &[u8], remote_authoritative: bool) -> Result<Vec<u8>> {
        let incoming = decode_comments(bytes)?;
        let mut comments = self.load()?;
        for mut remote in incoming {
            if let Some(index) = comments
                .iter()
                .position(|comment| same_comment(comment, &remote))
            {
                comments.remove(index);
            }
            if let Some(index) = comments.iter().position(|comment| comment.id == remote.id) {
                if remote_authoritative {
                    comments[index].id = next_id(&comments);
                } else {
                    remote.id = next_id(&comments);
                }
            }
            comments.push(remote);
        }
        self.save(&comments)?;
        Ok(encode_comments(&comments))
    }

    fn load(&self) -> Result<Vec<Comment>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", self.path.display()));
            }
        };
        decode_comments(&bytes)
    }

    fn save(&self, comments: &[Comment]) -> Result<()> {
        write_atomic(&self.path, &encode_comments(comments), &self.tmp)
    }
}

fn comment_on_branch(comment: &Comment, branch: &str) -> bool {
    comment.branch.is_empty() || comment.branch == branch
}

fn same_comment(left: &Comment, right: &Comment) -> bool {
    left.branch == right.branch
        && left.target == right.target
        && left.body == right.body
        && left.created_by == right.created_by
        && left.created_at == right.created_at
}

fn next_id(comments: &[Comment]) -> u64 {
    comments.iter().map(|comment| comment.id).max().unwrap_or(0) + 1
}

fn encode_comments(comments: &[Comment]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.raw(COMMENT_MAGIC);
    writer.u8(COMMENT_VERSION);
    writer.u32(comments.len() as u32);
    for comment in comments {
        writer.u64(comment.id);
        writer.text(&comment.branch);
        writer.text(&comment.target);
        writer.text(&comment.body);
        writer.text(&comment.conclusion);
        writer.text(&comment.resolved_by);
        writer.u8(u8::from(comment.resolved));
        writer.text(&comment.created_by);
        writer.u64(comment.created_at as u64);
        writer.u64(comment.updated_at as u64);
        writer.u8(u8::from(comment.deleted));
    }
    writer.finish()
}

fn decode_comments(bytes: &[u8]) -> Result<Vec<Comment>> {
    let mut reader = Reader::new(bytes);
    if reader.bytes(4)? != COMMENT_MAGIC {
        bail!("invalid comment store magic");
    }
    let version = reader.u8()?;
    if !matches!(version, 3 | 4 | COMMENT_VERSION) {
        bail!("unsupported comment store version {version}");
    }
    let count = reader.u32()? as usize;
    let mut comments = Vec::with_capacity(count);
    for _ in 0..count {
        let id = reader.u64()?;
        let branch = if version >= 4 {
            reader.text()?
        } else {
            String::new()
        };
        comments.push(Comment {
            id,
            branch,
            target: reader.text()?,
            body: reader.text()?,
            conclusion: reader.text()?,
            resolved_by: reader.text()?,
            resolved: reader.u8()? != 0,
            created_by: reader.text()?,
            created_at: reader.u64()? as i64,
            updated_at: reader.u64()? as i64,
            deleted: version >= 5 && reader.u8()? != 0,
        });
    }
    reader.finish()?;
    Ok(comments)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
