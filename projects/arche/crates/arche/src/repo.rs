use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::commit::Commit;
use crate::hash::{CommitHash, Hash32, ObjectHash, SymbolHash};
use crate::object::{
    Concept, ConceptDirectory, DocumentHtml, DocumentList, DocumentMermaid, DocumentParagraphList,
    DocumentText, DocumentTopic, Inline, Object, ObjectList, Paragraph, RequirementFunction,
    RequirementInteraction, RequirementModule, RequirementStory, RequirementSystem,
    RequirementUser, Requirements, Technical, TechnicalFile, TechnicalSystem, View,
};
use crate::snapshot::Snapshot;
use crate::store::{CommitStore, ObjectStore};
use crate::symbol::SymbolStore;
use crate::util::{list_files, write_atomic};

pub const ARCHE_DIR: &str = ".arche";
const MAIN_REF: &str = "refs/heads/main";
static WORKTREE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    dir: PathBuf,
    tmp: PathBuf,
    source_url: Option<String>,
    remote_url: Option<String>,
    symbols: SymbolStore,
    objects: ObjectStore,
    commits: CommitStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocFieldKind {
    Text,
    List,
    ParagraphList,
    Mermaid,
}

#[derive(Debug, Clone, Copy)]
enum ListField {
    Users,
    Stories,
    Systems,
    Modules,
    Functions,
    Interactions,
}

impl ListField {
    fn name(self) -> &'static str {
        match self {
            Self::Users => "users",
            Self::Stories => "stories",
            Self::Systems => "systems",
            Self::Modules => "modules",
            Self::Functions => "functions",
            Self::Interactions => "interactions",
        }
    }

    fn get(self, object: &Object) -> Result<Option<SymbolHash>> {
        Ok(match (self, object) {
            (Self::Users, Object::Requirements(value)) => value.users,
            (Self::Stories, Object::Requirements(value)) => value.stories,
            (Self::Systems, Object::Requirements(value)) => value.systems,
            (Self::Modules, Object::RequirementSystem(value)) => value.modules,
            (Self::Functions, Object::RequirementModule(value)) => value.functions,
            (Self::Interactions, Object::RequirementFunction(value)) => value.interactions,
            (_, other) => bail!("{} has no {} list", other.kind().name(), self.name()),
        })
    }

    fn set(self, object: &mut Object, value: SymbolHash) -> Result<()> {
        match (self, object) {
            (Self::Users, Object::Requirements(object)) => object.users = Some(value),
            (Self::Stories, Object::Requirements(object)) => object.stories = Some(value),
            (Self::Systems, Object::Requirements(object)) => object.systems = Some(value),
            (Self::Modules, Object::RequirementSystem(object)) => object.modules = Some(value),
            (Self::Functions, Object::RequirementModule(object)) => object.functions = Some(value),
            (Self::Interactions, Object::RequirementFunction(object)) => {
                object.interactions = Some(value);
            }
            (_, other) => bail!("{} has no {} list", other.kind().name(), self.name()),
        }
        Ok(())
    }
}

