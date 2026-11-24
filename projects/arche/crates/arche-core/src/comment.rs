use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::codec::{Reader, Writer};
use crate::util::write_atomic;

const COMMENT_MAGIC: &[u8; 4] = b"CMT1";
const COMMENT_VERSION: u8 = 3;

/// A comment attached to a design object by qname.
///
/// Comments live outside the versioned object graph: they are not content
/// addressed, not part of commits or snapshots, and do not participate in the
/// property system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: u64,
    pub target: String,
    pub body: String,
    pub conclusion: String,
    pub resolved_by: String,
    pub resolved: bool,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
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

    pub fn add(&self, target: &str, body: &str, created_by: &str) -> Result<Comment> {
        let mut comments = self.load()?;
        let id = comments.iter().map(|comment| comment.id).max().unwrap_or(0) + 1;
        let now = now();
        let comment = Comment {
            id,
            target: target.to_owned(),
            body: body.to_owned(),
            conclusion: String::new(),
            resolved_by: String::new(),
            resolved: false,
            created_by: created_by.to_owned(),
            created_at: now,
            updated_at: now,
        };
        comments.push(comment.clone());
        self.save(&comments)?;
        Ok(comment)
    }

    pub fn list(&self, target: Option<&str>, open_only: bool) -> Result<Vec<Comment>> {
        Ok(self
            .load()?
            .into_iter()
            .filter(|comment| !open_only || !comment.resolved)
            .filter(|comment| target.is_none_or(|target| comment.target == target))
            .collect())
    }

    /// Remove comments for one target, or all comments when `target` is `None`.
    pub fn clear(&self, target: Option<&str>) -> Result<usize> {
        let mut comments = self.load()?;
        let before = comments.len();
        match target {
            Some(target) => comments.retain(|comment| comment.target != target),
            None => comments.clear(),
        }
        self.save(&comments)?;
        Ok(before - comments.len())
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

    fn load(&self) -> Result<Vec<Comment>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", self.path.display()));
            }
        };
        let mut reader = Reader::new(&bytes);
        if reader.bytes(4)? != COMMENT_MAGIC {
            bail!("invalid comment store magic");
        }
        let version = reader.u8()?;
        if version != COMMENT_VERSION {
            bail!("unsupported comment store version {version}");
        }
        let count = reader.u32()? as usize;
        let mut comments = Vec::with_capacity(count);
        for _ in 0..count {
            comments.push(Comment {
                id: reader.u64()?,
                target: reader.text()?,
                body: reader.text()?,
                conclusion: reader.text()?,
                resolved_by: reader.text()?,
                resolved: reader.u8()? != 0,
                created_by: reader.text()?,
                created_at: reader.u64()? as i64,
                updated_at: reader.u64()? as i64,
            });
        }
        reader.finish()?;
        Ok(comments)
    }

    fn save(&self, comments: &[Comment]) -> Result<()> {
        let mut writer = Writer::new();
        writer.raw(COMMENT_MAGIC);
        writer.u8(COMMENT_VERSION);
        writer.u32(comments.len() as u32);
        for comment in comments {
            writer.u64(comment.id);
            writer.text(&comment.target);
            writer.text(&comment.body);
            writer.text(&comment.conclusion);
            writer.text(&comment.resolved_by);
            writer.u8(u8::from(comment.resolved));
            writer.text(&comment.created_by);
            writer.u64(comment.created_at as u64);
            writer.u64(comment.updated_at as u64);
        }
        write_atomic(&self.path, &writer.finish(), &self.tmp)
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