fn doc_field_kind(object: &Object, field: &str) -> Result<DocFieldKind> {
    let kind = match object {
        Object::TechnicalSystem(_)
        | Object::TechnicalModule(_)
        | Object::TechnicalDirectory(_)
        | Object::TechnicalFile(_) => match field {
            "position" => DocFieldKind::Text,
            "responsibilities"
            | "non_responsibilities"
            | "features"
            | "non_features"
            | "invariants"
            | "constraints"
            | "design_principles"
            | "external_dependencies" => DocFieldKind::List,
            "usage" | "design_points" => DocFieldKind::ParagraphList,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::RequirementUser(_) => match field {
            "age" | "basic_situation" | "hobbies" | "occupation" | "priority" => DocFieldKind::Text,
            "typical_scenario" => DocFieldKind::ParagraphList,
            "needs" => DocFieldKind::List,
            _ => bail!("RequirementUser has no field {field}"),
        },
        Object::RequirementStory(_) => match field {
            "frequency" => DocFieldKind::Text,
            "preconditions" | "description" => DocFieldKind::ParagraphList,
            "related_requirements" => DocFieldKind::List,
            _ => bail!("RequirementStory has no field {field}"),
        },
        Object::RequirementSystem(_) => match field {
            "form" => DocFieldKind::Text,
            "covered_users" | "boundary" => DocFieldKind::List,
            "closed_loop_goal" | "relationships" => DocFieldKind::ParagraphList,
            _ => bail!("RequirementSystem has no field {field}"),
        },
        Object::RequirementModule(_) => match field {
            "core_concept" => DocFieldKind::Text,
            "included_concepts" | "boundary" => DocFieldKind::List,
            "concept_relations" => DocFieldKind::ParagraphList,
            _ => bail!("RequirementModule has no field {field}"),
        },
        Object::RequirementFunction(_) => match field {
            "initiator" | "feedback" => DocFieldKind::Text,
            "system_change" | "preconditions" | "description" => DocFieldKind::ParagraphList,
            "flow" => DocFieldKind::Mermaid,
            _ => bail!("RequirementFunction has no field {field}"),
        },
        Object::RequirementInteraction(_) => match field {
            "description" | "ui" => DocFieldKind::ParagraphList,
            _ => bail!("RequirementInteraction has no field {field}"),
        },
        Object::Concept(_) => match field {
            "summary" => DocFieldKind::Text,
            "explanation" => DocFieldKind::ParagraphList,
            "characteristics" | "relations" | "aliases" => DocFieldKind::List,
            _ => bail!("Concept has no field {field}"),
        },
        _ => bail!("{} has no document fields", object.kind().name()),
    };
    Ok(kind)
}

fn set_doc_field(object: &mut Object, field: &str, value: SymbolHash) -> Result<()> {
    match object {
        Object::TechnicalSystem(object) => match field {
            "position" => object.position = Some(value),
            "responsibilities" => object.responsibilities = Some(value),
            "non_responsibilities" => object.non_responsibilities = Some(value),
            "features" => object.features = Some(value),
            "non_features" => object.non_features = Some(value),
            "invariants" => object.invariants = Some(value),
            "constraints" => object.constraints = Some(value),
            "design_principles" => object.design_principles = Some(value),
            "external_dependencies" => object.external_dependencies = Some(value),
            "usage" => object.usage = Some(value),
            "design_points" => object.design_points = Some(value),
            _ => bail!("TechnicalSystem has no field {field}"),
        },
        Object::TechnicalDirectory(object) => match field {
            "position" => object.position = Some(value),
            _ => bail!("TechnicalDirectory has no field {field}"),
        },
        Object::TechnicalFile(object) => match field {
            "position" => object.position = Some(value),
            _ => bail!("TechnicalFile has no field {field}"),
        },
        Object::TechnicalModule(object) => match field {
            "position" => object.position = Some(value),
            "responsibilities" => object.responsibilities = Some(value),
            "non_responsibilities" => object.non_responsibilities = Some(value),
            "features" => object.features = Some(value),
            "non_features" => object.non_features = Some(value),
            "invariants" => object.invariants = Some(value),
            "constraints" => object.constraints = Some(value),
            "design_principles" => object.design_principles = Some(value),
            "external_dependencies" => object.external_dependencies = Some(value),
            "usage" => object.usage = Some(value),
            "design_points" => object.design_points = Some(value),
            _ => bail!("TechnicalModule has no field {field}"),
        },
        Object::RequirementUser(object) => match field {
            "age" => object.age = Some(value),
            "basic_situation" => object.basic_situation = Some(value),
            "hobbies" => object.hobbies = Some(value),
            "occupation" => object.occupation = Some(value),
            "typical_scenario" => object.typical_scenario = Some(value),
            "needs" => object.needs = Some(value),
            "priority" => object.priority = Some(value),
            _ => bail!("RequirementUser has no field {field}"),
        },
        Object::RequirementStory(object) => match field {
            "preconditions" => object.preconditions = Some(value),
            "frequency" => object.frequency = Some(value),
            "description" => object.description = Some(value),
            "related_requirements" => object.related_requirements = Some(value),
            _ => bail!("RequirementStory has no field {field}"),
        },
        Object::RequirementSystem(object) => match field {
            "form" => object.form = Some(value),
            "covered_users" => object.covered_users = Some(value),
            "closed_loop_goal" => object.closed_loop_goal = Some(value),
            "relationships" => object.relationships = Some(value),
            "boundary" => object.boundary = Some(value),
            _ => bail!("RequirementSystem has no field {field}"),
        },
        Object::RequirementModule(object) => match field {
            "core_concept" => object.core_concept = Some(value),
            "included_concepts" => object.included_concepts = Some(value),
            "concept_relations" => object.concept_relations = Some(value),
            "boundary" => object.boundary = Some(value),
            _ => bail!("RequirementModule has no field {field}"),
        },
        Object::RequirementFunction(object) => match field {
            "flow" => object.flow = Some(value),
            "initiator" => object.initiator = Some(value),
            "system_change" => object.system_change = Some(value),
            "preconditions" => object.preconditions = Some(value),
            "feedback" => object.feedback = Some(value),
            "description" => object.description = Some(value),
            _ => bail!("RequirementFunction has no field {field}"),
        },
        Object::RequirementInteraction(object) => match field {
            "description" => object.description = Some(value),
            "ui" => object.ui = Some(value),
            _ => bail!("RequirementInteraction has no field {field}"),
        },
        Object::Concept(object) => match field {
            "summary" => object.summary = Some(value),
            "explanation" => object.explanation = Some(value),
            "characteristics" => object.characteristics = Some(value),
            "relations" => object.relations = Some(value),
            "aliases" => object.aliases = Some(value),
            _ => bail!("Concept has no field {field}"),
        },
        other => bail!("{} has no document fields", other.kind().name()),
    }
    Ok(())
}

pub fn doc_fields(object: &Object) -> Vec<(&'static str, Option<SymbolHash>)> {
    match object {
        Object::TechnicalSystem(value) => vec![
            ("position", value.position),
            ("responsibilities", value.responsibilities),
            ("non_responsibilities", value.non_responsibilities),
            ("features", value.features),
            ("non_features", value.non_features),
            ("invariants", value.invariants),
            ("constraints", value.constraints),
            ("design_principles", value.design_principles),
            ("external_dependencies", value.external_dependencies),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::TechnicalDirectory(value) => vec![("position", value.position)],
        Object::TechnicalFile(value) => vec![("position", value.position)],
        Object::TechnicalModule(value) => vec![
            ("position", value.position),
            ("responsibilities", value.responsibilities),
            ("non_responsibilities", value.non_responsibilities),
            ("features", value.features),
            ("non_features", value.non_features),
            ("invariants", value.invariants),
            ("constraints", value.constraints),
            ("design_principles", value.design_principles),
            ("external_dependencies", value.external_dependencies),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::RequirementUser(value) => vec![
            ("age", value.age),
            ("basic_situation", value.basic_situation),
            ("hobbies", value.hobbies),
            ("occupation", value.occupation),
            ("typical_scenario", value.typical_scenario),
            ("needs", value.needs),
            ("priority", value.priority),
        ],
        Object::RequirementStory(value) => vec![
            ("preconditions", value.preconditions),
            ("frequency", value.frequency),
            ("description", value.description),
            ("related_requirements", value.related_requirements),
        ],
        Object::RequirementSystem(value) => vec![
            ("form", value.form),
            ("covered_users", value.covered_users),
            ("closed_loop_goal", value.closed_loop_goal),
            ("relationships", value.relationships),
            ("boundary", value.boundary),
        ],
        Object::RequirementModule(value) => vec![
            ("core_concept", value.core_concept),
            ("included_concepts", value.included_concepts),
            ("concept_relations", value.concept_relations),
            ("boundary", value.boundary),
        ],
        Object::RequirementFunction(value) => vec![
            ("flow", value.flow),
            ("initiator", value.initiator),
            ("system_change", value.system_change),
            ("preconditions", value.preconditions),
            ("feedback", value.feedback),
            ("description", value.description),
        ],
        Object::RequirementInteraction(value) => {
            vec![("description", value.description), ("ui", value.ui)]
        }
        Object::Concept(value) => vec![
            ("summary", value.summary),
            ("explanation", value.explanation),
            ("characteristics", value.characteristics),
            ("relations", value.relations),
            ("aliases", value.aliases),
        ],
        _ => Vec::new(),
    }
}

fn paragraph_qname(paragraph: Paragraph) -> SymbolHash {
    match paragraph {
        Paragraph::Text(qname)
        | Paragraph::List(qname)
        | Paragraph::Topic(qname)
        | Paragraph::Code(qname)
        | Paragraph::Mermaid(qname)
        | Paragraph::Html(qname) => qname,
    }
}

fn expected_doc_kind(kind: DocFieldKind) -> crate::object::ObjectKind {
    match kind {
        DocFieldKind::Text => crate::object::ObjectKind::DocumentText,
        DocFieldKind::List => crate::object::ObjectKind::DocumentList,
        DocFieldKind::ParagraphList => crate::object::ObjectKind::DocumentParagraphList,
        DocFieldKind::Mermaid => crate::object::ObjectKind::DocumentMermaid,
    }
}

impl Repository {
    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        Self::init_with_source(path, None)
    }

    pub fn init_with_source(path: impl AsRef<Path>, source_url: Option<&str>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let dir = root.join(ARCHE_DIR);
        if dir.exists() {
            bail!("{} already exists", dir.display());
        }
        fs::create_dir_all(&root)
            .with_context(|| format!("create repository root {}", root.display()))?;
        let tmp = dir.join("tmp");
        for child in ["objects", "symbols", "commits", "refs/heads", "tmp"] {
            fs::create_dir_all(dir.join(child))
                .with_context(|| format!("create {}", dir.join(child).display()))?;
        }
        let repo = Self {
            symbols: SymbolStore::new(dir.join("symbols"), tmp.clone()),
            objects: ObjectStore::new(dir.join("objects"), tmp.clone()),
            commits: CommitStore::new(dir.join("commits"), tmp.clone()),
            root,
            dir,
            tmp,
            source_url: source_url.map(str::to_owned),
            remote_url: None,
        };
        repo.bootstrap()?;
        Ok(repo)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let dir = root.join(ARCHE_DIR);
        if !dir.is_dir() {
            bail!("{} is not an Arche repository", root.display());
        }
        let tmp = dir.join("tmp");
        let config = fs::read_to_string(dir.join("config")).unwrap_or_default();
        let source_url = config
            .lines()
            .find_map(|line| line.strip_prefix("source=").map(str::to_owned));
        let remote_url = config
            .lines()
            .find_map(|line| line.strip_prefix("remote=").map(str::to_owned));
        Ok(Self {
            symbols: SymbolStore::new(dir.join("symbols"), tmp.clone()),
            objects: ObjectStore::new(dir.join("objects"), tmp.clone()),
            commits: CommitStore::new(dir.join("commits"), tmp.clone()),
            root,
            dir,
            tmp,
            source_url,
            remote_url,
        })
    }

    pub fn source_url(&self) -> Option<&str> {
        self.source_url.as_deref()
    }

    pub fn remote_url(&self) -> Option<&str> {
        self.remote_url.as_deref()
    }

    pub fn set_remote(&self, remote: &str) -> Result<()> {
        let path = self.dir.join("config");
        let text = fs::read_to_string(&path).unwrap_or_default();
        let mut lines = text
            .lines()
            .filter(|line| !line.starts_with("remote="))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        lines.push(format!("remote={remote}"));
        fs::write(path, format!("{}\n", lines.join("\n")))?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn intern(&self, text: &str) -> Result<SymbolHash> {
        self.symbols.intern(text)
    }

    pub fn resolve(&self, hash: SymbolHash) -> Result<String> {
        self.symbols.resolve(hash)
    }

    pub fn read(&self, hash: ObjectHash) -> Result<Object> {
        self.objects.read(hash)
    }

    pub fn list_objects(&self) -> Result<Vec<(ObjectHash, Object)>> {
        let mut result = Vec::new();
        for (_, hash) in self.worktree_snapshot()?.entries() {
            result.push((*hash, self.read(*hash)?));
        }
        Ok(result)
    }

    pub fn find_by_qname(&self, qname: &str) -> Result<ObjectHash> {
        let target = SymbolHash::of(qname);
        self.worktree_snapshot()?
            .get(target)
            .ok_or_else(|| anyhow::anyhow!("object {qname} not found"))
    }

    pub fn head_snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot::from_entries(self.head()?.objects))
    }

    pub fn index_snapshot(&self) -> Result<Snapshot> {
        self.read_snapshot("index")
    }

    pub fn worktree_snapshot(&self) -> Result<Snapshot> {
        self.read_snapshot("worktree")
    }

    fn read_snapshot(&self, name: &str) -> Result<Snapshot> {
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(Snapshot::default());
        }
        let bytes = fs::read(&path).with_context(|| format!("read snapshot {}", path.display()))?;
        Snapshot::decode(&bytes).with_context(|| format!("decode snapshot {}", path.display()))
    }

    fn write_snapshot(&self, name: &str, snapshot: &Snapshot) -> Result<()> {
        let path = self.dir.join(name);
        write_atomic(&path, &snapshot.encode(), &self.tmp)
    }

    pub fn current_branch(&self) -> Result<String> {
        let text = fs::read_to_string(self.dir.join("HEAD")).context("read HEAD")?;
        let text = text.trim();
        text.strip_prefix("ref: refs/heads/")
            .map(str::to_owned)
            .with_context(|| format!("unsupported HEAD {text}"))
    }

    pub fn head_commit(&self) -> Result<CommitHash> {
        let branch = self.current_branch()?;
        let path = self.dir.join("refs/heads").join(branch);
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        CommitHash::from_hex(text.trim())
    }

    pub fn branch_list(&self) -> Result<Vec<String>> {
        let mut branches = Vec::new();
        let dir = self.dir.join("refs/heads");
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    branches.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        branches.sort();
        Ok(branches)
    }

    pub fn branch_create(&self, name: &str) -> Result<()> {
        validate_branch_name(name)?;
        let path = self.dir.join("refs/heads").join(name);
        if path.exists() {
            bail!("branch {name} already exists");
        }
        write_ref_file(
            &self.dir,
            &format!("refs/heads/{name}"),
            self.head_commit()?,
        )?;
        Ok(())
    }

    pub fn branch_delete(&self, name: &str) -> Result<()> {
        if self.current_branch()? == name {
            bail!("cannot delete the current branch");
        }
        let path = self.dir.join("refs/heads").join(name);
        if !path.exists() {
            bail!("branch {name} does not exist");
        }
        fs::remove_file(path)?;
        Ok(())
    }

    pub fn checkout(&self, branch: &str, create: bool) -> Result<()> {
        validate_branch_name(branch)?;
        let path = self.dir.join("refs/heads").join(branch);
        if create {
            if path.exists() {
                bail!("branch {branch} already exists");
            }
            write_ref_file(
                &self.dir,
                &format!("refs/heads/{branch}"),
                self.head_commit()?,
            )?;
            fs::write(self.dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))?;
            return Ok(());
        }
        if !path.exists() {
            bail!("branch {branch} does not exist");
        }
        let head = self.head_snapshot()?;
        if self.index_snapshot()? != head || self.worktree_snapshot()? != head {
            bail!("index or worktree has local changes; commit or reset first");
        }
        let target = read_ref_file(&self.dir, &format!("refs/heads/{branch}"))?;
        let commit = self.commits.read(target)?;
        fs::write(self.dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))?;
        let snapshot = Snapshot::from_entries(commit.objects);
        self.write_snapshot("index", &snapshot)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    pub fn head(&self) -> Result<Commit> {
        self.commits.read(self.head_commit()?)
    }

    pub fn log(&self) -> Result<Vec<(CommitHash, Commit)>> {
        let mut result = Vec::new();
        let mut current = Some(self.head_commit()?);
        while let Some(hash) = current {
            let commit = self.commits.read(hash)?;
            current = commit.parent;
            result.push((hash, commit));
        }
        Ok(result)
    }

    pub fn source_fetch(&self) -> Result<()> {
        let url = self
            .source_url
            .as_deref()
            .context("repository has no source URL")?;
        let cache = self.source_cache();
        if !cache.join("HEAD").is_file() {
            if cache.exists() {
                fs::remove_dir_all(&cache)?;
            }
            if let Some(parent) = cache.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut command = Command::new("git");
            command.args(["clone", "--bare", "--quiet", url]);
            command.arg(&cache);
            run_git(command, "clone source")?;
        } else {
            let mut command = Command::new("git");
            command.arg("-C").arg(&cache).args([
                "fetch",
                "--prune",
                "--tags",
                "origin",
                "+refs/heads/*:refs/remotes/origin/*",
            ]);
            run_git(command, "fetch source")?;
        }
        self.source_worktree()?;
        Ok(())
    }

    pub fn source_worktree(&self) -> Result<PathBuf> {
        let root_git = self.root.join(".git");
        if root_git.exists() {
            let text = fs::read_to_string(&root_git)
                .with_context(|| format!("read {}", root_git.display()))?;
            if !text.contains(".arche/source/worktrees/root") {
                bail!("workspace root has a different .git pointer");
            }
            self.ensure_source_exclude()?;
            return Ok(self.root.clone());
        }
        let cache = self.source_cache();
        if !cache.join("HEAD").is_file() {
            bail!("source repository is missing; run `arche source fetch`");
        }
        let commit = self.source_git_text(&cache, ["rev-parse", "HEAD"])?;
        let commit = commit.trim();
        let parent = self
            .root
            .parent()
            .context("workspace root has no parent")?
            .to_path_buf();
        let temp = parent.join(format!(
            ".arche-worktree-{}-{}",
            std::process::id(),
            WORKTREE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if temp.exists() {
            fs::remove_dir_all(&temp)?;
        }
        let mut command = Command::new("git");
        command
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
            ])
            .arg("--git-dir")
            .arg(&cache)
            .args(["worktree", "add", "--detach", "--force"])
            .arg(&temp)
            .arg(commit);
        if let Err(error) = run_git(command, "create source worktree") {
            let _ = fs::remove_dir_all(&temp);
            return Err(error);
        }
        if temp.join(".arche").exists() {
            let _ = fs::remove_dir_all(&temp);
            bail!("source repository contains a .arche directory");
        }
        let gitfile = fs::read_to_string(temp.join(".git"))?;
        let admin = gitfile
            .trim()
            .strip_prefix("gitdir: ")
            .map(PathBuf::from)
            .context("source worktree has no gitdir pointer")?;
        for entry in fs::read_dir(&temp)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let target = self.root.join(entry.file_name());
            if target.exists() {
                let _ = fs::remove_dir_all(&temp);
                bail!(
                    "source file collides with workspace data: {}",
                    target.display()
                );
            }
            fs::rename(entry.path(), target)?;
        }
        let worktrees = cache.join("worktrees");
        let final_admin = worktrees.join("root");
        if final_admin.exists() {
            fs::remove_dir_all(&final_admin)?;
        }
        fs::create_dir_all(&worktrees)?;
        fs::rename(&admin, &final_admin)?;
        fs::write(root_git, "gitdir: .arche/source/worktrees/root\n")?;
        fs::write(final_admin.join("gitdir"), "../../../.git\n")?;
        let _ = fs::remove_dir_all(&temp);
        self.ensure_source_exclude()?;
        Ok(self.root.clone())
    }

    pub fn source_checkout(&self, git_ref: &str) -> Result<()> {
        if git_ref.trim().is_empty() || git_ref.starts_with('-') || git_ref.contains('\0') {
            bail!("invalid git ref");
        }
        let worktree = self.source_worktree()?;
        let dirty = self.source_git_text(&worktree, ["status", "--porcelain"])?;
        if !dirty.trim().is_empty() {
            bail!("source worktree has uncommitted changes");
        }
        let ref_text = git_ref.trim();
        let commit = self
            .source_git_text(
                &self.source_cache(),
                [
                    "rev-parse",
                    "--verify",
                    &format!("refs/remotes/origin/{ref_text}^{{commit}}"),
                ],
            )
            .or_else(|_| {
                self.source_git_text(
                    &self.source_cache(),
                    ["rev-parse", "--verify", &format!("{ref_text}^{{commit}}")],
                )
            })?;
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&worktree)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
            ])
            .args(["checkout", "--force", "--detach", commit.trim()]);
        run_git(command, "checkout source")?;
        Ok(())
    }

    pub fn source_status(&self) -> Result<String> {
        if !self.root.join(".git").is_file() {
            return Ok("no source worktree\n".to_owned());
        }
        self.source_git_text(&self.root, ["status", "--porcelain"])
    }

    pub fn source_sync(&self) -> Result<usize> {
        let _ = self.source_worktree()?;
        let output = self.source_git_bytes(&self.root, ["ls-files", "-z"])?;
        let mut snapshot = self.worktree_snapshot()?;
        let files_qname = self.intern("technical/files")?;
        let files_hash = snapshot
            .get(files_qname)
            .context("technical/files list not found")?;
        let mut files = match self.read(files_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("technical/files is {}", other.kind().name()),
        };
        let mut count = 0;
        for chunk in output.split(|byte| *byte == 0) {
            if chunk.is_empty() {
                continue;
            }
            let path = std::str::from_utf8(chunk)?;
            if path.starts_with(".arche/") || path == ".arche" {
                continue;
            }
            let qname_text = format!("technical/files/{path}");
            let qname = self.intern(&qname_text)?;
            let source_path = self.intern(path)?;
            let source_hash = Hash32::sha256(&fs::read(self.root.join(path))?);
            let position = snapshot
                .get(qname)
                .and_then(|hash| self.read(hash).ok())
                .and_then(|object| match object {
                    Object::TechnicalFile(value) => value.position,
                    _ => None,
                });
            let file = Object::TechnicalFile(TechnicalFile {
                qname,
                name: self.intern(path.rsplit('/').next().unwrap_or(path))?,
                parent: files_qname,
                source_path: Some(source_path),
                source_hash,
                children: None,
                position,
            });
            let hash = self.objects.write(&file)?;
            snapshot.insert(qname, hash);
            if !files.items.contains(&qname) {
                files.items.push(qname);
            }
            count += 1;
        }
        let files_hash = self.objects.write(&Object::ObjectList(files))?;
        snapshot.insert(files_qname, files_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(count)
    }

    pub fn add_file(&self, path: &str) -> Result<()> {
        self.add(&[
            format!("technical/files/{path}"),
            "technical/files".to_owned(),
            "technical".to_owned(),
        ])
    }

    pub fn remote_push(&self) -> Result<()> {
        let remote_url = self.remote_url.as_deref().context("no remote configured")?;
        if is_http(remote_url) {
            return self.http_push(remote_url);
        }
        if is_ssh(remote_url) {
            return self.ssh_push(remote_url);
        }
        let remote_path = remote_url;
        let remote = Self::open(remote_path)?;
        for name in ["objects", "symbols", "commits"] {
            copy_tree(&self.dir.join(name), &remote.dir.join(name))?;
        }
        let branch = self.current_branch()?;
        let head = self.head_commit()?;
        write_ref_file(&remote.dir, &format!("refs/heads/{branch}"), head)?;
        Ok(())
    }

    pub fn remote_fetch(&self) -> Result<()> {
        let remote_url = self.remote_url.as_deref().context("no remote configured")?;
        if is_http(remote_url) {
            return self.http_fetch(remote_url);
        }
        if is_ssh(remote_url) {
            return self.ssh_fetch(remote_url);
        }
        let remote_path = remote_url;
        let remote = Self::open(remote_path)?;
        for name in ["objects", "symbols", "commits"] {
            copy_tree(&remote.dir.join(name), &self.dir.join(name))?;
        }
        let branch = self.current_branch()?;
        let remote_head = read_ref_file(&remote.dir, &format!("refs/heads/{branch}"))?;
        write_ref_file(
            &self.dir,
            &format!("refs/remotes/origin/{branch}"),
            remote_head,
        )?;
        Ok(())
    }

    pub fn remote_pull(&self) -> Result<()> {
        let head = self.head_snapshot()?;
        if self.index_snapshot()? != head || self.worktree_snapshot()? != head {
            bail!("index or worktree has local changes; commit or reset first");
        }
        self.remote_fetch()?;
        let branch = self.current_branch()?;
        let remote_head = read_ref_file(&self.dir, &format!("refs/remotes/origin/{branch}"))?;
        let commit = self.commits.read(remote_head)?;
        write_ref_file(&self.dir, &format!("refs/heads/{branch}"), remote_head)?;
        let snapshot = Snapshot::from_entries(commit.objects);
        self.write_snapshot("index", &snapshot)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    pub fn clone_from(remote_path: &str, dir: impl AsRef<Path>) -> Result<Self> {
        if is_http(remote_path) || is_ssh(remote_path) {
            let repo = Self::init(dir)?;
            repo.set_remote(remote_path)?;
            if is_http(remote_path) {
                repo.http_fetch(remote_path)?;
            } else {
                repo.ssh_fetch(remote_path)?;
            }
            let refs = repo.origin_refs()?;
            let (branch, hash) = refs.into_iter().next().context("remote has no branches")?;
            write_ref_file(&repo.dir, &format!("refs/heads/{branch}"), hash)?;
            write_ref_file(&repo.dir, &format!("refs/remotes/origin/{branch}"), hash)?;
            fs::write(repo.dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))?;
            let commit = repo.commits.read(hash)?;
            let snapshot = Snapshot::from_entries(commit.objects);
            repo.write_snapshot("index", &snapshot)?;
            repo.write_snapshot("worktree", &snapshot)?;
            return Ok(repo);
        }
        let remote = Self::open(remote_path)?;
        let branch = remote.current_branch()?;
        let repo = Self::init_with_source(dir, remote.source_url())?;
        for name in ["objects", "symbols", "commits"] {
            copy_tree(&remote.dir.join(name), &repo.dir.join(name))?;
        }
        let remote_head = read_ref_file(&remote.dir, &format!("refs/heads/{branch}"))?;
        write_ref_file(&repo.dir, &format!("refs/heads/{branch}"), remote_head)?;
        write_ref_file(
            &repo.dir,
            &format!("refs/remotes/origin/{branch}"),
            remote_head,
        )?;
        if branch != "main" {
            fs::write(repo.dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))?;
        }
        let commit = repo.commits.read(remote_head)?;
        let snapshot = Snapshot::from_entries(commit.objects);
        repo.write_snapshot("index", &snapshot)?;
        repo.write_snapshot("worktree", &snapshot)?;
        repo.set_remote(remote_path)?;
        Ok(repo)
    }

    fn http_push(&self, url: &str) -> Result<()> {
        let client = http_client();
        let base = url.trim_end_matches('/');
        for relative in record_files(&self.dir)? {
            let bytes = fs::read(self.dir.join(&relative))?;
            let response = client
                .put(format!("{base}/files/{relative}"))
                .body(bytes)
                .send()?;
            if !response.status().is_success() {
                bail!("push {relative}: HTTP {}", response.status());
            }
        }
        let refs = local_refs(&self.dir)?;
        let body = refs
            .iter()
            .map(|(branch, hash)| format!("{branch}\t{}", hash.to_hex()))
            .collect::<Vec<_>>()
            .join("\n");
        let response = client.put(format!("{base}/refs")).body(body).send()?;
        if !response.status().is_success() {
            bail!("push refs: HTTP {}", response.status());
        }
        Ok(())
    }

    fn http_fetch(&self, url: &str) -> Result<()> {
        let client = http_client();
        let base = url.trim_end_matches('/');
        let refs = self.remote_refs(url)?;
        for (branch, hash) in refs {
            write_ref_file(&self.dir, &format!("refs/remotes/origin/{branch}"), hash)?;
        }
        let listing = client.get(format!("{base}/files")).send()?;
        if !listing.status().is_success() {
            bail!("fetch files: HTTP {}", listing.status());
        }
        for relative in listing.text()?.lines() {
            if relative.trim().is_empty() || self.dir.join(relative).exists() {
                continue;
            }
            let response = client.get(format!("{base}/files/{relative}")).send()?;
            if !response.status().is_success() {
                bail!("fetch {relative}: HTTP {}", response.status());
            }
            let bytes = response.bytes()?;
            write_atomic(&self.dir.join(relative), &bytes, &self.tmp)?;
        }
        Ok(())
    }

    fn remote_refs(&self, url: &str) -> Result<Vec<(String, CommitHash)>> {
        let base = url.trim_end_matches('/');
        let response = http_client().get(format!("{base}/refs")).send()?;
        if !response.status().is_success() {
            bail!("fetch refs: HTTP {}", response.status());
        }
        let mut refs = Vec::new();
        for line in response.text()?.lines() {
            let Some((branch, hash)) = line.split_once('\t') else {
                continue;
            };
            refs.push((branch.to_owned(), CommitHash::from_hex(hash)?));
        }
        Ok(refs)
    }

    fn ssh_push(&self, url: &str) -> Result<()> {
        let (mut child, mut stdin, mut stdout) = ssh_connect(url)?;
        for relative in record_files(&self.dir)? {
            let bytes = fs::read(self.dir.join(&relative))?;
            writeln!(stdin, "PUT {relative} {}", bytes.len())?;
            stdin.write_all(&bytes)?;
            stdin.flush()?;
            let mut line = String::new();
            stdout.read_line(&mut line)?;
            if line.trim() != "OK" {
                bail!("ssh push {relative} failed");
            }
        }
        let refs = local_refs(&self.dir)?;
        writeln!(stdin, "REFSET")?;
        for (branch, hash) in refs {
            writeln!(stdin, "{branch}\t{}", hash.to_hex())?;
        }
        writeln!(stdin, "END")?;
        stdin.flush()?;
        let mut line = String::new();
        stdout.read_line(&mut line)?;
        drop(stdin);
        let _ = child.wait();
        Ok(())
    }

    fn ssh_fetch(&self, url: &str) -> Result<()> {
        let (mut child, mut stdin, mut stdout) = ssh_connect(url)?;
        writeln!(stdin, "REFS")?;
        stdin.flush()?;
        loop {
            let mut line = String::new();
            stdout.read_line(&mut line)?;
            let line = line.trim_end();
            if line == "END" {
                break;
            }
            if let Some((branch, hash)) = line.split_once('\t') {
                write_ref_file(
                    &self.dir,
                    &format!("refs/remotes/origin/{branch}"),
                    CommitHash::from_hex(hash)?,
                )?;
            }
        }
        writeln!(stdin, "LIST")?;
        stdin.flush()?;
        loop {
            let mut line = String::new();
            stdout.read_line(&mut line)?;
            let relative = line.trim_end();
            if relative == "END" {
                break;
            }
            if self.dir.join(relative).exists() {
                continue;
            }
            writeln!(stdin, "GET {relative}")?;
            stdin.flush()?;
            let mut header = String::new();
            stdout.read_line(&mut header)?;
            let len = header
                .trim()
                .strip_prefix("OK ")
                .with_context(|| format!("ssh fetch {relative}: {header}"))?;
            let len: usize = len.parse()?;
            let mut bytes = vec![0u8; len];
            stdout.read_exact(&mut bytes)?;
            write_atomic(&self.dir.join(relative), &bytes, &self.tmp)?;
        }
        drop(stdin);
        let _ = child.wait();
        Ok(())
    }

    fn origin_refs(&self) -> Result<Vec<(String, CommitHash)>> {
        local_refs_under(&self.dir, "refs/remotes/origin")
    }

    fn source_cache(&self) -> PathBuf {
        self.dir.join("source")
    }

    fn ensure_source_exclude(&self) -> Result<()> {
        let info = self.source_cache().join("info");
        fs::create_dir_all(&info)?;
        let path = info.join("exclude");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        if !text.lines().any(|line| line.trim() == ".arche/") {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(".arche/\n");
        }
        fs::write(path, text)?;
        Ok(())
    }

    fn source_git_text<const N: usize>(&self, cwd: &Path, args: [&str; N]) -> Result<String> {
        let mut command = Command::new("git");
        command.arg("-C").arg(cwd).args(args);
        let output = command.output().context("run git")?;
        if !output.status.success() {
            bail!(
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("git output is not UTF-8")
    }

    fn source_git_bytes<const N: usize>(&self, cwd: &Path, args: [&str; N]) -> Result<Vec<u8>> {
        let mut command = Command::new("git");
        command.arg("-C").arg(cwd).args(args);
        let output = command.output().context("run git")?;
        if !output.status.success() {
            bail!(
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output.stdout)
    }

    pub fn create_technical_system(&self, name: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let technical_name = self.intern("technical")?;
        let qname_text = format!("technical/{name}");
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let name_hash = self.intern(name)?;
        let system = Object::TechnicalSystem(TechnicalSystem {
            qname,
            name: name_hash,
            parent: technical_name,
            members: None,
            modules: None,
            layers: None,
            position: None,
            responsibilities: None,
            non_responsibilities: None,
            features: None,
            non_features: None,
            invariants: None,
            constraints: None,
            design_principles: None,
            external_dependencies: None,
            usage: None,
            design_points: None,
        });
        let system_hash = self.objects.write(&system)?;

        let technical_hash = snapshot
            .get(technical_name)
            .context("technical root not found")?;
        let technical = match self.read(technical_hash)? {
            Object::Technical(value) => value,
            other => bail!("technical root is {}", other.kind().name()),
        };
        let systems_qname = technical
            .systems
            .context("technical root has no systems list")?;
        let systems_hash = snapshot
            .get(systems_qname)
            .context("technical systems list not found")?;
        let mut systems = match self.read(systems_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("technical systems is {}", other.kind().name()),
        };
        if !systems.items.contains(&qname) {
            systems.items.push(qname);
        }
        let systems_object = Object::ObjectList(systems);
        let systems_hash = self.objects.write(&systems_object)?;

        let technical_object = Object::Technical(Technical {
            systems: Some(systems_qname),
            ..technical
        });
        let technical_hash = self.objects.write(&technical_object)?;

        snapshot.insert(qname, system_hash);
        snapshot.insert(systems_qname, systems_hash);
        snapshot.insert(technical_name, technical_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, system_hash))
    }

    pub fn create_technical_module(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let parent_symbol = self.intern(parent_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let parent_hash = snapshot
            .get(parent_symbol)
            .with_context(|| format!("{parent_qname} not found"))?;
        let parent_object = self.read(parent_hash)?;
        let qname_text = format!("{parent_qname}/{name}");
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let name_hash = self.intern(name)?;
        let module = Object::TechnicalModule(crate::object::TechnicalModule {
            qname,
            name: name_hash,
            parent: parent_symbol,
            members: None,
            modules: None,
            layers: None,
            position: None,
            responsibilities: None,
            non_responsibilities: None,
            features: None,
            non_features: None,
            invariants: None,
            constraints: None,
            design_principles: None,
            external_dependencies: None,
            usage: None,
            design_points: None,
        });
        let module_hash = self.objects.write(&module)?;

        let existing_modules = match &parent_object {
            Object::TechnicalSystem(value) => value.modules,
            Object::TechnicalModule(value) => value.modules,
            other => bail!("{} cannot contain a module", other.kind().name()),
        };
        let (modules_qname, modules_hash) = match existing_modules {
            Some(modules_qname) => {
                let hash = snapshot
                    .get(modules_qname)
                    .context("module list not found")?;
                let mut list = match self.read(hash)? {
                    Object::ObjectList(value) => value,
                    other => bail!("module list is {}", other.kind().name()),
                };
                if !list.items.contains(&qname) {
                    list.items.push(qname);
                }
                let object = Object::ObjectList(list);
                let hash = self.objects.write(&object)?;
                (modules_qname, hash)
            }
            None => {
                let text = format!("{parent_qname}/modules");
                let modules_qname = self.intern(&text)?;
                let list = ObjectList {
                    qname: modules_qname,
                    owner: parent_symbol,
                    next_id: 1,
                    items: vec![qname],
                };
                let object = Object::ObjectList(list);
                let hash = self.objects.write(&object)?;
                (modules_qname, hash)
            }
        };

        let updated_parent = match parent_object {
            Object::TechnicalSystem(mut value) => {
                value.modules = Some(modules_qname);
                Object::TechnicalSystem(value)
            }
            Object::TechnicalModule(mut value) => {
                value.modules = Some(modules_qname);
                Object::TechnicalModule(value)
            }
            _ => unreachable!(),
        };
        let updated_parent_hash = self.objects.write(&updated_parent)?;

        snapshot.insert(qname, module_hash);
        snapshot.insert(modules_qname, modules_hash);
        snapshot.insert(parent_symbol, updated_parent_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, module_hash))
    }

    pub fn set_document_text(
        &self,
        owner_qname: &str,
        field: &str,
        text: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, _, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::Text])?;
        let document = Object::DocumentText(DocumentText {
            qname: field_qname,
            content: vec![Inline::Text(text.to_owned())],
        });
        let document_hash = self.objects.write(&document)?;
        snapshot.insert(field_qname, document_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((field_qname, document_hash))
    }

    pub fn doc_list_add(&self, owner_qname: &str, field: &str) -> Result<(SymbolHash, ObjectHash)> {
        let (snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::List])?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok((field_qname, field_hash))
    }

    pub fn doc_list_item_add(
        &self,
        owner_qname: &str,
        field: &str,
        text: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::List])?;
        let mut list = match self.read(field_hash)? {
            Object::DocumentList(value) => value,
            other => bail!("{} is {}", self.resolve(field_qname)?, other.kind().name()),
        };
        let id = list.next_id;
        list.next_id += 1;
        let item_qname = self.intern(&format!("{}.{id}", self.resolve(field_qname)?))?;
        let item = Object::DocumentText(DocumentText {
            qname: item_qname,
            content: vec![Inline::Text(text.to_owned())],
        });
        let item_hash = self.objects.write(&item)?;
        list.items.push(Paragraph::Text(item_qname));
        let list_hash = self.objects.write(&Object::DocumentList(list))?;
        snapshot.insert(item_qname, item_hash);
        snapshot.insert(field_qname, list_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((item_qname, item_hash))
    }

    pub fn doc_topic_add(
        &self,
        owner_qname: &str,
        field: &str,
        title: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::ParagraphList])?;
        let mut list = match self.read(field_hash)? {
            Object::DocumentParagraphList(value) => value,
            other => bail!("{} is {}", self.resolve(field_qname)?, other.kind().name()),
        };
        let id = list.next_id;
        list.next_id += 1;
        let topic_qname = self.intern(&format!("{}.{id}", self.resolve(field_qname)?))?;
        let body_qname = self.intern(&format!("{}/body", self.resolve(topic_qname)?))?;
        let body = Object::DocumentParagraphList(DocumentParagraphList {
            qname: body_qname,
            next_id: 1,
            items: Vec::new(),
        });
        let body_hash = self.objects.write(&body)?;
        let topic = Object::DocumentTopic(DocumentTopic {
            qname: topic_qname,
            title: vec![Inline::Text(title.to_owned())],
            body: body_qname,
        });
        let topic_hash = self.objects.write(&topic)?;
        list.items.push(Paragraph::Topic(topic_qname));
        let list_hash = self.objects.write(&Object::DocumentParagraphList(list))?;
        snapshot.insert(body_qname, body_hash);
        snapshot.insert(topic_qname, topic_hash);
        snapshot.insert(field_qname, list_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((topic_qname, topic_hash))
    }

    pub fn doc_mermaid_set(
        &self,
        owner_qname: &str,
        field: &str,
        source: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, _, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::Mermaid])?;
        let document = Object::DocumentMermaid(DocumentMermaid {
            qname: field_qname,
            source: source.to_owned(),
        });
        let hash = self.objects.write(&document)?;
        snapshot.insert(field_qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((field_qname, hash))
    }

    pub fn doc_html_add(
        &self,
        owner_qname: &str,
        field: &str,
        source: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::ParagraphList])?;
        let mut list = match self.read(field_hash)? {
            Object::DocumentParagraphList(value) => value,
            other => bail!("{} is {}", self.resolve(field_qname)?, other.kind().name()),
        };
        let id = list.next_id;
        list.next_id += 1;
        let html_qname = self.intern(&format!("{}.{id}", self.resolve(field_qname)?))?;
        let html = Object::DocumentHtml(DocumentHtml {
            qname: html_qname,
            source: source.to_owned(),
        });
        let html_hash = self.objects.write(&html)?;
        list.items.push(Paragraph::Html(html_qname));
        let list_hash = self.objects.write(&Object::DocumentParagraphList(list))?;
        snapshot.insert(html_qname, html_hash);
        snapshot.insert(field_qname, list_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((html_qname, html_hash))
    }

    fn ensure_doc_container(
        &self,
        owner_qname: &str,
        field: &str,
        allowed: &[DocFieldKind],
    ) -> Result<(Snapshot, SymbolHash, ObjectHash, DocFieldKind)> {
        let owner_symbol = self.intern(owner_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let owner_hash = snapshot
            .get(owner_symbol)
            .with_context(|| format!("{owner_qname} not found"))?;
        let mut owner = self.read(owner_hash)?;
        let kind = doc_field_kind(&owner, field)?;
        if !allowed.contains(&kind) {
            bail!("{field} on {owner_qname} is a {kind:?} field");
        }
        let field_qname_text = format!("{owner_qname}/{field}");
        let field_qname = self.intern(&field_qname_text)?;
        let field_hash = match snapshot.get(field_qname) {
            Some(hash) => hash,
            None => {
                let object = match kind {
                    DocFieldKind::Text => Object::DocumentText(DocumentText {
                        qname: field_qname,
                        content: Vec::new(),
                    }),
                    DocFieldKind::List => Object::DocumentList(DocumentList {
                        qname: field_qname,
                        next_id: 1,
                        ordered: false,
                        items: Vec::new(),
                    }),
                    DocFieldKind::ParagraphList => {
                        Object::DocumentParagraphList(DocumentParagraphList {
                            qname: field_qname,
                            next_id: 1,
                            items: Vec::new(),
                        })
                    }
                    DocFieldKind::Mermaid => Object::DocumentMermaid(DocumentMermaid {
                        qname: field_qname,
                        source: String::new(),
                    }),
                };
                let hash = self.objects.write(&object)?;
                snapshot.insert(field_qname, hash);
                hash
            }
        };
        set_doc_field(&mut owner, field, field_qname)?;
        let owner_hash = self.objects.write(&owner)?;
        snapshot.insert(owner_symbol, owner_hash);
        Ok((snapshot, field_qname, field_hash, kind))
    }

    pub fn comment_add(&self, target_qname: &str, text: &str) -> Result<(SymbolHash, ObjectHash)> {
        let target = self.intern(target_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let target_hash = snapshot
            .get(target)
            .with_context(|| format!("{target_qname} not found"))?;
        let comments_qname = self.intern("comments")?;
        let comments_hash = snapshot
            .get(comments_qname)
            .context("comments list not found")?;
        let mut comments = match self.read(comments_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("comments list is {}", other.kind().name()),
        };
        let id = comments.next_id;
        comments.next_id += 1;
        let qname = self.intern(&format!("comments.{id}"))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        let comment = Object::Comment(crate::object::Comment {
            qname,
            target,
            target_hash: Some(target_hash.0),
            body: vec![crate::object::Inline::Text(text.to_owned())],
            reply_to: None,
            resolved: false,
            author: "arche".to_owned(),
            created_at: now,
            updated_at: now,
        });
        let comment_hash = self.objects.write(&comment)?;
        if !comments.items.contains(&qname) {
            comments.items.push(qname);
        }
        let comments_object = Object::ObjectList(comments);
        let comments_hash = self.objects.write(&comments_object)?;
        snapshot.insert(qname, comment_hash);
        snapshot.insert(comments_qname, comments_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, comment_hash))
    }

    pub fn comment_list(&self, target: Option<&str>) -> Result<Vec<crate::object::Comment>> {
        let target = target.map(SymbolHash::of);
        let snapshot = self.worktree_snapshot()?;
        let comments_hash = snapshot
            .get(SymbolHash::of("comments"))
            .context("comments list not found")?;
        let comments = match self.read(comments_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("comments list is {}", other.kind().name()),
        };
        let mut result = Vec::new();
        for qname in comments.items {
            match self.read(
                snapshot
                    .get(qname)
                    .with_context(|| format!("comment {} not found", qname.to_hex()))?,
            )? {
                Object::Comment(comment) => {
                    if target.is_none_or(|target| comment.target == target) {
                        result.push(comment);
                    }
                }
                other => bail!("comment target is {}", other.kind().name()),
            }
        }
        Ok(result)
    }

    pub fn comment_resolve(&self, qname: &str) -> Result<ObjectHash> {
        let qname = self.intern(qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let hash = snapshot
            .get(qname)
            .with_context(|| format!("{} not found", qname.to_hex()))?;
        let mut comment = match self.read(hash)? {
            Object::Comment(value) => value,
            other => bail!("{} is {}", self.resolve(qname)?, other.kind().name()),
        };
        comment.resolved = true;
        comment.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        let object = Object::Comment(comment);
        let hash = self.objects.write(&object)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(hash)
    }

    pub fn concept_mkdir(&self, qname_text: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_concept_path(qname_text, true)?;
        let qname = self.intern(qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let (parent_text, local) = qname_text
            .rsplit_once('/')
            .with_context(|| format!("concept directory {qname_text} needs a parent"))?;
        let parent = self.intern(parent_text)?;
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_text} not found"))?;
        let directory = match self.read(parent_hash)? {
            Object::ConceptDirectory(value) => value,
            other => bail!("{parent_text} is {}", other.kind().name()),
        };
        let children_qname = directory
            .children
            .context("concept directory has no children list")?;
        let children_hash = snapshot
            .get(children_qname)
            .context("concept children list not found")?;
        let mut children = match self.read(children_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("concept children is {}", other.kind().name()),
        };
        let (new_children_qname, new_children_hash) =
            self.create_list(&format!("{qname_text}/children"), qname)?;
        let object = Object::ConceptDirectory(ConceptDirectory {
            qname,
            name: self.intern(local)?,
            parent: Some(parent),
            children: Some(new_children_qname),
        });
        let hash = self.objects.write(&object)?;
        children.items.push(qname);
        let children_object = Object::ObjectList(children);
        let children_hash = self.objects.write(&children_object)?;
        snapshot.insert(qname, hash);
        snapshot.insert(new_children_qname, new_children_hash);
        snapshot.insert(children_qname, children_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn concept_create(
        &self,
        qname_text: &str,
        name: &str,
        summary: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_concept_path(qname_text, false)?;
        let qname = self.intern(qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let (parent_text, _) = qname_text
            .rsplit_once('/')
            .with_context(|| format!("concept {qname_text} needs a parent"))?;
        let parent = self.intern(parent_text)?;
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_text} not found"))?;
        let directory = match self.read(parent_hash)? {
            Object::ConceptDirectory(value) => value,
            other => bail!("{parent_text} is {}", other.kind().name()),
        };
        let children_qname = directory
            .children
            .context("concept directory has no children list")?;
        let children_hash = snapshot
            .get(children_qname)
            .context("concept children list not found")?;
        let mut children = match self.read(children_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("concept children is {}", other.kind().name()),
        };
        let summary_qname = self.intern(&format!("{qname_text}/summary"))?;
        let summary_object = Object::DocumentText(crate::object::DocumentText {
            qname: summary_qname,
            content: vec![crate::object::Inline::Text(summary.to_owned())],
        });
        let summary_hash = self.objects.write(&summary_object)?;
        let concept = Object::Concept(Concept {
            qname,
            name: self.intern(name)?,
            summary: Some(summary_qname),
            explanation: None,
            characteristics: None,
            relations: None,
            aliases: None,
            links: Vec::new(),
        });
        let concept_hash = self.objects.write(&concept)?;
        children.items.push(qname);
        let children_object = Object::ObjectList(children);
        let children_hash = self.objects.write(&children_object)?;
        snapshot.insert(summary_qname, summary_hash);
        snapshot.insert(qname, concept_hash);
        snapshot.insert(children_qname, children_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, concept_hash))
    }

    pub fn concept_list(&self) -> Result<Vec<(SymbolHash, Concept)>> {
        let snapshot = self.worktree_snapshot()?;
        let mut concepts = Vec::new();
        for (qname, hash) in snapshot.entries() {
            if let Object::Concept(value) = self.read(*hash)? {
                concepts.push((*qname, value));
            }
        }
        concepts.sort_by_key(|(_, value)| value.name);
        Ok(concepts)
    }

    pub fn concept_link_add(
        &self,
        qname: &str,
        target: &str,
        relation: crate::object::ConceptRelation,
    ) -> Result<ObjectHash> {
        let qname_hash = self.intern(qname)?;
        let target_hash = self.intern(target)?;
        let mut snapshot = self.worktree_snapshot()?;
        let source_hash = snapshot
            .get(qname_hash)
            .with_context(|| format!("{qname} not found"))?;
        if snapshot.get(target_hash).is_none() {
            bail!("{target} not found");
        }
        let mut concept = match self.read(source_hash)? {
            Object::Concept(value) => value,
            other => bail!("{qname} is {}", other.kind().name()),
        };
        if !concept
            .links
            .iter()
            .any(|link| link.target == target_hash && link.relation == relation)
        {
            concept.links.push(crate::object::ConceptLink {
                target: target_hash,
                relation,
            });
        }
        let hash = self.objects.write(&Object::Concept(concept))?;
        snapshot.insert(qname_hash, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(hash)
    }

    pub fn concept_link_remove(&self, qname: &str, target: &str) -> Result<ObjectHash> {
        let qname_hash = self.intern(qname)?;
        let target_hash = self.intern(target)?;
        let mut snapshot = self.worktree_snapshot()?;
        let source_hash = snapshot
            .get(qname_hash)
            .with_context(|| format!("{qname} not found"))?;
        let mut concept = match self.read(source_hash)? {
            Object::Concept(value) => value,
            other => bail!("{qname} is {}", other.kind().name()),
        };
        concept.links.retain(|link| link.target != target_hash);
        let hash = self.objects.write(&Object::Concept(concept))?;
        snapshot.insert(qname_hash, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(hash)
    }

    pub fn create_requirement_user(&self, name: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern("requirements")?;
        let qname_text = format!("requirements/users/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let object = Object::RequirementUser(RequirementUser {
            qname,
            name: self.intern(name)?,
            age: None,
            basic_situation: None,
            hobbies: None,
            occupation: None,
            typical_scenario: None,
            needs: None,
            priority: None,
        });
        let hash = self.objects.write(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Users, qname)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_story(&self, name: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern("requirements")?;
        let qname_text = format!("requirements/stories/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let object = Object::RequirementStory(RequirementStory {
            qname,
            name: self.intern(name)?,
            preconditions: None,
            frequency: None,
            description: None,
            related_requirements: None,
        });
        let hash = self.objects.write(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Stories, qname)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_system(&self, name: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern("requirements")?;
        let qname_text = format!("requirements/systems/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let object = Object::RequirementSystem(RequirementSystem {
            qname,
            name: self.intern(name)?,
            parent,
            modules: None,
            form: None,
            covered_users: None,
            closed_loop_goal: None,
            relationships: None,
            boundary: None,
        });
        let hash = self.objects.write(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Systems, qname)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_module(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern(parent_qname)?;
        let qname_text = format!("{parent_qname}/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        if !matches!(self.read(parent_hash)?, Object::RequirementSystem(_)) {
            bail!("{parent_qname} is not a RequirementSystem");
        }
        let object = Object::RequirementModule(RequirementModule {
            qname,
            name: self.intern(name)?,
            parent,
            functions: None,
            core_concept: None,
            included_concepts: None,
            concept_relations: None,
            boundary: None,
        });
        let hash = self.objects.write(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Modules, qname)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_function(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern(parent_qname)?;
        let qname_text = format!("{parent_qname}/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        if !matches!(self.read(parent_hash)?, Object::RequirementModule(_)) {
            bail!("{parent_qname} is not a RequirementModule");
        }
        let object = Object::RequirementFunction(RequirementFunction {
            qname,
            name: self.intern(name)?,
            parent,
            flow: None,
            interactions: None,
            initiator: None,
            system_change: None,
            preconditions: None,
            feedback: None,
            description: None,
        });
        let hash = self.objects.write(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Functions, qname)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_interaction(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern(parent_qname)?;
        let qname_text = format!("{parent_qname}/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        if !matches!(self.read(parent_hash)?, Object::RequirementFunction(_)) {
            bail!("{parent_qname} is not a RequirementFunction");
        }
        let object = Object::RequirementInteraction(RequirementInteraction {
            qname,
            name: self.intern(name)?,
            parent,
            description: None,
            ui: None,
        });
        let hash = self.objects.write(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Interactions, qname)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    fn append_child(
        &self,
        snapshot: &mut Snapshot,
        parent: SymbolHash,
        field: ListField,
        child: SymbolHash,
    ) -> Result<()> {
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("parent {} not found", parent.to_hex()))?;
        let mut parent_object = self.read(parent_hash)?;
        let list_qname = match field.get(&parent_object)? {
            Some(list_qname) => list_qname,
            None => {
                let parent_text = self.resolve(parent)?;
                let list_qname = self.intern(&format!("{parent_text}/{}", field.name()))?;
                let list = ObjectList {
                    qname: list_qname,
                    owner: parent,
                    next_id: 1,
                    items: Vec::new(),
                };
                let list_hash = self.objects.write(&Object::ObjectList(list))?;
                field.set(&mut parent_object, list_qname)?;
                let parent_hash = self.objects.write(&parent_object)?;
                snapshot.insert(parent, parent_hash);
                snapshot.insert(list_qname, list_hash);
                list_qname
            }
        };
        let list_hash = snapshot
            .get(list_qname)
            .with_context(|| format!("list {} not found", list_qname.to_hex()))?;
        let mut list = match self.read(list_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("{} is {}", self.resolve(list_qname)?, other.kind().name()),
        };
        if !list.items.contains(&child) {
            list.items.push(child);
        }
        let list_hash = self.objects.write(&Object::ObjectList(list))?;
        snapshot.insert(list_qname, list_hash);
        Ok(())
    }

    pub fn search(
        &self,
        query: Option<&str>,
        kinds: &[String],
        has_design: bool,
    ) -> Result<Vec<(ObjectHash, Object)>> {
        let normalized_kinds = kinds
            .iter()
            .map(|kind| kind.to_lowercase().replace('-', ""))
            .collect::<Vec<_>>();
        let snapshot = self.worktree_snapshot()?;
        let mut result = Vec::new();
        for (qname, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            let kind = object.kind().name().to_lowercase();
            if !normalized_kinds.is_empty() && !normalized_kinds.contains(&kind) {
                continue;
            }
            if has_design && !doc_fields(&object).iter().any(|(_, value)| value.is_some()) {
                continue;
            }
            if let Some(query) = query {
                let qname_text = self.resolve(*qname)?;
                let name_text = object
                    .name()
                    .map(|name| self.resolve(name))
                    .transpose()?
                    .unwrap_or_default();
                if !qname_text.contains(query) && !name_text.contains(query) {
                    continue;
                }
            }
            result.push((*hash, object));
        }
        Ok(result)
    }

    #[allow(clippy::collapsible_if)]
    pub fn check(&self) -> Result<Vec<String>> {
        let snapshot = self.worktree_snapshot()?;
        let mut errors = Vec::new();
        for (_, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            let qname = self.resolve(object.qname())?;
            for (field, value) in doc_fields(&object) {
                let Some(value) = value else { continue };
                let Some(field_hash) = snapshot.get(value) else {
                    errors.push(format!("{qname}: field {field} target is missing"));
                    continue;
                };
                let field_object = self.read(field_hash)?;
                let kind = doc_field_kind(&object, field)?;
                let expected = expected_doc_kind(kind);
                if field_object.kind() != expected {
                    errors.push(format!(
                        "{qname}: field {field} must be {}, found {}",
                        expected.name(),
                        field_object.kind().name()
                    ));
                }
            }
            match &object {
                Object::ObjectList(value) => {
                    for item in &value.items {
                        if snapshot.get(*item).is_none() {
                            errors.push(format!(
                                "{qname}: list item {} is missing",
                                self.resolve(*item)?
                            ));
                        }
                    }
                }
                Object::DocumentList(value) => {
                    for item in &value.items {
                        let item_qname = paragraph_qname(*item);
                        if snapshot.get(item_qname).is_none() {
                            errors.push(format!(
                                "{qname}: item {} is missing",
                                self.resolve(item_qname)?
                            ));
                        }
                    }
                }
                Object::DocumentParagraphList(value) => {
                    for item in &value.items {
                        let item_qname = paragraph_qname(*item);
                        if snapshot.get(item_qname).is_none() {
                            errors.push(format!(
                                "{qname}: item {} is missing",
                                self.resolve(item_qname)?
                            ));
                        }
                    }
                }
                Object::DocumentTopic(value) => {
                    if snapshot.get(value.body).is_none() {
                        errors.push(format!("{qname}: topic body is missing"));
                    }
                }
                Object::RequirementInteraction(value) => {
                    if let Some(ui) = value.ui {
                        if let Some(ui_hash) = snapshot.get(ui) {
                            if let Object::DocumentParagraphList(list) = self.read(ui_hash)? {
                                for item in &list.items {
                                    if !matches!(item, Paragraph::Html(_)) {
                                        errors
                                            .push(format!("{qname}: ui item must be DocumentHtml"));
                                    }
                                }
                            }
                        }
                    }
                }
                Object::RequirementFunction(value) => {
                    if let Some(interactions) = value.interactions {
                        if let Some(hash) = snapshot.get(interactions) {
                            if let Object::ObjectList(list) = self.read(hash)? {
                                for item in list.items {
                                    if let Some(hash) = snapshot.get(item) {
                                        if !matches!(
                                            self.read(hash)?,
                                            Object::RequirementInteraction(_)
                                        ) {
                                            errors.push(format!(
                                                "{qname}: interaction {} has wrong kind",
                                                self.resolve(item)?
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Object::RequirementSystem(value) => {
                    if let Some(modules) = value.modules {
                        if let Some(hash) = snapshot.get(modules) {
                            if let Object::ObjectList(list) = self.read(hash)? {
                                for item in list.items {
                                    if let Some(hash) = snapshot.get(item) {
                                        if !matches!(self.read(hash)?, Object::RequirementModule(_))
                                        {
                                            errors.push(format!(
                                                "{qname}: module {} has wrong kind",
                                                self.resolve(item)?
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Object::RequirementModule(value) => {
                    if let Some(functions) = value.functions {
                        if let Some(hash) = snapshot.get(functions) {
                            if let Object::ObjectList(list) = self.read(hash)? {
                                for item in list.items {
                                    if let Some(hash) = snapshot.get(item) {
                                        if !matches!(
                                            self.read(hash)?,
                                            Object::RequirementFunction(_)
                                        ) {
                                            errors.push(format!(
                                                "{qname}: function {} has wrong kind",
                                                self.resolve(item)?
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Object::Concept(value) => {
                    for link in &value.links {
                        if snapshot.get(link.target).is_none() {
                            errors.push(format!(
                                "{qname}: concept link {} is missing",
                                self.resolve(link.target)?
                            ));
                        }
                    }
                    let source = self.resolve(object.qname())?;
                    for link in &value.links {
                        let target = self.resolve(link.target)?;
                        let valid = if source.starts_with("concepts/requirements/")
                            && target.starts_with("concepts/domain/")
                        {
                            link.relation == crate::object::ConceptRelation::Refines
                        } else if source.starts_with("concepts/technical/")
                            && target.starts_with("concepts/requirements/")
                        {
                            link.relation == crate::object::ConceptRelation::Realizes
                        } else {
                            true
                        };
                        if !valid {
                            errors.push(format!("{qname}: invalid concept relation to {target}"));
                        }
                    }
                }
                Object::Comment(value) => {
                    if snapshot.get(value.target).is_none() {
                        errors.push(format!(
                            "{qname}: comment target {} is missing",
                            self.resolve(value.target)?
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(errors)
    }

    pub fn show_qname(&self, qname: &str) -> Result<(ObjectHash, Object)> {
        let hash = self.find_by_qname(qname)?;
        let object = self.read(hash)?;
        Ok((hash, object))
    }

    fn bootstrap(&self) -> Result<()> {
        let mut config = String::from("format = 2\n");
        if let Some(source_url) = &self.source_url {
            config.push_str(&format!("source={source_url}\n"));
        }
        if let Some(remote_url) = &self.remote_url {
            config.push_str(&format!("remote={remote_url}\n"));
        }
        fs::write(self.dir.join("config"), config).with_context(|| "write Arche config")?;
        fs::write(self.dir.join("HEAD"), format!("ref: {MAIN_REF}\n"))
            .with_context(|| "write HEAD")?;

        let technical_name = self.intern("technical")?;
        let requirements_name = self.intern("requirements")?;
        let concepts_name = self.intern("concepts")?;
        let view_name = self.intern("view")?;

        let (technical_systems, _) = self.create_list("technical/systems", technical_name)?;
        let (technical_files, _) = self.create_list("technical/files", technical_name)?;
        let (technical_namespaces, _) = self.create_list("technical/namespaces", technical_name)?;
        let technical = Object::Technical(Technical {
            qname: technical_name,
            name: technical_name,
            files: Some(technical_files),
            namespaces: Some(technical_namespaces),
            systems: Some(technical_systems),
        });
        let technical_hash = self.objects.write(&technical)?;

        let (requirement_users, _) = self.create_list("requirements/users", requirements_name)?;
        let (requirement_stories, _) =
            self.create_list("requirements/stories", requirements_name)?;
        let (requirement_systems, _) =
            self.create_list("requirements/systems", requirements_name)?;
        let requirements = Object::Requirements(Requirements {
            qname: requirements_name,
            name: requirements_name,
            users: Some(requirement_users),
            stories: Some(requirement_stories),
            systems: Some(requirement_systems),
        });
        let requirements_hash = self.objects.write(&requirements)?;

        let concepts_domain = self.intern("concepts/domain")?;
        let concepts_requirements = self.intern("concepts/requirements")?;
        let concepts_technical = self.intern("concepts/technical")?;
        let domain_children = self.create_list("concepts/domain/children", concepts_domain)?;
        let domain = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_domain,
            name: self.intern("domain")?,
            parent: Some(concepts_name),
            children: Some(domain_children.0),
        });
        self.objects.write(&domain)?;
        let requirement_children =
            self.create_list("concepts/requirements/children", concepts_requirements)?;
        let requirement_concepts = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_requirements,
            name: self.intern("requirements")?,
            parent: Some(concepts_name),
            children: Some(requirement_children.0),
        });
        self.objects.write(&requirement_concepts)?;
        let technical_children =
            self.create_list("concepts/technical/children", concepts_technical)?;
        let technical_concepts = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_technical,
            name: self.intern("technical")?,
            parent: Some(concepts_name),
            children: Some(technical_children.0),
        });
        self.objects.write(&technical_concepts)?;
        let (concept_children, _) = self.create_list_with_items(
            "concepts/children",
            concepts_name,
            vec![concepts_domain, concepts_requirements, concepts_technical],
        )?;
        let concepts = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_name,
            name: concepts_name,
            parent: None,
            children: Some(concept_children),
        });
        let concepts_hash = self.objects.write(&concepts)?;

        let (_, comments) = self.create_list("comments", view_name)?;
        let view = Object::View(View {
            qname: view_name,
            source_commit: Hash32::ZERO,
            technical: technical_hash,
            requirements: requirements_hash,
            concepts: concepts_hash,
            comments: Some(comments),
        });
        let view_hash = self.objects.write(&view)?;
        let snapshot = self.snapshot_from_store()?;
        self.commit(None, view_hash, "init".to_owned(), &snapshot)?;
        self.write_snapshot("index", &snapshot)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    fn snapshot_from_store(&self) -> Result<Snapshot> {
        let mut snapshot = Snapshot::default();
        for (hash, object) in self.objects.all()? {
            snapshot.insert(object.qname(), hash);
        }
        Ok(snapshot)
    }

    fn create_list(
        &self,
        qname_text: &str,
        owner_text: SymbolHash,
    ) -> Result<(SymbolHash, ObjectHash)> {
        self.create_list_with_items(qname_text, owner_text, Vec::new())
    }

    fn create_list_with_items(
        &self,
        qname_text: &str,
        owner_text: SymbolHash,
        items: Vec<SymbolHash>,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let qname = self.intern(qname_text)?;
        let object = Object::ObjectList(ObjectList {
            qname,
            owner: owner_text,
            next_id: 1,
            items,
        });
        let hash = self.objects.write(&object)?;
        Ok((qname, hash))
    }

    pub fn add_all(&self) -> Result<()> {
        let snapshot = self.worktree_snapshot()?;
        self.write_snapshot("index", &snapshot)
    }

    pub fn add(&self, qnames: &[String]) -> Result<()> {
        if qnames.is_empty() {
            return self.add_all();
        }
        let worktree = self.worktree_snapshot()?;
        let mut index = self.index_snapshot()?;
        for qname in qnames {
            let qname = SymbolHash::of(qname);
            match worktree.get(qname) {
                Some(hash) => index.insert(qname, hash),
                None => {
                    index.remove(qname);
                }
            }
        }
        self.write_snapshot("index", &index)
    }

    pub fn unstage(&self, qnames: &[String]) -> Result<()> {
        let head = self.head_snapshot()?;
        if qnames.is_empty() {
            return self.write_snapshot("index", &head);
        }
        let mut index = self.index_snapshot()?;
        for qname in qnames {
            let qname = SymbolHash::of(qname);
            match head.get(qname) {
                Some(hash) => index.insert(qname, hash),
                None => {
                    index.remove(qname);
                }
            }
        }
        self.write_snapshot("index", &index)
    }

    pub fn reset(&self, hard: bool) -> Result<()> {
        let head = self.head_snapshot()?;
        self.write_snapshot("index", &head)?;
        if hard {
            self.write_snapshot("worktree", &head)?;
        }
        Ok(())
    }

    pub fn commit_index(&self, message: &str, allow_empty: bool) -> Result<CommitHash> {
        let head = self.head()?;
        let head_snapshot = Snapshot::from_entries(head.objects.clone());
        let mut index = self.index_snapshot()?;
        if index == head_snapshot && !allow_empty {
            bail!("nothing to commit");
        }
        let qname = SymbolHash::of;
        let technical = index
            .get(qname("technical"))
            .context("missing technical root")?;
        let requirements = index
            .get(qname("requirements"))
            .context("missing requirements root")?;
        let concepts = index
            .get(qname("concepts"))
            .context("missing concepts root")?;
        let comments = index.get(qname("comments"));
        let source_commit = match self.read(head.view)? {
            Object::View(value) => value.source_commit,
            _ => Hash32::ZERO,
        };
        let view = Object::View(View {
            qname: qname("view"),
            source_commit,
            technical,
            requirements,
            concepts,
            comments,
        });
        let view_hash = self.objects.write(&view)?;
        index.insert(qname("view"), view_hash);
        let commit = self.commit(
            Some(self.head_commit()?),
            view_hash,
            message.to_owned(),
            &index,
        )?;
        self.write_snapshot("index", &index)?;
        let mut worktree = self.worktree_snapshot()?;
        worktree.insert(qname("view"), view_hash);
        self.write_snapshot("worktree", &worktree)?;
        Ok(commit)
    }

    fn commit(
        &self,
        parent: Option<CommitHash>,
        view: ObjectHash,
        message: String,
        objects: &Snapshot,
    ) -> Result<CommitHash> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        let commit = Commit {
            parent,
            view,
            message,
            author: "arche".to_owned(),
            timestamp,
            objects: objects.entries().to_vec(),
        };
        let hash = self.commits.write(&commit)?;
        let branch = self.current_branch()?;
        self.write_ref(&format!("refs/heads/{branch}"), hash)?;
        Ok(hash)
    }

    fn write_ref(&self, name: &str, hash: CommitHash) -> Result<()> {
        let path = self.dir.join(name);
        write_atomic(&path, format!("{}\n", hash.to_hex()).as_bytes(), &self.tmp)
    }
}

fn http_client() -> reqwest::blocking::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(token) = auth_token()
        && let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

fn auth_token() -> Option<String> {
    if let Ok(token) = std::env::var("ARCHE_TOKEN") {
        return Some(token);
    }
    let mut paths = vec![PathBuf::from("/run/foundry/arche_token")];
    if let Ok(config) = std::env::var("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(config).join("arche/token"));
    }
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(home).join(".config/arche/token"));
    }
    for path in paths {
        if let Ok(token) = fs::read_to_string(path) {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_owned());
            }
        }
    }
    None
}

fn is_http(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn is_ssh(url: &str) -> bool {
    url.starts_with("ssh://")
}

fn ssh_connect(
    url: &str,
) -> Result<(
    std::process::Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
)> {
    let (authority, port, path) = parse_ssh_url(url)?;
    let mut command = Command::new("ssh");
    if let Some(port) = port {
        command.args(["-p", &port]);
    }
    command
        .arg(authority)
        .arg("arche-server")
        .arg("--stdio")
        .arg("--root")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = command.spawn().context("spawn ssh")?;
    let stdin = child.stdin.take().context("ssh stdin")?;
    let stdout = child.stdout.take().context("ssh stdout")?;
    Ok((child, stdin, BufReader::new(stdout)))
}

fn parse_ssh_url(url: &str) -> Result<(String, Option<String>, String)> {
    let rest = url.strip_prefix("ssh://").context("invalid ssh url")?;
    let (authority, path) = rest.split_once('/').context("ssh url has no path")?;
    if path.is_empty() {
        bail!("ssh url has an empty path");
    }
    let (authority, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        (host.to_owned(), Some(port.to_owned()))
    } else {
        (authority.to_owned(), None)
    };
    Ok((authority, port, format!("/{path}")))
}

fn record_files(dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for name in ["objects", "symbols", "commits"] {
        for path in list_files(&dir.join(name))? {
            let relative = path
                .strip_prefix(dir)
                .context("record path is outside repository")?;
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    files.sort();
    Ok(files)
}

fn local_refs(dir: &Path) -> Result<Vec<(String, CommitHash)>> {
    local_refs_under(dir, "refs/heads")
}

fn local_refs_under(dir: &Path, relative: &str) -> Result<Vec<(String, CommitHash)>> {
    let mut refs = Vec::new();
    let root = dir.join(relative);
    if !root.is_dir() {
        return Ok(refs);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let branch = entry.file_name().to_string_lossy().to_string();
        let text = fs::read_to_string(entry.path())?;
        refs.push((branch, CommitHash::from_hex(text.trim())?));
    }
    refs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(refs)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source, &target)?;
        } else if !target.exists() {
            fs::copy(&source, &target)
                .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
        }
    }
    Ok(())
}

fn write_ref_file(base: &Path, name: &str, hash: CommitHash) -> Result<()> {
    let path = base.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", hash.to_hex()))?;
    Ok(())
}

fn read_ref_file(base: &Path, name: &str) -> Result<CommitHash> {
    let text = fs::read_to_string(base.join(name))?;
    CommitHash::from_hex(text.trim())
}

fn run_git(mut command: Command, label: &str) -> Result<()> {
    let output = command.output().with_context(|| format!("run {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn validate_concept_path(path: &str, _directory: bool) -> Result<()> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < 3 || segments[0] != "concepts" {
        bail!("concept path must be concepts/<domain|requirements|technical>/...");
    }
    if !matches!(segments[1], "domain" | "requirements" | "technical") {
        bail!(
            "concept path must be under concepts/domain, concepts/requirements, or concepts/technical"
        );
    }
    if segments.iter().any(|segment| segment.is_empty()) {
        bail!("concept path contains an empty segment");
    }
    Ok(())
}

fn validate_branch_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('-')
        || name.contains("..")
        || name.contains('/')
        || name.chars().any(char::is_whitespace)
    {
        bail!("invalid branch name {name}");
    }
    Ok(())
}

fn validate_local_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("name must not be empty");
    }
    if name.contains('/') || name.contains('.') || name.chars().any(char::is_whitespace) {
        bail!("name must not contain '/', '.', or whitespace");
    }
    Ok(())
}

fn validate_requirement_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("name must not be empty");
    }
    if name.contains('/') || name.contains('.') {
        bail!("name must not contain '/' or '.'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn init_creates_roots_and_commit() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let commit = repo.head().unwrap();
        assert_eq!(commit.message, "init");
        assert!(repo.find_by_qname("technical").is_ok());
        assert!(repo.find_by_qname("requirements").is_ok());
        assert!(repo.find_by_qname("concepts").is_ok());
    }

    #[test]
    fn create_system_updates_worktree_then_commits() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let (qname, _) = repo.create_technical_system("app").unwrap();
        assert_eq!(repo.resolve(qname).unwrap(), "technical/app");
        assert!(repo.find_by_qname("technical/app").is_ok());
        assert_eq!(repo.log().unwrap().len(), 1);

        repo.add_all().unwrap();
        repo.commit_index("add app", false).unwrap();
        let log = repo.log().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].1.message, "add app");
    }

    #[test]
    fn comments_do_not_change_target_hash() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.create_technical_system("app").unwrap();
        repo.add_all().unwrap();
        repo.commit_index("add app", false).unwrap();
        let before = repo.find_by_qname("technical/app").unwrap();
        let (comment, _) = repo.comment_add("technical/app", "补充错误码").unwrap();
        assert_eq!(repo.resolve(comment).unwrap(), "comments.1");
        assert_eq!(repo.find_by_qname("technical/app").unwrap(), before);
        let comments = repo.comment_list(Some("technical/app")).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body_text(), "补充错误码");
        repo.comment_resolve("comments.1").unwrap();
        assert!(repo.comment_list(None).unwrap()[0].resolved);
    }

    #[test]
    fn concepts_can_share_a_name_across_dimensions() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.concept_create("concepts/domain/agent", "智能体", "现实中的智能机器")
            .unwrap();
        repo.concept_create(
            "concepts/requirements/agent",
            "智能体",
            "用户感知到的智能虚拟对象",
        )
        .unwrap();
        repo.concept_create("concepts/technical/agent", "智能体", "程序中的内存对象")
            .unwrap();
        repo.concept_link_add(
            "concepts/requirements/agent",
            "concepts/domain/agent",
            crate::object::ConceptRelation::Refines,
        )
        .unwrap();
        repo.concept_link_add(
            "concepts/technical/agent",
            "concepts/requirements/agent",
            crate::object::ConceptRelation::Realizes,
        )
        .unwrap();
        let concepts = repo.concept_list().unwrap();
        assert_eq!(concepts.len(), 3);
        let names = concepts
            .iter()
            .map(|(_, concept)| repo.resolve(concept.name).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["智能体", "智能体", "智能体"]);
        assert!(repo.check().unwrap().is_empty());
    }

    #[test]
    fn requirement_hierarchy_is_explicit() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let (system, _) = repo.create_requirement_system("仓库系统").unwrap();
        let (module, _) = repo
            .create_requirement_module("requirements/systems/仓库系统", "订单管理")
            .unwrap();
        let (function, _) = repo
            .create_requirement_function("requirements/systems/仓库系统/订单管理", "创建订单")
            .unwrap();
        let (interaction, _) = repo
            .create_requirement_interaction(
                "requirements/systems/仓库系统/订单管理/创建订单",
                "填写订单",
            )
            .unwrap();
        assert_eq!(
            repo.resolve(system).unwrap(),
            "requirements/systems/仓库系统"
        );
        assert_eq!(
            repo.resolve(module).unwrap(),
            "requirements/systems/仓库系统/订单管理"
        );
        assert_eq!(
            repo.resolve(function).unwrap(),
            "requirements/systems/仓库系统/订单管理/创建订单"
        );
        assert_eq!(
            repo.resolve(interaction).unwrap(),
            "requirements/systems/仓库系统/订单管理/创建订单/填写订单"
        );
        assert!(
            repo.find_by_qname("requirements/systems/仓库系统/订单管理")
                .is_ok()
        );
    }

    #[test]
    fn document_fields_are_typed_and_authorable() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let system = "requirements/systems/仓库系统";
        let module = "requirements/systems/仓库系统/订单管理";
        let function = "requirements/systems/仓库系统/订单管理/创建订单";
        let interaction = "requirements/systems/仓库系统/订单管理/创建订单/填写订单";
        repo.create_requirement_system("仓库系统").unwrap();
        repo.create_requirement_module(system, "订单管理").unwrap();
        repo.create_requirement_function(module, "创建订单")
            .unwrap();
        repo.create_requirement_interaction(function, "填写订单")
            .unwrap();

        repo.doc_mermaid_set(function, "flow", "flowchart TD\nA-->B")
            .unwrap();
        repo.doc_html_add(interaction, "ui", "<div>UI</div>")
            .unwrap();
        repo.doc_list_add(system, "covered_users").unwrap();
        repo.doc_list_item_add(system, "covered_users", "仓库管理员")
            .unwrap();
        repo.doc_topic_add(function, "description", "创建订单")
            .unwrap();

        assert!(repo.find_by_qname(&format!("{function}/flow")).is_ok());
        assert!(repo.find_by_qname(&format!("{interaction}/ui.1")).is_ok());
        assert!(
            repo.find_by_qname(&format!("{system}/covered_users.1"))
                .is_ok()
        );
        assert!(
            repo.find_by_qname(&format!("{function}/description.1"))
                .is_ok()
        );
    }
}
