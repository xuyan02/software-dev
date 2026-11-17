//! command-line front end (`ds`).
//!
//! Source checkouts are read-only. Remote sync supports local directories,
//! HTTP endpoints served by `ds-server`, and SSH.

use crate::catalog::{Catalog, structural_dependencies};
use crate::format::{self, content_hash, symbol_hash};
use crate::model::*;
use crate::remote::Remote;
use crate::source::SourceRepo;
use crate::workspace::{ObjectFormat, State, Workspace};
use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "ds", version, about = "design-space CLI")]
struct Cli {
    /// Workspace root (default: nearest `.ds` from the current directory).
    #[arg(long, global = true)]
    dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Initialize a design workspace.
    Init {
        /// Source git URL recorded in .ds/config.
        #[arg(long)]
        source: String,
        /// Source repository object format.
        #[arg(long, value_enum, default_value_t = FormatArg::Sha1)]
        format: FormatArg,
    },
    /// Clone one branch from a design-space remote.
    Clone {
        /// Design-space remote URL.
        remote: String,
        /// Source git URL recorded in .ds/config.
        #[arg(long)]
        source: String,
        /// Remote branch to clone.
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// Create one design object.
    Create(CreateArgs),
    /// Edit one design object.
    Edit(EditArgs),
    /// Inspect or change explicit/implicit dependencies.
    Dep {
        #[command(subcommand)]
        command: DepCommand,
    },
    /// Manage the read-only source git clone.
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    /// Manage a remote.
    Remote {
        #[command(subcommand)]
        command: RemoteCommand,
    },
    /// Upload the current branch to the remote.
    Push {
        #[arg(long)]
        branch: Option<String>,
    },
    /// Download remote refs and objects.
    Fetch,
    /// Fetch and fast-forward one local branch.
    Pull {
        #[arg(long)]
        branch: Option<String>,
    },
    /// Stage object(s) from the worktree.
    Add {
        /// Qualified names to stage.
        names: Vec<String>,
        /// Stage every worktree object.
        #[arg(short = 'A', long = "all")]
        all: bool,
    },
    /// Unstage everything or selected objects; --hard discards the worktree.
    Reset {
        /// Objects whose index entries are restored from HEAD.
        names: Vec<String>,
        /// Reset both index and worktree to HEAD.
        #[arg(long, conflicts_with = "names")]
        hard: bool,
    },
    /// Show differences between worktree/index or index/HEAD.
    Diff {
        /// Compare index against HEAD instead of worktree against index.
        #[arg(long)]
        staged: bool,
    },
    /// Remove one worktree object.
    Rm {
        name: String,
        /// Remove incoming relations and dependencies before deleting.
        #[arg(long)]
        force: bool,
    },
    /// Switch to an existing branch.
    Checkout {
        branch: String,
        /// Create the branch at the current HEAD.
        #[arg(short = 'b', long = "create")]
        create: bool,
        /// Discard index/worktree changes.
        #[arg(long)]
        force: bool,
    },
    /// List design objects in the worktree.
    Ls {
        /// List directory and file objects.
        #[arg(long, conflicts_with_all = ["namespaces", "design"])]
        files: bool,
        /// List namespace and code objects.
        #[arg(long, conflicts_with_all = ["files", "design"])]
        namespaces: bool,
        /// List objects that have a design document.
        #[arg(long, conflicts_with_all = ["files", "namespaces"])]
        design: bool,
        /// Read the index instead of the worktree.
        #[arg(long)]
        staged: bool,
    },
    /// Show HEAD, index, and worktree roots plus changed objects.
    Status,
    /// Verify the worktree object graph.
    Verify,
    /// Commit the index.
    Commit {
        #[arg(short = 'm', long)]
        message: String,
        #[arg(long, default_value = "agent")]
        author: String,
        /// Allow a commit whose view equals its parent.
        #[arg(long)]
        allow_empty: bool,
    },
    /// Show one worktree object.
    Show {
        name: String,
        /// Print the design document too.
        #[arg(long)]
        source: bool,
        /// Read the index instead of the worktree.
        #[arg(long)]
        staged: bool,
    },
    /// Walk the commit chain.
    Log {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Remove unreachable objects, docs, and symbols.
    Gc,
    /// Pack all loose records into one pack.
    Repack,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FormatArg {
    Sha1,
    Sha256,
}

impl From<FormatArg> for ObjectFormat {
    fn from(value: FormatArg) -> Self {
        match value {
            FormatArg::Sha1 => Self::Sha1,
            FormatArg::Sha256 => Self::Sha256,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum KindArg {
    Directory,
    File,
    Namespace,
    Class,
    Interface,
    Method,
    Function,
    Part,
    Mixin,
}

#[derive(Debug, Clone, Args)]
struct CreateArgs {
    #[arg(value_enum)]
    kind: KindArg,
    /// Local name. Qualified name is generated from the parent/namespace.
    name: String,
    /// Parent for directory/file/namespace, the enclosing type for a nested
    /// class/interface/mixin, or the parent of a method/part. Top-level types
    /// use --namespace (plus optional --file/--module), not --parent.
    #[arg(long)]
    parent: Option<String>,
    /// Namespace parent for a top-level class/interface/function/mixin.
    #[arg(long)]
    namespace: Option<String>,
    /// File parent for a top-level code object.
    #[arg(long)]
    file: Option<String>,
    /// Module parent for a top-level code object.
    #[arg(long)]
    module: Option<String>,
    /// Source: `path` or `path:start-end` (1-based lines).
    #[arg(long)]
    source: Option<String>,
    /// Inline design text.
    #[arg(long, conflicts_with = "design_file")]
    design: Option<String>,
    /// Read design text from a file.
    #[arg(long, conflicts_with = "design")]
    design_file: Option<PathBuf>,
    /// Class/mixin semantic superclass.
    #[arg(long)]
    superclass: Option<String>,
    /// Class/mixin implemented interfaces.
    #[arg(long = "interface")]
    interfaces: Vec<String>,
    /// Class/mixin reused mixins.
    #[arg(long = "mixin")]
    mixins: Vec<String>,
    /// Mixin `on` constraints.
    #[arg(long = "on")]
    on: Vec<String>,
    /// Interface parent.
    #[arg(long)]
    extends: Option<String>,
    /// Method signature; required for kind=method.
    #[arg(long)]
    signature: Option<String>,
    /// Method being overridden.
    #[arg(long)]
    overrides: Option<String>,
}

#[derive(Debug, Clone, Args)]
struct EditArgs {
    name: String,
    /// Replace the design text.
    #[arg(long, conflicts_with_all = ["design_file", "clear_design"])]
    design: Option<String>,
    /// Read replacement design text from a file.
    #[arg(long, conflicts_with_all = ["design", "clear_design"])]
    design_file: Option<PathBuf>,
    /// Remove the design document reference.
    #[arg(long)]
    clear_design: bool,
    /// Replace the source anchor.
    #[arg(long)]
    source: Option<String>,
    /// Replace the semantic superclass.
    #[arg(long)]
    superclass: Option<String>,
    /// Add implemented interfaces.
    #[arg(long = "interface-add")]
    interface_add: Vec<String>,
    /// Remove implemented interfaces.
    #[arg(long = "interface-rm")]
    interface_rm: Vec<String>,
    /// Add reused mixins.
    #[arg(long = "mixin-add")]
    mixin_add: Vec<String>,
    /// Remove reused mixins.
    #[arg(long = "mixin-rm")]
    mixin_rm: Vec<String>,
    /// Add mixin `on` constraints.
    #[arg(long = "on-add")]
    on_add: Vec<String>,
    /// Remove mixin `on` constraints.
    #[arg(long = "on-rm")]
    on_rm: Vec<String>,
    /// Replace the interface parent.
    #[arg(long)]
    extends: Option<String>,
    /// Replace the overridden method.
    #[arg(long)]
    overrides: Option<String>,
    /// Change the containment parent.
    #[arg(long)]
    parent: Option<String>,
    /// Rename the object within its parent.
    #[arg(long = "name")]
    new_name: Option<String>,
    /// Change the namespace parent of a top-level type/function.
    #[arg(long)]
    namespace: Option<String>,
    /// Change the file parent of a top-level type/function.
    #[arg(long)]
    file: Option<String>,
    /// Change the module parent of a top-level type/function.
    #[arg(long)]
    module: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
enum DepCommand {
    /// Add one dependency.
    Add {
        src: String,
        dst: String,
        #[arg(long, conflicts_with = "implicit")]
        explicit: bool,
        #[arg(long, conflicts_with = "explicit")]
        implicit: bool,
        /// Evidence for an explicit dependency: path:start-end or path.
        #[arg(long)]
        evidence: Option<String>,
        /// Rationale document for an implicit dependency.
        #[arg(long)]
        rationale_file: Option<PathBuf>,
    },
    /// Remove one dependency.
    Rm {
        src: String,
        dst: String,
        #[arg(long, conflicts_with = "implicit")]
        explicit: bool,
        #[arg(long, conflicts_with = "explicit")]
        implicit: bool,
    },
    /// List dependencies of one object.
    List { name: String },
}

#[derive(Debug, Clone, Subcommand)]
enum RemoteCommand {
    /// Set the remote for this workspace.
    Add {
        url: String,
        /// Read the HTTP bearer token from this file.
        #[arg(long)]
        token_file: Option<PathBuf>,
    },
    /// Print the configured remote.
    Show,
    /// Replace the configured remote URL.
    SetUrl { url: String },
    /// Set the HTTP bearer token file; omit the path to clear it.
    SetToken { path: Option<PathBuf> },
}

#[derive(Debug, Clone, Subcommand)]
enum SourceCommand {
    /// Clone if missing, otherwise fetch branches and tags.
    Fetch,
    /// Switch the current source ref.
    Checkout { git_ref: String },
    /// Show the current source ref and commit.
    Status,
    /// List source blobs at the current source commit.
    Ls { path: Option<String> },
    /// Print one source file at the current source commit.
    Cat { path: String },
    /// Walk source commits.
    Log {
        git_ref: Option<String>,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Compare the source recorded in the latest design commit with the current source HEAD.
    Changes {
        /// Compare from this git ref instead of the latest design commit.
        #[arg(long)]
        base: Option<String>,
    },
    /// Add, update, delete and rename File/Directory objects to match the
    /// current source tree in one pass.
    Sync,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { source, format } => init(cli.dir, &source, format.into()),
        Command::Clone {
            remote,
            source,
            branch,
        } => clone(cli.dir, &remote, &source, &branch),
        Command::Create(args) => create(cli.dir, args),
        Command::Edit(args) => edit(cli.dir, args),
        Command::Dep { command } => dep(cli.dir, command),
        Command::Source { command } => source(cli.dir, command),
        Command::Remote { command } => remote(cli.dir, command),
        Command::Push { branch } => push(cli.dir, branch),
        Command::Fetch => fetch(cli.dir),
        Command::Pull { branch } => pull(cli.dir, branch),
        Command::Add { names, all } => add(cli.dir, names, all),
        Command::Reset { names, hard } => reset(cli.dir, names, hard),
        Command::Diff { staged } => diff(cli.dir, staged),
        Command::Rm { name, force } => rm(cli.dir, &name, force),
        Command::Checkout {
            branch,
            create,
            force,
        } => checkout(cli.dir, &branch, create, force),
        Command::Ls {
            files,
            namespaces,
            design,
            staged,
        } => ls(cli.dir, files, namespaces, design, staged),
        Command::Status => status(cli.dir),
        Command::Verify => verify(cli.dir),
        Command::Commit {
            message,
            author,
            allow_empty,
        } => commit(cli.dir, &message, &author, allow_empty),
        Command::Show {
            name,
            source,
            staged,
        } => show(cli.dir, &name, source, staged),
        Command::Log { limit } => log(cli.dir, limit),
        Command::Gc => gc(cli.dir),
        Command::Repack => repack(cli.dir),
    }
}

fn init(dir: Option<PathBuf>, source: &str, format: ObjectFormat) -> Result<()> {
    let root = dir.unwrap_or(std::env::current_dir()?);
    let meta = root.join(".ds");
    if meta.join("config").exists() {
        bail!("{} already exists", meta.display());
    }
    let ws = Workspace::init(&meta, source, format)?;

    let mut catalog = Catalog::new(GitOid::ZERO);
    let files = ws.put_symbol("files")?;
    let namespaces = ws.put_symbol("namespaces")?;
    catalog.insert(
        files,
        DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: files,
                parent: ParentRef::View,
                source: None,
                design_doc: DocHash::ZERO,
                children: vec![],
            }),
        ),
    )?;
    catalog.insert(
        namespaces,
        DesignObject::new(
            Kind::Namespace,
            ObjectPayload::Namespace(NamespacePayload {
                qualified_name: namespaces,
                parent: ParentRef::View,
                source: None,
                content_hash: Hash32::ZERO,
                design_doc: DocHash::ZERO,
                children: vec![],
            }),
        ),
    )?;
    let view = catalog.rebuild(ws.store())?;
    let state = State {
        base_commit: CommitHash::ZERO,
        root_view: view,
    };
    ws.write_worktree(state)?;
    ws.write_index(state)?;
    println!("Initialized design workspace at {}", meta.display());
    Ok(())
}

fn clone(dir: Option<PathBuf>, remote_url: &str, source: &str, branch: &str) -> Result<()> {
    let root = dir.unwrap_or(std::env::current_dir()?);
    let meta = root.join(".ds");
    if meta.join("config").exists() {
        bail!("{} already exists", meta.display());
    }
    let ws = Workspace::init(&meta, source, ObjectFormat::Sha1)?;
    let token_file = std::env::var_os("DS_TOKEN_FILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let remote = Remote::open(remote_url, token_file.as_deref())?;
    remote.fetch(&ws)?;
    let commit_hash = ws
        .read_remote_ref(branch)?
        .with_context(|| format!("remote branch not found: {branch}"))?;
    let commit = ws.get_commit(commit_hash)?;
    ws.write_ref(branch, commit_hash)?;
    ws.write_head(&format!("ref: refs/heads/{branch}"))?;
    let state = State {
        base_commit: commit_hash,
        root_view: commit.view,
    };
    ws.write_index(state)?;
    ws.write_worktree(state)?;
    println!(
        "Cloned {branch} from {} at {}",
        remote.display(),
        commit_hash.to_hex()
    );
    Ok(())
}

fn create(dir: Option<PathBuf>, args: CreateArgs) -> Result<()> {
    validate_create_args(&args)?;
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    sync_source_commit(&ws, &mut catalog)?;
    let object = build_object(&ws, &catalog, &args)?;
    let qname = object_qname(&object).context("view objects cannot be created")?;
    let qname_text = ws.get_symbol(qname)?;
    catalog.insert(qname, object)?;
    let view = catalog.rebuild(ws.store())?;
    ws.write_worktree(State {
        base_commit: state.base_commit,
        root_view: view,
    })?;
    println!("Created {qname_text}");
    println!("Worktree view {}", view.to_hex());
    Ok(())
}

fn remote(dir: Option<PathBuf>, command: RemoteCommand) -> Result<()> {
    let mut ws = open_workspace(dir)?;
    match command {
        RemoteCommand::Add { url, token_file } => {
            if ws.remote_url().is_some() {
                bail!("remote already configured; use `ds remote set-url`");
            }
            let token_file = token_file.map(prepare_token_file).transpose()?;
            ws.set_remote_url(url.clone())?;
            println!("remote: {url}");
            if let Some(path) = token_file {
                ws.set_remote_token_file(Some(path.clone()))?;
                println!("token-file: {path}");
            }
        }
        RemoteCommand::Show => match ws.remote_url() {
            Some(url) => {
                println!("{url}");
                if let Some(path) = ws.remote_token_file() {
                    println!("token-file: {path}");
                }
            }
            None => println!("no remote configured"),
        },
        RemoteCommand::SetUrl { url } => {
            ws.set_remote_url(url.clone())?;
            println!("remote: {url}");
        }
        RemoteCommand::SetToken { path } => {
            let path = path.map(prepare_token_file).transpose()?;
            ws.set_remote_token_file(path)?;
            match ws.remote_token_file() {
                Some(path) => println!("token-file: {path}"),
                None => println!("token-file cleared"),
            }
        }
    }
    Ok(())
}

fn prepare_token_file(path: PathBuf) -> Result<String> {
    crate::remote::read_token_file(&path)?;
    let path = fs::canonicalize(&path)
        .with_context(|| format!("resolve token file: {}", path.display()))?;
    Ok(path.to_string_lossy().into_owned())
}

fn remote_for(ws: &Workspace) -> Result<Remote> {
    let url = ws
        .remote_url()
        .context("no remote configured; run `ds remote add <path>`")?;
    let token_file = ws.remote_token_file().map(PathBuf::from);
    Remote::open(url, token_file.as_deref())
}

fn push(dir: Option<PathBuf>, branch: Option<String>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let remote = remote_for(&ws)?;
    let branch = branch.unwrap_or(current_branch(&ws)?);
    let commit = remote.push(&ws, &branch)?;
    println!(
        "Pushed {branch} to {}: {}",
        remote.display(),
        commit.to_hex()
    );
    Ok(())
}

fn fetch(dir: Option<PathBuf>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let remote = remote_for(&ws)?;
    let refs = remote.fetch(&ws)?;
    for (branch, commit) in refs {
        println!("origin/{branch} -> {}", commit.to_hex());
    }
    Ok(())
}

fn pull(dir: Option<PathBuf>, branch: Option<String>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let remote = remote_for(&ws)?;
    let branch = branch.unwrap_or(current_branch(&ws)?);
    let commit = remote.pull(&ws, &branch)?;
    println!("Pulled {branch}: {}", commit.to_hex());
    Ok(())
}


fn object_qname_text(path: &str) -> String {
    format!("files/{path}")
}

fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

fn parent_qname(ws: &Workspace, path: &str) -> Result<SymbolHash> {
    match parent_path(path) {
        Some(parent) => Ok(ws.put_symbol(&object_qname_text(parent))?),
        None => Ok(ws.put_symbol("files")?),
    }
}

fn object_source_path(ws: &Workspace, object: &DesignObject) -> Result<Option<String>> {
    let symbol = match &object.payload {
        ObjectPayload::Directory(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::File(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Namespace(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Class(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Interface(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Method(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Function(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Part(p) => Some(p.source.path),
        ObjectPayload::Mixin(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::View(_) => None,
    };
    match symbol {
        Some(symbol) => Ok(Some(ws.get_symbol(symbol)?)),
        None => Ok(None),
    }
}

fn set_object_source(ws: &Workspace, object: &mut DesignObject, path: &str) -> Result<()> {
    let symbol = ws.put_symbol(path)?;
    match &mut object.payload {
        ObjectPayload::Directory(p) => p.source = Some(PathSource { path: symbol }),
        ObjectPayload::File(p) => p.source = Some(PathSource { path: symbol }),
        ObjectPayload::Namespace(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Class(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Interface(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Method(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Function(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Part(p) => p.source.path = symbol,
        ObjectPayload::Mixin(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::View(_) => bail!("view objects have no source"),
    }
    Ok(())
}

fn set_object_parent(object: &mut DesignObject, parent: SymbolHash) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::Directory(p) => p.parent = ParentRef::Object(parent),
        ObjectPayload::File(p) => p.parent = parent,
        _ => bail!("rename source is not a File or Directory"),
    }
    Ok(())
}

fn remove_source_subtree(catalog: &mut Catalog, qname: SymbolHash) -> usize {
    let mut subtree = BTreeSet::new();
    let mut queue = vec![qname];
    while let Some(current) = queue.pop() {
        if subtree.insert(current) {
            queue.extend(catalog.children(current));
        }
    }
    let mut external = BTreeSet::new();
    for target in &subtree {
        for referrer in catalog.references_to(*target) {
            if !subtree.contains(&referrer) {
                external.insert((*target, referrer));
            }
        }
    }
    for (target, referrer) in external {
        if let Some(object) = catalog.get_mut(referrer) {
            remove_references_to(object, target);
        }
    }
    let count = subtree.len();
    for qname in subtree {
        catalog.remove(qname);
    }
    count
}

fn ensure_directory(ws: &Workspace, catalog: &mut Catalog, path: &str) -> Result<bool> {
    let qname = ws.put_symbol(&object_qname_text(path))?;
    let parent = parent_qname(ws, path)?;
    let source = ws.put_symbol(path)?;
    match catalog.get(qname).map(|object| object.kind) {
        Some(Kind::Directory) => {
            if let Some(object) = catalog.get_mut(qname)
                && let ObjectPayload::Directory(p) = &mut object.payload
            {
                p.parent = ParentRef::Object(parent);
                p.source = Some(PathSource { path: source });
            }
            Ok(false)
        }
        Some(_) => {
            remove_source_subtree(catalog, qname);
            create_directory(catalog, qname, parent, source)?;
            Ok(true)
        }
        None => {
            create_directory(catalog, qname, parent, source)?;
            Ok(true)
        }
    }
}

fn create_directory(
    catalog: &mut Catalog,
    qname: SymbolHash,
    parent: SymbolHash,
    source: SymbolHash,
) -> Result<()> {
    catalog.insert(
        qname,
        DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: qname,
                parent: ParentRef::Object(parent),
                source: Some(PathSource { path: source }),
                design_doc: DocHash::ZERO,
                children: vec![],
            }),
        ),
    )?;
    Ok(())
}

fn retarget_source_paths(
    ws: &Workspace,
    catalog: &mut Catalog,
    old_path: &str,
    new_path: &str,
) -> Result<()> {
    let mut targets = Vec::new();
    for (qname, object) in catalog.iter() {
        if object_source_path(ws, object)?.as_deref() == Some(old_path) {
            targets.push(qname);
        }
    }
    for qname in targets {
        if let Some(object) = catalog.get_mut(qname) {
            set_object_source(ws, object, new_path)?;
        }
    }
    Ok(())
}

fn rename_file(
    ws: &Workspace,
    catalog: &mut Catalog,
    old_path: &str,
    new_path: &str,
) -> Result<bool> {
    let old_qname = ws.put_symbol(&object_qname_text(old_path))?;
    let Some(object) = catalog.get(old_qname) else {
        return Ok(false);
    };
    if object.kind != Kind::File {
        return Ok(false);
    }
    let new_qname = ws.put_symbol(&object_qname_text(new_path))?;
    if catalog.get(new_qname).is_some() {
        return Ok(false);
    }
    let mapping = rename_mapping(ws, catalog, old_qname, new_qname)?;
    for target in mapping.values() {
        if *target != new_qname && catalog.get(*target).is_some() && !mapping.contains_key(target) {
            bail!("rename target already exists: {}", ws.get_symbol(*target)?);
        }
    }
    let parent = parent_qname(ws, new_path)?;
    if let Some(object) = catalog.get_mut(old_qname) {
        set_object_parent(object, parent)?;
    }
    catalog.rename_symbols(&mapping)?;
    retarget_source_paths(ws, catalog, old_path, new_path)?;
    Ok(true)
}

fn sync_source(ws: &Workspace, repo: &SourceRepo) -> Result<()> {
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    let head = repo.resolve_current()?;
    let head_oid = repo.current_git_oid()?;
    let base = catalog.source_commit();
    let changes = if base.is_zero() {
        Vec::new()
    } else {
        repo.changes(&repo.oid_hex(base), &head)?
    };
    let blobs = repo.list_blobs(&head)?;

    let mut file_paths = BTreeSet::new();
    let mut dir_paths = BTreeSet::new();
    for blob in &blobs {
        file_paths.insert(blob.path.clone());
        let mut parent = parent_path(&blob.path);
        while let Some(dir) = parent {
            dir_paths.insert(dir.to_owned());
            parent = parent_path(dir);
        }
    }

    let mut dirs = dir_paths.iter().cloned().collect::<Vec<_>>();
    dirs.sort_by_key(|path| path.matches('/').count());
    let mut created_dirs = 0usize;
    for dir in &dirs {
        if ensure_directory(ws, &mut catalog, dir)? {
            created_dirs += 1;
        }
    }

    let mut renamed = 0usize;
    for change in changes.iter().filter(|change| change.status == 'R') {
        let Some(old_path) = change.old_path.as_deref() else {
            continue;
        };
        if rename_file(ws, &mut catalog, old_path, &change.path)? {
            renamed += 1;
        }
    }

    let mut stale = BTreeSet::new();
    for (qname, object) in catalog.iter() {
        let Some(path) = object_source_path(ws, object)? else {
            continue;
        };
        let keep = match object.kind {
            Kind::File => file_paths.contains(&path),
            Kind::Directory => dir_paths.contains(&path),
            _ => true,
        };
        if !keep {
            stale.insert(qname);
        }
    }
    let mut removed = 0usize;
    for qname in stale {
        removed += remove_source_subtree(&mut catalog, qname);
    }

    let mut created_files = 0usize;
    let mut updated_files = 0usize;
    for blob in &blobs {
        let qname = ws.put_symbol(&object_qname_text(&blob.path))?;
        let parent = parent_qname(ws, &blob.path)?;
        let source = ws.put_symbol(&blob.path)?;
        let hash = content_hash(&repo.read_path(&head, &blob.path)?);
        match catalog.get(qname).map(|object| object.kind) {
            Some(Kind::File) => {
                if let Some(object) = catalog.get_mut(qname)
                    && let ObjectPayload::File(p) = &mut object.payload
                {
                    p.parent = parent;
                    p.source = Some(PathSource { path: source });
                    p.content_hash = hash;
                }
                updated_files += 1;
            }
            _ => {
                if catalog.get(qname).is_some() {
                    removed += remove_source_subtree(&mut catalog, qname);
                }
                catalog.insert(
                    qname,
                    DesignObject::new(
                        Kind::File,
                        ObjectPayload::File(FilePayload {
                            qualified_name: qname,
                            parent,
                            source: Some(PathSource { path: source }),
                            content_hash: hash,
                            design_doc: DocHash::ZERO,
                            children: vec![],
                        }),
                    ),
                )?;
                created_files += 1;
            }
        }
    }

    catalog.set_source_commit(head_oid);
    let view = catalog.rebuild(ws.store())?;
    ws.write_worktree(State {
        base_commit: state.base_commit,
        root_view: view,
    })?;
    println!(
        "Synced files/directories: {created_files} added, {updated_files} updated, \
         {created_dirs} directories added, {removed} objects removed, {renamed} renamed"
    );
    Ok(())
}

fn source(dir: Option<PathBuf>, command: SourceCommand) -> Result<()> {
    let ws = open_workspace(dir)?;
    let repo = SourceRepo::open(ws.meta_root(), ws.source_url(), ws.object_format())?;
    match command {
        SourceCommand::Fetch => {
            repo.fetch()?;
            let commit = repo.resolve_current()?;
            println!("Fetched source; current commit {commit}");
        }
        SourceCommand::Checkout { git_ref } => {
            if !repo.exists() {
                bail!("source repository is missing; run `ds source fetch`");
            }
            let source_ref = repo.checkout(&git_ref)?;
            let commit = repo.resolve_current()?;
            println!("Checked out {} ({commit})", source_ref.label);
        }
        SourceCommand::Status => {
            if !repo.exists() {
                println!("source: not fetched");
                return Ok(());
            }
            println!("ref:    {}", repo.current_ref()?);
            println!("commit: {}", repo.resolve_current()?);
        }
        SourceCommand::Ls { path } => {
            if !repo.exists() {
                bail!("source repository is missing; run `ds source fetch`");
            }
            let commit = repo.resolve_current()?;
            for entry in repo.list_blobs(&commit)? {
                if path
                    .as_deref()
                    .is_none_or(|prefix| entry.path.starts_with(prefix))
                {
                    println!("{}", entry.path);
                }
            }
        }
        SourceCommand::Cat { path } => {
            if !repo.exists() {
                bail!("source repository is missing; run `ds source fetch`");
            }
            let commit = repo.resolve_current()?;
            let bytes = repo.read_path(&commit, &path)?;
            std::io::stdout().write_all(&bytes)?;
        }
        SourceCommand::Log { git_ref, limit } => {
            if !repo.exists() {
                bail!("source repository is missing; run `ds source fetch`");
            }
            let target = match git_ref {
                Some(git_ref) => git_ref,
                None => {
                    let current = repo.current_ref()?;
                    current
                        .strip_prefix("branch ")
                        .or_else(|| current.strip_prefix("commit "))
                        .context("malformed source-head")?
                        .to_owned()
                }
            };
            let commit = repo.resolve_source(&target)?;
            for line in repo.log(&commit, limit)? {
                let fields: Vec<&str> = line.split('\0').collect();
                if fields.len() != 4 {
                    continue;
                }
                println!("commit {}", fields[0]);
                println!("Author: {}", fields[1]);
                println!("Date:   {}", fields[2]);
                println!();
                println!("    {}", fields[3]);
                println!();
            }
        }
        SourceCommand::Sync => {
            if !repo.exists() {
                bail!("source repository is missing; run `ds source fetch`");
            }
            sync_source(&ws, &repo)?;
        }
        SourceCommand::Changes { base } => {
            if !repo.exists() {
                bail!("source repository is missing; run `ds source fetch`");
            }
            let head = repo.resolve_current()?;
            let base = match base {
                Some(base) => Some(repo.resolve_source(&base)?),
                None => latest_design_source_commit(&ws, &repo)?,
            };
            let Some(base) = base else {
                for entry in repo.list_blobs(&head)? {
                    println!("A\t{}", entry.path);
                }
                return Ok(());
            };
            if !repo.is_ancestor(&base, &head)? {
                for entry in repo.list_blobs(&head)? {
                    println!("A\t{}", entry.path);
                }
                return Ok(());
            }
            for change in repo.changes(&base, &head)? {
                match change.status {
                    'R' => println!(
                        "R\t{}\t{}",
                        change.old_path.as_deref().unwrap_or_default(),
                        change.path
                    ),
                    status => println!("{status}\t{}", change.path),
                }
            }
        }
    }
    Ok(())
}

fn latest_design_source_commit(ws: &Workspace, repo: &SourceRepo) -> Result<Option<String>> {
    let branch = current_branch(ws)?;
    let Some(commit_hash) = ws.read_ref(&branch)? else {
        return Ok(None);
    };
    let commit = ws.get_commit(commit_hash)?;
    let view = ws.get_object(commit.view)?;
    let ObjectPayload::View(payload) = &view.payload else {
        return Ok(None);
    };
    if payload.source_commit.is_zero() {
        Ok(None)
    } else {
        Ok(Some(repo.oid_hex(payload.source_commit)))
    }
}

fn sync_source_commit(ws: &Workspace, catalog: &mut Catalog) -> Result<()> {
    let repo = SourceRepo::open(ws.meta_root(), ws.source_url(), ws.object_format())?;
    if repo.exists() {
        catalog.set_source_commit(repo.current_git_oid()?);
    }
    Ok(())
}

fn edit(dir: Option<PathBuf>, args: EditArgs) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    sync_source_commit(&ws, &mut catalog)?;
    let old_qname = ws.put_symbol(&args.name)?;
    let mut object = catalog
        .get(old_qname)
        .with_context(|| format!("object not found: {}", args.name))?
        .clone();
    let old_text = ws.get_symbol(old_qname)?;

    if args.clear_design {
        set_design_doc(&mut object, DocHash::ZERO)?;
    } else if let Some(text) = args.design.as_deref() {
        set_design_doc(&mut object, ws.put_doc(text)?)?;
    } else if let Some(path) = args.design_file.as_deref() {
        set_design_doc(&mut object, ws.put_doc(&fs::read_to_string(path)?)?)?;
    }

    if let Some(spec) = args.source.as_deref() {
        apply_source(&ws, &mut object, spec)?;
    }
    if let Some(target) = args.superclass.as_deref() {
        let symbol = if target.is_empty() {
            SymbolHash::ZERO
        } else {
            relation_symbol(&ws, Some(target))?
        };
        set_superclass(&mut object, symbol)?;
    }
    if !args.interface_add.is_empty() || !args.interface_rm.is_empty() {
        let add = relation_symbols(&ws, &args.interface_add)?;
        let rm = relation_symbols(&ws, &args.interface_rm)?;
        edit_symbol_list(&mut object, &add, &rm, SymbolList::Interfaces)?;
    }
    if !args.mixin_add.is_empty() || !args.mixin_rm.is_empty() {
        let add = ordered_relation_symbols(&ws, &args.mixin_add)?;
        let rm = relation_symbols(&ws, &args.mixin_rm)?;
        edit_symbol_list(&mut object, &add, &rm, SymbolList::Mixins)?;
    }
    if !args.on_add.is_empty() || !args.on_rm.is_empty() {
        let add = relation_symbols(&ws, &args.on_add)?;
        let rm = relation_symbols(&ws, &args.on_rm)?;
        edit_symbol_list(&mut object, &add, &rm, SymbolList::On)?;
    }
    if let Some(target) = args.extends.as_deref() {
        let symbol = if target.is_empty() {
            SymbolHash::ZERO
        } else {
            relation_symbol(&ws, Some(target))?
        };
        set_extends(&mut object, symbol)?;
    }
    if let Some(target) = args.overrides.as_deref() {
        let symbol = if target.is_empty() {
            SymbolHash::ZERO
        } else {
            relation_symbol(&ws, Some(target))?
        };
        set_overrides(&mut object, symbol)?;
    }

    apply_structure_edit(&ws, &catalog, old_qname, &mut object, &args)?;
    apply_rename(&ws, &mut object, args.new_name.as_deref())?;
    format::validate_object(&object)?;
    let new_qname = object_qname(&object).context("view objects cannot be edited")?;
    if new_qname != old_qname {
        let mapping = rename_mapping(&ws, &catalog, old_qname, new_qname)?;
        for new_name in mapping.values() {
            if catalog.get(*new_name).is_some() && !mapping.contains_key(new_name) {
                bail!(
                    "new qualified name already exists: {}",
                    ws.get_symbol(*new_name)?
                );
            }
        }
        catalog.remove(old_qname);
        catalog.rename_symbols(&mapping)?;
        catalog.insert(new_qname, object)?;
    } else {
        catalog.replace(old_qname, object)?;
    }
    let view = catalog.rebuild(ws.store())?;
    ws.write_worktree(State {
        base_commit: state.base_commit,
        root_view: view,
    })?;
    println!("Edited {} as {}", old_text, ws.get_symbol(new_qname)?);
    Ok(())
}

fn apply_structure_edit(
    ws: &Workspace,
    catalog: &Catalog,
    old_qname: SymbolHash,
    object: &mut DesignObject,
    args: &EditArgs,
) -> Result<()> {
    if args.parent.is_some() && args.namespace.is_some() {
        bail!("use either --parent or --namespace, not both");
    }
    if args.parent.is_none()
        && args.namespace.is_none()
        && args.file.is_none()
        && args.module.is_none()
    {
        return Ok(());
    }

    let old_text = ws.get_symbol(old_qname)?;
    let local = local_name(&old_text);
    let is_root = old_qname == symbol_hash("files") || old_qname == symbol_hash("namespaces");

    match &mut object.payload {
        ObjectPayload::Directory(p) => {
            if is_root && args.parent.is_some() {
                bail!("cannot move a root directory");
            }
            if let Some(parent) = args.parent.as_deref() {
                let parent = parent_symbol(ws, catalog, parent, &[Kind::Directory])?;
                p.parent = ParentRef::Object(parent);
                p.qualified_name = rename_qname(ws, parent, &local)?;
            }
        }
        ObjectPayload::File(p) => {
            if let Some(parent) = args.parent.as_deref() {
                let parent = parent_symbol(ws, catalog, parent, &[Kind::Directory])?;
                p.parent = parent;
                p.qualified_name = rename_qname(ws, parent, &local)?;
            }
        }
        ObjectPayload::Namespace(p) => {
            if is_root && args.parent.is_some() {
                bail!("cannot move the root namespace");
            }
            if let Some(parent) = args.parent.as_deref() {
                let parent = parent_symbol(ws, catalog, parent, &[Kind::Namespace])?;
                p.parent = ParentRef::Object(parent);
                p.qualified_name = rename_qname(ws, parent, &local)?;
            }
        }
        ObjectPayload::Class(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
            )?;
        }
        ObjectPayload::Interface(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
            )?;
        }
        ObjectPayload::Mixin(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
            )?;
        }
        ObjectPayload::Method(p) => {
            if let Some(parent) = args.parent.as_deref() {
                let parent = parent_symbol(
                    ws,
                    catalog,
                    parent,
                    &[Kind::Class, Kind::Interface, Kind::Part],
                )?;
                let (name, sig) = local
                    .split_once('#')
                    .context("method qname has no signature hash")?;
                p.parent = parent;
                p.qualified_name =
                    ws.put_symbol(&format!("{}/{}#{}", ws.get_symbol(parent)?, name, sig))?;
            }
        }
        ObjectPayload::Part(p) => {
            if let Some(parent) = args.parent.as_deref() {
                let parent = parent_symbol(ws, catalog, parent, &[Kind::Class, Kind::Interface])?;
                p.parent = parent;
                p.qualified_name =
                    ws.put_symbol(&format!("{}/{}", ws.get_symbol(parent)?, local))?;
            }
        }
        ObjectPayload::Function(p) => {
            if let Some(namespace) = args.namespace.as_deref() {
                let namespace = required_namespace(ws, catalog, Some(namespace))?;
                p.namespace = namespace;
                p.qualified_name = rename_qname(ws, namespace, &local)?;
            }
            if let Some(file) = args.file.as_deref() {
                p.file = optional_ref(ws, Some(file))?;
            }
            if let Some(module) = args.module.as_deref() {
                p.module = optional_ref(ws, Some(module))?;
            }
        }
        ObjectPayload::View(_) => {}
    }
    Ok(())
}

fn rename_mapping(
    ws: &Workspace,
    catalog: &Catalog,
    old_qname: SymbolHash,
    new_qname: SymbolHash,
) -> Result<BTreeMap<SymbolHash, SymbolHash>> {
    let old_text = ws.get_symbol(old_qname)?;
    let new_text = ws.get_symbol(new_qname)?;
    let prefix = format!("{old_text}/");
    let mut mapping = BTreeMap::from([(old_qname, new_qname)]);
    let mut queue = catalog.children(old_qname);
    let mut seen = BTreeSet::new();
    while let Some(child) = queue.pop() {
        if !seen.insert(child) {
            continue;
        }
        let text = ws.get_symbol(child)?;
        let suffix = text
            .strip_prefix(&prefix)
            .with_context(|| format!("child qname does not share the old prefix: {text}"))?;
        let renamed = ws.put_symbol(&format!("{new_text}/{suffix}"))?;
        mapping.insert(child, renamed);
        queue.extend(catalog.children(child));
    }
    Ok(mapping)
}

fn apply_rename(ws: &Workspace, object: &mut DesignObject, name: Option<&str>) -> Result<()> {
    let Some(name) = name else {
        return Ok(());
    };
    if name.is_empty() || name.contains('/') || name.contains('#') {
        bail!("invalid local name: {name}");
    }
    match &mut object.payload {
        ObjectPayload::Directory(p) => {
            let ParentRef::Object(parent) = p.parent else {
                bail!("cannot rename a root object");
            };
            p.qualified_name = rename_qname(ws, parent, name)?;
        }
        ObjectPayload::File(p) => {
            p.qualified_name = rename_qname(ws, p.parent, name)?;
        }
        ObjectPayload::Namespace(p) => {
            let ParentRef::Object(parent) = p.parent else {
                bail!("cannot rename a root object");
            };
            p.qualified_name = rename_qname(ws, parent, name)?;
        }
        ObjectPayload::Class(p) => {
            rename_type(ws, &p.parent, &mut p.qualified_name, name)?;
        }
        ObjectPayload::Interface(p) => {
            rename_type(ws, &p.parent, &mut p.qualified_name, name)?;
        }
        ObjectPayload::Mixin(p) => {
            rename_type(ws, &p.parent, &mut p.qualified_name, name)?;
        }
        ObjectPayload::Function(p) => {
            p.qualified_name = rename_qname(ws, p.namespace, name)?;
        }
        ObjectPayload::Method(_) => {
            bail!("method name cannot be changed; create a new method");
        }
        ObjectPayload::Part(_) => {
            bail!("part name is derived from its content hash");
        }
        ObjectPayload::View(_) => bail!("view has no name"),
    }
    Ok(())
}

fn rename_type(
    ws: &Workspace,
    parent: &ClassParent,
    qname: &mut SymbolHash,
    name: &str,
) -> Result<()> {
    let parent = match parent {
        ClassParent::TopLevel { namespace, .. } => *namespace,
        ClassParent::Nested { parent } => *parent,
    };
    *qname = rename_qname(ws, parent, name)?;
    Ok(())
}

fn edit_class_parent(
    ws: &Workspace,
    catalog: &Catalog,
    parent: &mut ClassParent,
    qname: &mut SymbolHash,
    local: &str,
    args: &EditArgs,
) -> Result<()> {
    if let Some(new_parent) = args.parent.as_deref() {
        let new_parent = parent_symbol(ws, catalog, new_parent, &[Kind::Class, Kind::Interface])?;
        *parent = ClassParent::Nested { parent: new_parent };
        *qname = rename_qname(ws, new_parent, local)?;
        if args.file.is_some() || args.module.is_some() {
            bail!("nested type has no file/module parent");
        }
        return Ok(());
    }

    if let Some(namespace) = args.namespace.as_deref() {
        let namespace = required_namespace(ws, catalog, Some(namespace))?;
        let (file, module) = match *parent {
            ClassParent::TopLevel { file, module, .. } => (file, module),
            ClassParent::Nested { .. } => (SymbolHash::ZERO, SymbolHash::ZERO),
        };
        *parent = ClassParent::TopLevel {
            file: args
                .file
                .as_deref()
                .map(|_| optional_ref(ws, args.file.as_deref()))
                .transpose()?
                .unwrap_or(file),
            namespace,
            module: args
                .module
                .as_deref()
                .map(|_| optional_ref(ws, args.module.as_deref()))
                .transpose()?
                .unwrap_or(module),
        };
        *qname = rename_qname(ws, namespace, local)?;
        return Ok(());
    }

    if let ClassParent::TopLevel {
        file,
        namespace: _,
        module,
    } = parent
    {
        if let Some(new_file) = args.file.as_deref() {
            *file = optional_ref(ws, Some(new_file))?;
        }
        if let Some(new_module) = args.module.as_deref() {
            *module = optional_ref(ws, Some(new_module))?;
        }
    } else if args.file.is_some() || args.module.is_some() {
        bail!("nested type has no file/module parent");
    }
    Ok(())
}

fn parent_symbol(
    ws: &Workspace,
    catalog: &Catalog,
    text: &str,
    allowed: &[Kind],
) -> Result<SymbolHash> {
    let symbol = ws.put_symbol(text)?;
    let object = catalog
        .get(symbol)
        .with_context(|| format!("parent not found: {text}"))?;
    if !allowed.contains(&object.kind) {
        bail!("parent {text} has kind {:?}", object.kind);
    }
    Ok(symbol)
}

fn rename_qname(ws: &Workspace, parent: SymbolHash, local: &str) -> Result<SymbolHash> {
    Ok(ws.put_symbol(&format!("{}/{}", ws.get_symbol(parent)?, local))?)
}

fn local_name(text: &str) -> String {
    text.rsplit('/').next().unwrap_or(text).to_owned()
}

fn dep(dir: Option<PathBuf>, command: DepCommand) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    sync_source_commit(&ws, &mut catalog)?;
    match command {
        DepCommand::Add {
            src,
            dst,
            explicit,
            implicit,
            evidence,
            rationale_file,
        } => {
            if explicit == implicit {
                bail!("choose exactly one of --explicit or --implicit");
            }
            let src_qname = ws.put_symbol(&src)?;
            let dst_qname = ws.put_symbol(&dst)?;
            if catalog.get(src_qname).is_none() {
                bail!("source not found: {src}");
            }
            let structural = structural_dependencies(
                catalog
                    .get(src_qname)
                    .with_context(|| format!("source not found: {src}"))?,
            );
            if structural.contains(&dst_qname) {
                bail!("target is already a structural dependency");
            }
            let evidence = match evidence {
                Some(spec) => Some(read_evidence(&ws, &spec)?),
                None => None,
            };
            let rationale = match rationale_file {
                Some(path) => ws.put_doc(&fs::read_to_string(path)?)?,
                None => DocHash::ZERO,
            };
            let dependency = Dependency {
                target: dst_qname,
                evidence,
                rationale,
            };
            let object = catalog
                .get_mut(src_qname)
                .with_context(|| format!("source not found: {src}"))?;
            add_dependency(object, dependency, explicit)?;
            let view = catalog.rebuild(ws.store())?;
            ws.write_worktree(State {
                base_commit: state.base_commit,
                root_view: view,
            })?;
            println!(
                "{} {}",
                if explicit { "Explicit" } else { "Implicit" },
                dependency_label(&ws, dst_qname)?
            );
        }
        DepCommand::Rm {
            src,
            dst,
            explicit,
            implicit,
        } => {
            if explicit == implicit {
                bail!("choose exactly one of --explicit or --implicit");
            }
            let src_qname = ws.put_symbol(&src)?;
            let dst_qname = ws.put_symbol(&dst)?;
            let structural = catalog
                .get(src_qname)
                .map(structural_dependencies)
                .unwrap_or_default();
            if structural.contains(&dst_qname) {
                bail!("structural dependency cannot be removed");
            }
            let object = catalog
                .get_mut(src_qname)
                .with_context(|| format!("source not found: {src}"))?;
            remove_dependency(object, dst_qname, explicit)?;
            let view = catalog.rebuild(ws.store())?;
            ws.write_worktree(State {
                base_commit: state.base_commit,
                root_view: view,
            })?;
            println!("Removed dependency {} -> {}", src, dst);
        }
        DepCommand::List { name } => {
            let qname = ws.put_symbol(&name)?;
            let object = catalog
                .get(qname)
                .with_context(|| format!("object not found: {name}"))?;
            let (explicit, implicit) = dependency_lists(object)?;
            let mut explicit_targets = structural_dependencies(object)
                .into_iter()
                .collect::<BTreeSet<_>>();
            explicit_targets.extend(explicit.iter().map(|dependency| dependency.target));
            println!("explicit:");
            for target in explicit_targets {
                println!("  {}", dependency_label(&ws, target)?);
            }
            println!("implicit:");
            for dep in implicit {
                println!("  {}", dependency_label(&ws, dep.target)?);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SymbolList {
    Interfaces,
    Mixins,
    On,
}

fn set_design_doc(object: &mut DesignObject, doc: DocHash) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::View(_) => bail!("view has no design document"),
        ObjectPayload::Directory(p) => p.design_doc = doc,
        ObjectPayload::File(p) => p.design_doc = doc,
        ObjectPayload::Namespace(p) => p.design_doc = doc,
        ObjectPayload::Class(p) => p.design_doc = doc,
        ObjectPayload::Interface(p) => p.design_doc = doc,
        ObjectPayload::Method(p) => p.design_doc = doc,
        ObjectPayload::Function(p) => p.design_doc = doc,
        ObjectPayload::Part(p) => p.design_doc = doc,
        ObjectPayload::Mixin(p) => p.design_doc = doc,
    }
    Ok(())
}

fn apply_source(ws: &Workspace, object: &mut DesignObject, spec: &str) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::Directory(p) => {
            let path = read_source_path(ws, Some(spec))?.context("source missing")?;
            p.source = Some(PathSource { path });
        }
        ObjectPayload::File(p) => {
            let source = read_source(ws, Some(spec), false)?.context("source missing")?;
            p.source = Some(PathSource { path: source.path });
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Namespace(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Class(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Interface(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Method(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Function(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Part(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            if !source.has_range {
                bail!("part source must use path:start-end");
            }
            p.source = source.span_source();
            p.content_hash = source.content_hash;
            let parent = p.parent;
            let qname = format!(
                "{}/part#{}",
                ws.get_symbol(parent)?,
                source.content_hash.to_hex()
            );
            p.qualified_name = ws.put_symbol(&qname)?;
        }
        ObjectPayload::Mixin(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::View(_) => bail!("view has no source"),
    }
    Ok(())
}

fn set_superclass(object: &mut DesignObject, symbol: SymbolHash) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::Class(p) => p.superclass = symbol,
        ObjectPayload::Mixin(p) => p.superclass = symbol,
        _ => bail!("object kind does not have superclass"),
    }
    Ok(())
}

fn set_extends(object: &mut DesignObject, symbol: SymbolHash) -> Result<()> {
    let ObjectPayload::Interface(p) = &mut object.payload else {
        bail!("object kind does not have extends");
    };
    p.extends = symbol;
    Ok(())
}

fn set_overrides(object: &mut DesignObject, symbol: SymbolHash) -> Result<()> {
    let ObjectPayload::Method(p) = &mut object.payload else {
        bail!("object kind does not have overrides");
    };
    p.overrides = symbol;
    Ok(())
}

fn edit_symbol_list(
    object: &mut DesignObject,
    add: &[SymbolHash],
    remove: &[SymbolHash],
    list: SymbolList,
) -> Result<()> {
    let target = match (&mut object.payload, list) {
        (ObjectPayload::Class(p), SymbolList::Interfaces) => &mut p.interfaces,
        (ObjectPayload::Class(p), SymbolList::Mixins) => &mut p.mixins,
        (ObjectPayload::Class(_), SymbolList::On) => bail!("class has no on list"),
        (ObjectPayload::Interface(_), SymbolList::Mixins | SymbolList::On) => {
            bail!("interface has no such list")
        }
        (ObjectPayload::Mixin(p), SymbolList::Interfaces) => &mut p.interfaces,
        (ObjectPayload::Mixin(p), SymbolList::Mixins) => &mut p.mixins,
        (ObjectPayload::Mixin(p), SymbolList::On) => &mut p.on,
        _ => bail!("object kind has no such relation list"),
    };
    target.retain(|item| !remove.contains(item));
    for item in add {
        if !target.contains(item) {
            target.push(*item);
        }
    }
    if !matches!(list, SymbolList::Mixins) {
        target.sort();
    }
    target.dedup();
    Ok(())
}

fn read_evidence(ws: &Workspace, spec: &str) -> Result<DependencyEvidence> {
    let source = read_source(ws, Some(spec), true)?.context("evidence source missing")?;
    Ok(DependencyEvidence {
        path: source.path,
        span: source.span_source().span,
    })
}

fn add_dependency(object: &mut DesignObject, dependency: Dependency, explicit: bool) -> Result<()> {
    let list = match &mut object.payload {
        ObjectPayload::Class(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Interface(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Method(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Function(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Part(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Mixin(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        _ => bail!("object kind cannot have dependencies"),
    };
    if list.iter().any(|item| item.target == dependency.target) {
        bail!("dependency already exists");
    }
    list.push(dependency);
    list.sort_by_key(|item| item.target.0.0);
    Ok(())
}

fn select_dependency_list<'a>(
    explicit: &'a mut Vec<Dependency>,
    implicit: &'a mut Vec<Dependency>,
    is_explicit: bool,
) -> Result<&'a mut Vec<Dependency>> {
    if is_explicit {
        Ok(explicit)
    } else {
        Ok(implicit)
    }
}

fn remove_references_to(object: &mut DesignObject, target: SymbolHash) -> bool {
    let mut changed = false;
    match &mut object.payload {
        ObjectPayload::Class(p) => {
            if p.superclass == target {
                p.superclass = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_symbol(&mut p.interfaces, target);
            changed |= remove_symbol(&mut p.mixins, target);
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Interface(p) => {
            if p.extends == target {
                p.extends = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Method(p) => {
            if p.overrides == target {
                p.overrides = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Function(p) => {
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Part(p) => {
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Mixin(p) => {
            changed |= remove_symbol(&mut p.on, target);
            if p.superclass == target {
                p.superclass = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_symbol(&mut p.interfaces, target);
            changed |= remove_symbol(&mut p.mixins, target);
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        _ => {}
    }
    changed
}

fn remove_symbol(list: &mut Vec<SymbolHash>, target: SymbolHash) -> bool {
    let before = list.len();
    list.retain(|symbol| *symbol != target);
    list.len() != before
}

fn remove_dependencies(list: &mut Vec<Dependency>, target: SymbolHash) -> bool {
    let before = list.len();
    list.retain(|dependency| dependency.target != target);
    list.len() != before
}

fn remove_dependency(object: &mut DesignObject, target: SymbolHash, explicit: bool) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::Class(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Interface(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Method(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Function(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Part(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Mixin(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        _ => bail!("object kind cannot have dependencies"),
    }?;
    Ok(())
}

fn remove_from(list: &mut Vec<Dependency>, target: SymbolHash) -> Result<()> {
    let before = list.len();
    list.retain(|item| item.target != target);
    if list.len() == before {
        bail!("dependency not found");
    }
    Ok(())
}

fn dependency_lists(object: &DesignObject) -> Result<(&[Dependency], &[Dependency])> {
    match &object.payload {
        ObjectPayload::Class(p) => Ok((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Interface(p) => Ok((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Method(p) => Ok((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Function(p) => Ok((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Part(p) => Ok((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Mixin(p) => Ok((&p.explicit_dependencies, &p.implicit_dependencies)),
        _ => bail!("object kind cannot have dependencies"),
    }
}

fn dependency_label(ws: &Workspace, target: SymbolHash) -> Result<String> {
    Ok(ws.get_symbol(target)?)
}

fn add(dir: Option<PathBuf>, names: Vec<String>, all: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let worktree = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let index = ws
        .read_index()?
        .context("index is not initialized; run `ds init`")?;
    let work_catalog = Catalog::load(ws.store(), worktree.root_view)?;
    let mut index_catalog = Catalog::load(ws.store(), index.root_view)?;

    if all {
        index_catalog = work_catalog.clone();
    } else {
        for name in names {
            let qname = ws.put_symbol(&name)?;
            if work_catalog.get(qname).is_none() {
                bail!("object not found in worktree: {name}");
            }
            stage_object_with_parents(&work_catalog, &mut index_catalog, qname)?;
        }
    }

    let view = index_catalog.rebuild(ws.store())?;
    ws.write_index(State {
        base_commit: index.base_commit,
        root_view: view,
    })?;
    println!("Staged index view {}", view.to_hex());
    Ok(())
}

fn stage_object_with_parents(
    worktree: &Catalog,
    index: &mut Catalog,
    qname: SymbolHash,
) -> Result<()> {
    let mut queue = vec![qname];
    let mut seen = BTreeSet::new();
    while let Some(current) = queue.pop() {
        if !seen.insert(current) {
            continue;
        }
        let object = worktree
            .get(current)
            .with_context(|| format!("object not found in worktree: {}", current.to_hex()))?
            .clone();
        if current == qname || index.get(current).is_none() {
            if index.get(current).is_some() {
                index.replace(current, object)?;
            } else {
                index.insert(current, object)?;
            }
        }
        queue.extend(worktree.parents(current));
    }
    Ok(())
}

fn reset(dir: Option<PathBuf>, names: Vec<String>, hard: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let branch = current_branch(&ws)?;
    let Some(commit_hash) = ws.read_ref(&branch)? else {
        bail!("branch {branch} has no commit yet");
    };
    let commit = ws.get_commit(commit_hash)?;
    let head_catalog = Catalog::load(ws.store(), commit.view)?;

    if hard {
        let view = head_catalog.rebuild(ws.store())?;
        let state = State {
            base_commit: commit_hash,
            root_view: view,
        };
        ws.write_index(state)?;
        ws.write_worktree(state)?;
        println!("Reset worktree and index to {}", commit_hash.to_hex());
        return Ok(());
    }

    let index = ws
        .read_index()?
        .context("index is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), index.root_view)?;
    if names.is_empty() {
        catalog = head_catalog;
    } else {
        for name in names {
            let qname = ws.put_symbol(&name)?;
            if let Some(object) = head_catalog.get(qname) {
                if catalog.get(qname).is_some() {
                    catalog.replace(qname, object.clone())?;
                } else {
                    catalog.insert(qname, object.clone())?;
                }
            } else if catalog.get(qname).is_some() {
                remove_subtree(&mut catalog, qname);
            } else {
                bail!("object not found in index or HEAD: {name}");
            }
        }
    }
    let view = catalog.rebuild(ws.store())?;
    ws.write_index(State {
        base_commit: index.base_commit,
        root_view: view,
    })?;
    println!("Reset index to {}", view.to_hex());
    Ok(())
}

fn remove_subtree(catalog: &mut Catalog, qname: SymbolHash) {
    let mut queue = vec![qname];
    let mut seen = BTreeSet::new();
    while let Some(current) = queue.pop() {
        if !seen.insert(current) {
            continue;
        }
        queue.extend(catalog.children(current));
        catalog.remove(current);
    }
}

fn status(dir: Option<PathBuf>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let branch = current_branch(&ws)?;
    let head = ws.read_ref(&branch)?;
    let index = ws.read_index()?;
    let worktree = ws.read_worktree()?;
    println!("On branch {branch}");
    match head {
        Some(hash) => println!("HEAD commit       {}", hash.to_hex()),
        None => println!("HEAD commit       (none)"),
    }
    match index {
        Some(state) => println!("Index view        {}", state.root_view.to_hex()),
        None => println!("Index view        (none)"),
    }
    match worktree {
        Some(state) => println!("Worktree view     {}", state.root_view.to_hex()),
        None => println!("Worktree view     (none)"),
    }

    let head_catalog = load_catalog(
        ws.store(),
        head.map(|hash| ws.get_commit(hash))
            .transpose()?
            .map(|commit| commit.view),
    )?;
    let index_catalog = match index {
        Some(state) => Some(Catalog::load(ws.store(), state.root_view)?),
        None => None,
    };
    let worktree_catalog = match worktree {
        Some(state) => Some(Catalog::load(ws.store(), state.root_view)?),
        None => None,
    };

    let mut staged = Vec::new();
    if let Some(index_catalog) = &index_catalog {
        staged = changed_objects(&ws, &head_catalog, index_catalog)?;
    }
    let mut unstaged = Vec::new();
    if let (Some(index_catalog), Some(worktree_catalog)) = (&index_catalog, &worktree_catalog) {
        unstaged = changed_objects(&ws, index_catalog, worktree_catalog)?;
    }

    let clean = staged.is_empty() && unstaged.is_empty();
    if !staged.is_empty() {
        println!();
        println!("Changes to be committed:");
        for (status, name) in &staged {
            println!("  {status} {name}");
        }
    }
    if !unstaged.is_empty() {
        println!();
        println!("Changes not staged for commit:");
        for (status, name) in &unstaged {
            println!("  {status} {name}");
        }
    }
    if clean {
        println!();
        println!("working tree clean");
    }
    Ok(())
}

fn changed_objects(ws: &Workspace, left: &Catalog, right: &Catalog) -> Result<Vec<(char, String)>> {
    let mut names = BTreeSet::new();
    names.extend(left.iter().map(|(name, _)| name));
    names.extend(right.iter().map(|(name, _)| name));
    let mut changes = Vec::new();
    for qname in names {
        let name = symbol_text(ws, qname);
        match (left.get(qname), right.get(qname)) {
            (None, Some(_)) => changes.push(('A', name)),
            (Some(_), None) => changes.push(('D', name)),
            (Some(left_object), Some(right_object)) => {
                let left_fields = object_fields(ws, left_object)?;
                let right_fields = object_fields(ws, right_object)?;
                if left_fields != right_fields {
                    changes.push(('M', name));
                }
            }
            (None, None) => {}
        }
    }
    Ok(changes)
}

fn verify(dir: Option<PathBuf>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let catalog = Catalog::load(ws.store(), state.root_view)?;
    catalog.verify(ws.store())?;
    println!("ok");
    Ok(())
}

fn commit(dir: Option<PathBuf>, message: &str, author: &str, allow_empty: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let branch = current_branch(&ws)?;
    let index = ws
        .read_index()?
        .context("index is not initialized; run `ds init`")?;
    let catalog = Catalog::load(ws.store(), index.root_view)?;
    catalog
        .verify(ws.store())
        .context("design-space verification failed")?;
    let parent = ws.read_ref(&branch)?.unwrap_or(CommitHash::ZERO);
    if !allow_empty && !parent.is_zero() {
        let parent_view = ws.get_commit(parent)?.view;
        if parent_view == index.root_view {
            bail!("nothing to commit; use --allow-empty");
        }
    }
    let commit = Commit {
        view: index.root_view,
        parent,
        message: message.to_owned(),
        author: author.to_owned(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64,
    };
    let hash = ws.put_commit(&commit)?;
    ws.write_ref(&branch, hash)?;
    ws.write_index(State {
        base_commit: hash,
        root_view: index.root_view,
    })?;
    if let Some(mut worktree) = ws.read_worktree()? {
        worktree.base_commit = hash;
        ws.write_worktree(worktree)?;
    }
    println!("Committed {} on branch {branch}", hash.to_hex());
    Ok(())
}

fn diff(dir: Option<PathBuf>, staged: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let branch = current_branch(&ws)?;
    let head_view = match ws.read_ref(&branch)? {
        Some(hash) => Some(ws.get_commit(hash)?.view),
        None => None,
    };
    let index = ws
        .read_index()?
        .context("index is not initialized; run `ds init`")?;
    let worktree = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let (left_label, left_view, right_label, right_view) = if staged {
        ("HEAD", head_view, "index", Some(index.root_view))
    } else {
        (
            "index",
            Some(index.root_view),
            "worktree",
            Some(worktree.root_view),
        )
    };
    let left = load_catalog(ws.store(), left_view)?;
    let right = load_catalog(ws.store(), right_view)?;

    let mut names = BTreeSet::new();
    names.extend(left.iter().map(|(name, _)| name));
    names.extend(right.iter().map(|(name, _)| name));
    let mut names = names
        .into_iter()
        .map(|name| (symbol_text(&ws, name), name))
        .collect::<Vec<_>>();
    names.sort();

    println!("--- {left_label}");
    println!("+++ {right_label}");
    let mut changed = false;
    for (name, qname) in names {
        match (left.get(qname), right.get(qname)) {
            (None, Some(_)) => {
                println!("A {name}");
                changed = true;
            }
            (Some(_), None) => {
                println!("D {name}");
                changed = true;
            }
            (Some(left_object), Some(right_object)) => {
                let left_fields = object_fields(&ws, left_object)?;
                let right_fields = object_fields(&ws, right_object)?;
                let mut keys = BTreeSet::new();
                keys.extend(left_fields.keys().copied());
                keys.extend(right_fields.keys().copied());
                let mut fields = Vec::new();
                for key in keys {
                    let before = left_fields.get(key).map(String::as_str).unwrap_or("<none>");
                    let after = right_fields
                        .get(key)
                        .map(String::as_str)
                        .unwrap_or("<none>");
                    if before != after {
                        fields.push((key, before.to_owned(), after.to_owned()));
                    }
                }
                if !fields.is_empty() {
                    changed = true;
                    println!("M {name}");
                    for (key, before, after) in fields {
                        println!("  {key}: {before} -> {after}");
                    }
                }
            }
            (None, None) => {}
        }
    }
    if !changed {
        println!("no changes");
    }
    Ok(())
}

fn load_catalog(store: &crate::store::Store, view: Option<ObjectHash>) -> Result<Catalog> {
    match view {
        Some(view) => Ok(Catalog::load(store, view)?),
        None => Ok(Catalog::new(GitOid::ZERO)),
    }
}

fn object_fields(ws: &Workspace, object: &DesignObject) -> Result<BTreeMap<&'static str, String>> {
    let mut fields = BTreeMap::new();
    fields.insert("kind", format!("{:?}", object.kind));
    match &object.payload {
        ObjectPayload::View(p) => {
            fields.insert("source_commit", p.source_commit.to_hex());
            fields.insert("files", p.files.to_hex());
            fields.insert("namespaces", p.namespaces.to_hex());
            fields.insert("design", format_hash(p.design.0));
        }
        ObjectPayload::Directory(p) => {
            fields.insert("parent", parent_ref_text(ws, p.parent));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| path_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::File(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| path_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::Namespace(p) => {
            fields.insert("parent", parent_ref_text(ws, p.parent));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| span_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::Class(p) => {
            fields.insert("parent", class_parent_text(ws, &p.parent));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| span_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert("superclass", symbol_text(ws, p.superclass));
            fields.insert("interfaces", symbol_list_text(ws, &p.interfaces));
            fields.insert("mixins", symbol_list_text(ws, &p.mixins));
            fields.insert(
                "explicit_dependencies",
                dependencies_text(ws, &p.explicit_dependencies)?,
            );
            fields.insert(
                "implicit_dependencies",
                dependencies_text(ws, &p.implicit_dependencies)?,
            );
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::Interface(p) => {
            fields.insert("parent", class_parent_text(ws, &p.parent));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| span_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert("extends", symbol_text(ws, p.extends));
            fields.insert(
                "explicit_dependencies",
                dependencies_text(ws, &p.explicit_dependencies)?,
            );
            fields.insert(
                "implicit_dependencies",
                dependencies_text(ws, &p.implicit_dependencies)?,
            );
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::Method(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| span_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert("overrides", symbol_text(ws, p.overrides));
            fields.insert(
                "explicit_dependencies",
                dependencies_text(ws, &p.explicit_dependencies)?,
            );
            fields.insert(
                "implicit_dependencies",
                dependencies_text(ws, &p.implicit_dependencies)?,
            );
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::Function(p) => {
            fields.insert("file", symbol_text(ws, p.file));
            fields.insert("namespace", symbol_text(ws, p.namespace));
            fields.insert("module", symbol_text(ws, p.module));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| span_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert(
                "explicit_dependencies",
                dependencies_text(ws, &p.explicit_dependencies)?,
            );
            fields.insert(
                "implicit_dependencies",
                dependencies_text(ws, &p.implicit_dependencies)?,
            );
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::Part(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("source", span_source_text(ws, &p.source));
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert(
                "explicit_dependencies",
                dependencies_text(ws, &p.explicit_dependencies)?,
            );
            fields.insert(
                "implicit_dependencies",
                dependencies_text(ws, &p.implicit_dependencies)?,
            );
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
        ObjectPayload::Mixin(p) => {
            fields.insert("parent", class_parent_text(ws, &p.parent));
            fields.insert(
                "source",
                p.source
                    .as_ref()
                    .map(|source| span_source_text(ws, source))
                    .unwrap_or_else(|| "<none>".to_owned()),
            );
            fields.insert("content_hash", format_hash(p.content_hash));
            fields.insert("on", symbol_list_text(ws, &p.on));
            fields.insert("superclass", symbol_text(ws, p.superclass));
            fields.insert("interfaces", symbol_list_text(ws, &p.interfaces));
            fields.insert("mixins", symbol_list_text(ws, &p.mixins));
            fields.insert(
                "explicit_dependencies",
                dependencies_text(ws, &p.explicit_dependencies)?,
            );
            fields.insert(
                "implicit_dependencies",
                dependencies_text(ws, &p.implicit_dependencies)?,
            );
            fields.insert("design", summarize_doc(ws, p.design_doc)?);
        }
    }
    Ok(fields)
}

fn symbol_text(ws: &Workspace, hash: SymbolHash) -> String {
    if hash.is_zero() {
        "<none>".to_owned()
    } else {
        ws.get_symbol(hash).unwrap_or_else(|_| hash.to_hex())
    }
}

fn symbol_list_text(ws: &Workspace, hashes: &[SymbolHash]) -> String {
    if hashes.is_empty() {
        return "<none>".to_owned();
    }
    hashes
        .iter()
        .map(|hash| symbol_text(ws, *hash))
        .collect::<Vec<_>>()
        .join(", ")
}

fn parent_ref_text(ws: &Workspace, parent: ParentRef) -> String {
    match parent {
        ParentRef::View => "view".to_owned(),
        ParentRef::Object(hash) => symbol_text(ws, hash),
    }
}

fn class_parent_text(ws: &Workspace, parent: &ClassParent) -> String {
    match parent {
        ClassParent::Nested { parent } => format!("nested {}", symbol_text(ws, *parent)),
        ClassParent::TopLevel {
            file,
            namespace,
            module,
        } => {
            let mut parts = vec![format!("namespace {}", symbol_text(ws, *namespace))];
            if !file.is_zero() {
                parts.push(format!("file {}", symbol_text(ws, *file)));
            }
            if !module.is_zero() {
                parts.push(format!("module {}", symbol_text(ws, *module)));
            }
            parts.join(", ")
        }
    }
}

fn path_source_text(ws: &Workspace, source: &PathSource) -> String {
    symbol_text(ws, source.path)
}

fn span_source_text(ws: &Workspace, source: &SpanSource) -> String {
    format!(
        "{}:{}-{}",
        symbol_text(ws, source.path),
        source.span.start_line,
        source.span.end_line
    )
}

fn format_hash(hash: Hash32) -> String {
    if hash.is_zero() {
        "<none>".to_owned()
    } else {
        hash.to_hex()
    }
}

fn summarize_doc(ws: &Workspace, hash: DocHash) -> Result<String> {
    if hash.is_zero() {
        return Ok("<none>".to_owned());
    }
    let text = ws.get_doc(hash)?;
    let mut summary = text.chars().take(100).collect::<String>();
    if text.chars().count() > 100 {
        summary.push('…');
    }
    Ok(format!("{summary:?}"))
}

fn dependencies_text(ws: &Workspace, dependencies: &[Dependency]) -> Result<String> {
    if dependencies.is_empty() {
        return Ok("<none>".to_owned());
    }
    let mut out = Vec::with_capacity(dependencies.len());
    for dependency in dependencies {
        let mut text = symbol_text(ws, dependency.target);
        if let Some(evidence) = dependency.evidence {
            text.push_str(&format!(
                " ({}:{}-{})",
                symbol_text(ws, evidence.path),
                evidence.span.start_line,
                evidence.span.end_line
            ));
        }
        if !dependency.rationale.is_zero() {
            text.push_str(&format!(
                " rationale={}",
                summarize_doc(ws, dependency.rationale)?
            ));
        }
        out.push(text);
    }
    Ok(out.join("; "))
}

fn rm(dir: Option<PathBuf>, name: &str, force: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    let qname = ws.put_symbol(name)?;
    if qname == symbol_hash("files") || qname == symbol_hash("namespaces") {
        bail!("cannot remove a root object");
    }
    if catalog.get(qname).is_none() {
        bail!("object not found: {name}");
    }
    let children = catalog.children(qname);
    if !children.is_empty() {
        let names = children
            .iter()
            .map(|child| symbol_text(&ws, *child))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("object has children; remove them first: {names}");
    }
    let references = catalog.references_to(qname);
    if !references.is_empty() {
        if !force {
            let names = references
                .iter()
                .map(|reference| symbol_text(&ws, *reference))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("object is referenced by: {names}");
        }
        let mut changed = Vec::new();
        for reference in references {
            if let Some(object) = catalog.get_mut(reference)
                && remove_references_to(object, qname)
            {
                changed.push(symbol_text(&ws, reference));
            }
        }
        changed.sort();
        for name in changed {
            println!("Unreferenced from {name}");
        }
    }
    catalog.remove(qname);
    let view = catalog.rebuild(ws.store())?;
    ws.write_worktree(State {
        base_commit: state.base_commit,
        root_view: view,
    })?;
    println!("Removed {name}");
    Ok(())
}

fn checkout(dir: Option<PathBuf>, branch: &str, create: bool, force: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let current = current_branch(&ws)?;
    if create {
        if ws.read_ref(branch)?.is_some() {
            bail!("branch already exists: {branch}");
        }
        let Some(head_hash) = ws.read_ref(&current)? else {
            bail!("current branch has no commit yet");
        };
        ws.write_ref(branch, head_hash)?;
        ws.write_head(&format!("ref: refs/heads/{branch}"))?;
        println!("Switched to new branch {branch}");
        return Ok(());
    }
    if current == branch {
        println!("Already on branch {branch}");
        return Ok(());
    }
    let commit_hash = ws
        .read_ref(branch)?
        .with_context(|| format!("branch not found: {branch}"))?;
    let commit = ws.get_commit(commit_hash)?;
    if !force && let Some(head_hash) = ws.read_ref(&current)? {
        let head_view = ws.get_commit(head_hash)?.view;
        if let Some(index) = ws.read_index()?
            && index.root_view != head_view
        {
            bail!("index has changes; commit, reset, or use --force");
        }
        if let Some(worktree) = ws.read_worktree()?
            && worktree.root_view != head_view
        {
            bail!("worktree has changes; commit, reset, or use --force");
        }
    }
    let state = State {
        base_commit: commit_hash,
        root_view: commit.view,
    };
    ws.write_head(&format!("ref: refs/heads/{branch}"))?;
    ws.write_index(state)?;
    ws.write_worktree(state)?;
    println!("Switched to branch {branch}");
    Ok(())
}

fn ls(
    dir: Option<PathBuf>,
    files: bool,
    namespaces: bool,
    design: bool,
    staged: bool,
) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = if staged {
        ws.read_index()?
            .context("index is not initialized; run `ds init`")?
    } else {
        ws.read_worktree()?
            .context("worktree is not initialized; run `ds init`")?
    };
    let catalog = Catalog::load(ws.store(), state.root_view)?;
    let mut rows = Vec::new();
    for (qname, object) in catalog.iter() {
        let selected = if files {
            matches!(object.kind, Kind::Directory | Kind::File)
        } else if namespaces {
            !matches!(object.kind, Kind::Directory | Kind::File)
        } else if design {
            design_doc_of(object).is_some_and(|doc| !doc.is_zero())
        } else {
            true
        };
        if selected {
            rows.push((symbol_text(&ws, qname), design_doc_of(object)));
        }
    }
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, design_doc) in rows {
        if design {
            println!("{name}\t{}", design_doc.unwrap_or(DocHash::ZERO).to_hex());
        } else {
            println!("{name}");
        }
    }
    Ok(())
}

fn design_doc_of(object: &DesignObject) -> Option<DocHash> {
    match &object.payload {
        ObjectPayload::View(_) => None,
        ObjectPayload::Directory(p) => Some(p.design_doc),
        ObjectPayload::File(p) => Some(p.design_doc),
        ObjectPayload::Namespace(p) => Some(p.design_doc),
        ObjectPayload::Class(p) => Some(p.design_doc),
        ObjectPayload::Interface(p) => Some(p.design_doc),
        ObjectPayload::Method(p) => Some(p.design_doc),
        ObjectPayload::Function(p) => Some(p.design_doc),
        ObjectPayload::Part(p) => Some(p.design_doc),
        ObjectPayload::Mixin(p) => Some(p.design_doc),
    }
}

fn show(dir: Option<PathBuf>, name: &str, source: bool, staged: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = if staged {
        ws.read_index()?
            .context("index is not initialized; run `ds init`")?
    } else {
        ws.read_worktree()?
            .context("worktree is not initialized; run `ds init`")?
    };
    let catalog = Catalog::load(ws.store(), state.root_view)?;
    let qname = ws.put_symbol(name)?;
    let object = catalog
        .get(qname)
        .with_context(|| format!("object not found: {name}"))?;
    println!("{name}");
    println!("kind: {:?}", object.kind);
    match &object.payload {
        ObjectPayload::View(_) => {}
        ObjectPayload::Directory(p) => {
            print_source_path(p.source, &ws)?;
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::File(p) => {
            print_source_path(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::Namespace(p) => {
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::Class(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_optional_symbol("superclass", p.superclass, &ws)?;
            print_symbol_list("interfaces", &p.interfaces, &ws)?;
            print_symbol_list("mixins", &p.mixins, &ws)?;
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::Interface(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_optional_symbol("extends", p.extends, &ws)?;
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::Method(p) => {
            print_optional_symbol("parent", p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_optional_symbol("overrides", p.overrides, &ws)?;
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::Function(p) => {
            print_optional_symbol("file", p.file, &ws)?;
            print_optional_symbol("namespace", p.namespace, &ws)?;
            print_optional_symbol("module", p.module, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::Part(p) => {
            print_optional_symbol("parent", p.parent, &ws)?;
            print_span_source(Some(p.source), &ws)?;
            print_content_hash(p.content_hash);
            print_design(&ws, p.design_doc)?;
        }
        ObjectPayload::Mixin(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_symbol_list("on", &p.on, &ws)?;
            print_optional_symbol("superclass", p.superclass, &ws)?;
            print_symbol_list("interfaces", &p.interfaces, &ws)?;
            print_symbol_list("mixins", &p.mixins, &ws)?;
            print_design(&ws, p.design_doc)?;
        }
    }
    let children = catalog.children(qname);
    if !children.is_empty() {
        let names = children
            .iter()
            .map(|child| symbol_text(&ws, *child))
            .collect::<Vec<_>>()
            .join(", ");
        println!("children: {names}");
    }
    if source {
        print_source_content(&ws, &catalog, object)?;
    }
    Ok(())
}

fn print_source_content(ws: &Workspace, catalog: &Catalog, object: &DesignObject) -> Result<()> {
    if catalog.source_commit().is_zero() {
        bail!("current view has no source commit");
    }
    let repo = SourceRepo::open(ws.meta_root(), ws.source_url(), ws.object_format())?;
    if !repo.exists() {
        bail!("source repository is missing; run `ds source fetch`");
    }
    let commit = repo.oid_hex(catalog.source_commit());
    let (path, span) = match &object.payload {
        ObjectPayload::View(_) => bail!("view has no source anchor"),
        ObjectPayload::Directory(p) => match p.source {
            Some(source) => (source.path, None),
            None => bail!("object has no source anchor"),
        },
        ObjectPayload::File(p) => match p.source {
            Some(source) => (source.path, None),
            None => bail!("object has no source anchor"),
        },
        ObjectPayload::Namespace(p) => span_anchor(p.source)?,
        ObjectPayload::Class(p) => span_anchor(p.source)?,
        ObjectPayload::Interface(p) => span_anchor(p.source)?,
        ObjectPayload::Method(p) => span_anchor(p.source)?,
        ObjectPayload::Function(p) => span_anchor(p.source)?,
        ObjectPayload::Part(p) => (p.source.path, Some(p.source.span)),
        ObjectPayload::Mixin(p) => span_anchor(p.source)?,
    };
    let path = ws.get_symbol(path)?;
    let bytes = repo.read_path(&commit, &path)?;
    let bytes = match span {
        Some(span) => bytes
            .get(span.start_byte as usize..span.end_byte as usize)
            .context("source span out of range")?,
        None => bytes.as_slice(),
    };
    std::io::stdout().write_all(bytes)?;
    Ok(())
}

fn span_anchor(source: Option<SpanSource>) -> Result<(SymbolHash, Option<Span>)> {
    match source {
        Some(source) => Ok((source.path, Some(source.span))),
        None => bail!("object has no source anchor"),
    }
}

fn log(dir: Option<PathBuf>, limit: usize) -> Result<()> {
    let ws = open_workspace(dir)?;
    let branch = current_branch(&ws)?;
    let mut current = ws.read_ref(&branch)?;
    let Some(mut hash) = current.take() else {
        println!("No commits on {branch}");
        return Ok(());
    };
    for _ in 0..limit {
        let commit = ws.get_commit(hash)?;
        println!("commit {}", hash.to_hex());
        println!("Author: {}", commit.author);
        println!("Date:   {}", commit.timestamp);
        println!();
        println!("    {}", commit.message);
        println!();
        if commit.parent.is_zero() {
            break;
        }
        hash = commit.parent;
    }
    Ok(())
}

fn gc(dir: Option<PathBuf>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let store = ws.store();
    store.expand_packs()?;
    let mut objects = HashSet::new();
    let mut commits = HashSet::new();
    let mut docs = HashSet::new();
    let mut symbols = HashSet::new();

    let mut roots = Vec::new();
    collect_hex_files(&ws.meta_root().join("refs"), &mut roots)?;
    for path in roots {
        let text = fs::read_to_string(&path)?;
        let hash = parse_hex(text.trim())?;
        if !hash.is_zero() {
            mark_commit(
                &ws,
                CommitHash(hash),
                &mut commits,
                &mut objects,
                &mut docs,
                &mut symbols,
            )?;
        }
    }

    for state in [ws.read_index()?, ws.read_worktree()?]
        .into_iter()
        .flatten()
    {
        if !state.base_commit.is_zero() {
            mark_commit(
                &ws,
                state.base_commit,
                &mut commits,
                &mut objects,
                &mut docs,
                &mut symbols,
            )?;
        }
        if !state.root_view.is_zero() {
            mark_view(&ws, state.root_view, &mut objects, &mut docs, &mut symbols)?;
        }
    }

    let removed_objects = prune(&store.root().join("objects"), &objects)?;
    let removed_commits = prune(&store.root().join("commits"), &commits)?;
    let removed_docs = prune(&store.root().join("docs"), &docs)?;
    let removed_symbols = prune(&store.root().join("symbols"), &symbols)?;
    let _ = fs::remove_dir_all(store.root().join("tmp"));
    fs::create_dir_all(store.root().join("tmp"))?;
    println!(
        "Removed {removed_objects} object(s), {removed_commits} commit(s), \
         {removed_docs} doc(s), {removed_symbols} symbol(s)"
    );
    Ok(())
}

fn repack(dir: Option<PathBuf>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let hash = ws.store().repack()?;
    println!("Packed store {}", hash.to_hex());
    Ok(())
}

fn mark_commit(
    ws: &Workspace,
    hash: CommitHash,
    commits: &mut HashSet<Hash32>,
    objects: &mut HashSet<Hash32>,
    docs: &mut HashSet<Hash32>,
    symbols: &mut HashSet<Hash32>,
) -> Result<()> {
    let mut current = hash;
    while !current.is_zero() {
        if !commits.insert(current.0) {
            break;
        }
        let commit = ws.get_commit(current)?;
        mark_view(ws, commit.view, objects, docs, symbols)?;
        current = commit.parent;
    }
    Ok(())
}

fn mark_view(
    ws: &Workspace,
    view_hash: ObjectHash,
    objects: &mut HashSet<Hash32>,
    docs: &mut HashSet<Hash32>,
    symbols: &mut HashSet<Hash32>,
) -> Result<()> {
    if view_hash.is_zero() || !objects.insert(view_hash.0) {
        return Ok(());
    }
    let view = ws.get_object(view_hash)?;
    let ObjectPayload::View(p) = &view.payload else {
        bail!("commit does not point at a view");
    };
    for root in [p.files, p.namespaces, p.design] {
        mark_object(ws, root, objects, docs, symbols)?;
    }
    Ok(())
}

fn mark_object(
    ws: &Workspace,
    hash: ObjectHash,
    objects: &mut HashSet<Hash32>,
    docs: &mut HashSet<Hash32>,
    symbols: &mut HashSet<Hash32>,
) -> Result<()> {
    if hash.is_zero() || !objects.insert(hash.0) {
        return Ok(());
    }
    let object = ws.get_object(hash)?;
    for symbol in object_symbols(&object) {
        if !symbol.is_zero() {
            symbols.insert(symbol.0);
        }
    }
    for doc in object_docs(&object) {
        if !doc.is_zero() {
            docs.insert(doc.0);
        }
    }
    for child in object_children(&object) {
        mark_object(ws, *child, objects, docs, symbols)?;
    }
    Ok(())
}

fn object_symbols(object: &DesignObject) -> Vec<SymbolHash> {
    let mut out = Vec::new();
    match &object.payload {
        ObjectPayload::View(_) => {}
        ObjectPayload::Directory(p) => {
            out.push(p.qualified_name);
            push_parent_ref(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
        }
        ObjectPayload::File(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
        }
        ObjectPayload::Namespace(p) => {
            out.push(p.qualified_name);
            push_parent_ref(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
        }
        ObjectPayload::Class(p) => {
            out.push(p.qualified_name);
            push_class_parent(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
            out.push(p.superclass);
            out.extend(p.interfaces.iter().copied());
            out.extend(p.mixins.iter().copied());
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
        ObjectPayload::Interface(p) => {
            out.push(p.qualified_name);
            push_class_parent(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
            out.push(p.extends);
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
        ObjectPayload::Method(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
            out.push(p.overrides);
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
        ObjectPayload::Function(p) => {
            out.push(p.qualified_name);
            out.push(p.file);
            out.push(p.namespace);
            out.push(p.module);
            if let Some(source) = p.source {
                out.push(source.path);
            }
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
        ObjectPayload::Part(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
            out.push(p.source.path);
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
        ObjectPayload::Mixin(p) => {
            out.push(p.qualified_name);
            push_class_parent(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
            out.extend(p.on.iter().copied());
            out.push(p.superclass);
            out.extend(p.interfaces.iter().copied());
            out.extend(p.mixins.iter().copied());
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
    }
    out.retain(|hash| !hash.is_zero());
    out
}

fn object_docs(object: &DesignObject) -> Vec<DocHash> {
    let mut out = Vec::new();
    match &object.payload {
        ObjectPayload::View(_) => {}
        ObjectPayload::Directory(p) => out.push(p.design_doc),
        ObjectPayload::File(p) => out.push(p.design_doc),
        ObjectPayload::Namespace(p) => out.push(p.design_doc),
        ObjectPayload::Class(p) => {
            out.push(p.design_doc);
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Interface(p) => {
            out.push(p.design_doc);
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Method(p) => {
            out.push(p.design_doc);
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Function(p) => {
            out.push(p.design_doc);
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Part(p) => {
            out.push(p.design_doc);
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Mixin(p) => {
            out.push(p.design_doc);
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
    }
    out.retain(|hash| !hash.is_zero());
    out
}

fn object_children(object: &DesignObject) -> &[ObjectHash] {
    match &object.payload {
        ObjectPayload::Directory(p) => &p.children,
        ObjectPayload::File(p) => &p.children,
        ObjectPayload::Namespace(p) => &p.children,
        ObjectPayload::Class(p) => &p.children,
        ObjectPayload::Interface(p) => &p.children,
        ObjectPayload::Part(p) => &p.children,
        ObjectPayload::Mixin(p) => &p.children,
        _ => &[],
    }
}

fn push_parent_ref(out: &mut Vec<SymbolHash>, parent: ParentRef) {
    if let ParentRef::Object(parent) = parent {
        out.push(parent);
    }
}

fn push_class_parent(out: &mut Vec<SymbolHash>, parent: ClassParent) {
    match parent {
        ClassParent::TopLevel {
            file,
            namespace,
            module,
        } => {
            out.push(file);
            out.push(namespace);
            out.push(module);
        }
        ClassParent::Nested { parent } => out.push(parent),
    }
}

fn push_deps(out: &mut Vec<SymbolHash>, deps: &[Dependency]) {
    for dep in deps {
        out.push(dep.target);
        if let Some(evidence) = dep.evidence {
            out.push(evidence.path);
        }
    }
}

fn collect_hex_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_hex_files(&path, out)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_hex(text: &str) -> Result<Hash32> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid hash: {text}");
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in text.as_bytes().chunks(2).enumerate() {
        bytes[index] = u8::from_str_radix(std::str::from_utf8(chunk)?, 16)?;
    }
    Ok(Hash32(bytes))
}

fn prune(dir: &Path, marked: &HashSet<Hash32>) -> Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut files = Vec::new();
    collect_hex_files(dir, &mut files)?;
    let mut removed = 0;
    for path in files {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("hash filename")?;
        let hash = parse_hex(name)?;
        if !marked.contains(&hash) {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn open_workspace(dir: Option<PathBuf>) -> Result<Workspace> {
    let meta = locate_meta(dir)?;
    Ok(Workspace::open(meta)?)
}

fn locate_meta(dir: Option<PathBuf>) -> Result<PathBuf> {
    let start = dir.unwrap_or(std::env::current_dir()?);
    let mut current = start.as_path();
    loop {
        let candidate = current.join(".ds");
        if candidate.join("config").is_file() {
            return Ok(candidate);
        }
        current = current.parent().context("no .ds workspace found")?;
    }
}

fn current_branch(ws: &Workspace) -> Result<String> {
    let head = ws.read_head()?;
    let target = head
        .strip_prefix("ref: refs/heads/")
        .context("HEAD is not a branch")?;
    Ok(target.to_owned())
}

fn build_object(ws: &Workspace, catalog: &Catalog, args: &CreateArgs) -> Result<DesignObject> {
    let design_doc = read_design(ws, args)?;
    let object = match args.kind {
        KindArg::Directory => {
            let parent =
                required_parent_kind(ws, catalog, args.parent.as_deref(), &[Kind::Directory])?;
            let qname = child_qname(ws, parent, &args.name)?;
            let source = read_source_path(ws, args.source.as_deref())?;
            let path_source = source.map(|path| PathSource { path });
            DesignObject::new(
                Kind::Directory,
                ObjectPayload::Directory(DirectoryPayload {
                    qualified_name: qname,
                    parent: ParentRef::Object(parent),
                    source: path_source,
                    design_doc,
                    children: vec![],
                }),
            )
        }
        KindArg::File => {
            let parent =
                required_parent_kind(ws, catalog, args.parent.as_deref(), &[Kind::Directory])?;
            let qname = child_qname(ws, parent, &args.name)?;
            let source = read_source(ws, args.source.as_deref(), false)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::File,
                ObjectPayload::File(FilePayload {
                    qualified_name: qname,
                    parent,
                    source: source.map(|s| PathSource { path: s.path }),
                    content_hash: content,
                    design_doc,
                    children: vec![],
                }),
            )
        }
        KindArg::Namespace => {
            let parent =
                required_parent_kind(ws, catalog, args.parent.as_deref(), &[Kind::Namespace])?;
            let qname = child_qname(ws, parent, &args.name)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Namespace,
                ObjectPayload::Namespace(NamespacePayload {
                    qualified_name: qname,
                    parent: ParentRef::Object(parent),
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    design_doc,
                    children: vec![],
                }),
            )
        }
        KindArg::Class => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Class,
                ObjectPayload::Class(ClassPayload {
                    qualified_name: qname,
                    parent,
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    design_doc,
                    superclass: relation_symbol(ws, args.superclass.as_deref())?,
                    interfaces: relation_symbols(ws, &args.interfaces)?,
                    mixins: relation_symbols(ws, &args.mixins)?,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::Interface => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Interface,
                ObjectPayload::Interface(InterfacePayload {
                    qualified_name: qname,
                    parent,
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    design_doc,
                    extends: relation_symbol(ws, args.extends.as_deref())?,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::Method => {
            let parent = required_parent_kind(
                ws,
                catalog,
                args.parent.as_deref(),
                &[Kind::Class, Kind::Interface, Kind::Mixin],
            )?;
            let signature = args
                .signature
                .as_deref()
                .context("method requires --signature")?;
            let qname_text = format!(
                "{}/{}#{}",
                ws.get_symbol(parent)?,
                args.name,
                method_signature_hash(signature)
            );
            let qname = ws.put_symbol(&qname_text)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Method,
                ObjectPayload::Method(MethodPayload {
                    qualified_name: qname,
                    parent,
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    design_doc,
                    overrides: relation_symbol(ws, args.overrides.as_deref())?,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                }),
            )
        }
        KindArg::Function => {
            let namespace = required_namespace(ws, catalog, args.namespace.as_deref())?;
            let qname_text = format!("{}/{}", ws.get_symbol(namespace)?, args.name);
            let qname = ws.put_symbol(&qname_text)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Function,
                ObjectPayload::Function(FunctionPayload {
                    qualified_name: qname,
                    file: optional_ref(ws, args.file.as_deref())?,
                    namespace,
                    module: optional_ref(ws, args.module.as_deref())?,
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    design_doc,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                }),
            )
        }
        KindArg::Part => {
            let parent = required_parent(ws, catalog, args.parent.as_deref())?;
            let source = read_source(ws, args.source.as_deref(), true)?
                .context("part requires --source path:start-end")?;
            if !source.has_range {
                bail!("part requires --source path:start-end");
            }
            let qname_text = format!(
                "{}/part#{}",
                ws.get_symbol(parent)?,
                source.content_hash.to_hex()
            );
            let qname = ws.put_symbol(&qname_text)?;
            DesignObject::new(
                Kind::Part,
                ObjectPayload::Part(PartPayload {
                    qualified_name: qname,
                    parent,
                    source: source.span_source(),
                    content_hash: source.content_hash,
                    design_doc,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::Mixin => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Mixin,
                ObjectPayload::Mixin(MixinPayload {
                    qualified_name: qname,
                    parent,
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    design_doc,
                    on: relation_symbols(ws, &args.on)?,
                    superclass: relation_symbol(ws, args.superclass.as_deref())?,
                    interfaces: relation_symbols(ws, &args.interfaces)?,
                    mixins: relation_symbols(ws, &args.mixins)?,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
    };
    format::validate_object(&object)?;
    Ok(object)
}

fn validate_create_args(args: &CreateArgs) -> Result<()> {
    match args.kind {
        KindArg::Class | KindArg::Interface | KindArg::Mixin => {
            if args.parent.is_some() {
                if args.namespace.is_some() || args.file.is_some() || args.module.is_some() {
                    bail!(
                        "a nested type uses --parent; do not also pass                          --namespace/--file/--module"
                    );
                }
            } else if args.namespace.is_none() {
                bail!("a top-level type requires --namespace");
            }
        }
        KindArg::Function => {
            if args.parent.is_some() {
                bail!("function does not take --parent; use --namespace");
            }
        }
        KindArg::Method | KindArg::Part => {
            if args.namespace.is_some() || args.file.is_some() || args.module.is_some() {
                bail!("{} does not take --namespace/--file/--module", match args.kind {
                    KindArg::Method => "method",
                    _ => "part",
                });
            }
        }
        KindArg::Directory | KindArg::File | KindArg::Namespace => {
            if args.namespace.is_some() || args.file.is_some() || args.module.is_some() {
                bail!("{} does not take --namespace/--file/--module", match args.kind {
                    KindArg::Directory => "directory",
                    KindArg::File => "file",
                    _ => "namespace",
                });
            }
        }
    }
    Ok(())
}

fn code_qname_and_parent(
    ws: &Workspace,
    catalog: &Catalog,
    args: &CreateArgs,
) -> Result<(SymbolHash, ClassParent)> {
    if let Some(parent) = args.parent.as_deref() {
        let parent = ws.put_symbol(parent)?;
        let parent_object = catalog
            .get(parent)
            .with_context(|| format!("parent not found: {parent:?}"))?;
        if !matches!(
            parent_object.kind,
            Kind::Class | Kind::Interface | Kind::Mixin
        ) {
            bail!("a nested type parent must be a class, interface, or mixin: {parent:?}");
        }
        let qname_text = format!("{}/{}", ws.get_symbol(parent)?, args.name);
        let qname = ws.put_symbol(&qname_text)?;
        Ok((qname, ClassParent::Nested { parent }))
    } else {
        let namespace = required_namespace(ws, catalog, args.namespace.as_deref())?;
        let qname_text = format!("{}/{}", ws.get_symbol(namespace)?, args.name);
        let qname = ws.put_symbol(&qname_text)?;
        Ok((
            qname,
            ClassParent::TopLevel {
                file: optional_ref(ws, args.file.as_deref())?,
                namespace,
                module: optional_ref(ws, args.module.as_deref())?,
            },
        ))
    }
}

fn required_parent(ws: &Workspace, catalog: &Catalog, text: Option<&str>) -> Result<SymbolHash> {
    let text = text.context("--parent is required")?;
    let parent = ws.put_symbol(text)?;
    if catalog.get(parent).is_none() {
        bail!("parent not found: {text}");
    }
    Ok(parent)
}

fn required_parent_kind(
    ws: &Workspace,
    catalog: &Catalog,
    text: Option<&str>,
    allowed: &[Kind],
) -> Result<SymbolHash> {
    let parent = required_parent(ws, catalog, text)?;
    let object = catalog
        .get(parent)
        .with_context(|| format!("parent not found: {}", text.unwrap_or_default()))?;
    if !allowed.contains(&object.kind) {
        bail!(
            "parent {} has kind {:?}; expected one of {:?}",
            text.unwrap_or_default(),
            object.kind,
            allowed
        );
    }
    Ok(parent)
}

fn required_namespace(ws: &Workspace, catalog: &Catalog, text: Option<&str>) -> Result<SymbolHash> {
    let text = text.context("--namespace is required")?;
    let namespace = ws.put_symbol(text)?;
    let object = catalog
        .get(namespace)
        .with_context(|| format!("namespace not found: {text}"))?;
    if object.kind != Kind::Namespace {
        bail!("not a namespace: {text}");
    }
    Ok(namespace)
}

fn child_qname(ws: &Workspace, parent: SymbolHash, name: &str) -> Result<SymbolHash> {
    let qname = format!("{}/{}", ws.get_symbol(parent)?, name);
    Ok(ws.put_symbol(&qname)?)
}

fn optional_ref(ws: &Workspace, text: Option<&str>) -> Result<SymbolHash> {
    match text {
        Some("") => Ok(SymbolHash::ZERO),
        Some(text) => Ok(ws.put_symbol(text)?),
        None => Ok(SymbolHash::ZERO),
    }
}

fn relation_symbol(ws: &Workspace, text: Option<&str>) -> Result<SymbolHash> {
    match text {
        Some(text) if !text.trim().is_empty() => Ok(ws.put_symbol(text)?),
        _ => Ok(SymbolHash::ZERO),
    }
}

fn relation_symbols(ws: &Workspace, names: &[String]) -> Result<Vec<SymbolHash>> {
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        if !name.trim().is_empty() {
            out.push(ws.put_symbol(name)?);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn ordered_relation_symbols(ws: &Workspace, names: &[String]) -> Result<Vec<SymbolHash>> {
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        if !name.trim().is_empty() {
            out.push(ws.put_symbol(name)?);
        }
    }
    out.dedup();
    Ok(out)
}

fn read_design(ws: &Workspace, args: &CreateArgs) -> Result<DocHash> {
    let text = match (&args.design, &args.design_file) {
        (Some(text), None) => Some(text.clone()),
        (None, Some(path)) => Some(fs::read_to_string(path)?),
        (None, None) => None,
        (Some(_), Some(_)) => bail!("use either --design or --design-file"),
    };
    match text {
        Some(text) => Ok(ws.put_doc(&text)?),
        None => Ok(DocHash::ZERO),
    }
}

#[derive(Debug)]
struct SourceSpec {
    path: SymbolHash,
    content_hash: Hash32,
    start_byte: u64,
    end_byte: u64,
    start_line: u32,
    end_line: u32,
    has_range: bool,
}

impl SourceSpec {
    fn span_source(&self) -> SpanSource {
        SpanSource {
            path: self.path,
            span: Span {
                start_byte: self.start_byte,
                end_byte: self.end_byte,
                start_line: self.start_line,
                end_line: self.end_line,
            },
        }
    }
}

fn read_source(ws: &Workspace, spec: Option<&str>, span: bool) -> Result<Option<SourceSpec>> {
    let Some(spec) = spec else {
        return Ok(None);
    };
    let (path, range) = split_source(spec);
    let repo = SourceRepo::open(ws.meta_root(), ws.source_url(), ws.object_format())?;
    let bytes = if repo.exists() {
        let commit = repo.resolve_current()?;
        repo.read_path(&commit, path)
            .with_context(|| format!("read source {path} at {commit}"))?
    } else {
        fs::read(path).with_context(|| format!("read source {path}"))?
    };
    let (start_byte, end_byte, start_line, end_line, has_range) = match range {
        Some((start, end)) => {
            let starts = line_starts(&bytes);
            if start == 0 || start > starts.len() || end < start || end > starts.len() {
                bail!("invalid source line range: {spec}");
            }
            let start_byte = starts[start - 1];
            let end_byte = if end < starts.len() {
                starts[end]
            } else {
                bytes.len()
            };
            (start_byte, end_byte, start as u32, end as u32, true)
        }
        None => {
            if span {
                let lines = line_starts(&bytes).len() as u32;
                (0, bytes.len(), 1, lines.max(1), false)
            } else {
                (0, bytes.len(), 0, 0, false)
            }
        }
    };
    let hash = content_hash(&bytes[start_byte..end_byte]);
    Ok(Some(SourceSpec {
        path: ws.put_symbol(path)?,
        content_hash: hash,
        start_byte: start_byte as u64,
        end_byte: end_byte as u64,
        start_line,
        end_line,
        has_range,
    }))
}

/// Directory sources only record the path; a directory has no blob to read.
fn read_source_path(ws: &Workspace, spec: Option<&str>) -> Result<Option<SymbolHash>> {
    let Some(spec) = spec else {
        return Ok(None);
    };
    let (path, _) = split_source(spec);
    Ok(Some(ws.put_symbol(path)?))
}

fn split_source(spec: &str) -> (&str, Option<(usize, usize)>) {
    if let Some((path, suffix)) = spec.rsplit_once(':')
        && let Some((start, end)) = suffix.split_once('-')
        && let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>())
    {
        return (path, Some((start, end)));
    }
    (spec, None)
}

fn line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn method_signature_hash(signature: &str) -> String {
    let hash = symbol_hash(signature);
    hash.to_hex()[..16].to_owned()
}

fn object_qname(object: &DesignObject) -> Option<SymbolHash> {
    match &object.payload {
        ObjectPayload::View(_) => None,
        ObjectPayload::Directory(p) => Some(p.qualified_name),
        ObjectPayload::File(p) => Some(p.qualified_name),
        ObjectPayload::Namespace(p) => Some(p.qualified_name),
        ObjectPayload::Class(p) => Some(p.qualified_name),
        ObjectPayload::Interface(p) => Some(p.qualified_name),
        ObjectPayload::Method(p) => Some(p.qualified_name),
        ObjectPayload::Function(p) => Some(p.qualified_name),
        ObjectPayload::Part(p) => Some(p.qualified_name),
        ObjectPayload::Mixin(p) => Some(p.qualified_name),
    }
}

fn print_source_path(source: Option<PathSource>, ws: &Workspace) -> Result<()> {
    if let Some(source) = source {
        println!("source path: {}", ws.get_symbol(source.path)?);
    }
    Ok(())
}

fn print_span_source(source: Option<SpanSource>, ws: &Workspace) -> Result<()> {
    if let Some(source) = source {
        println!(
            "source: {}:{}-{} bytes {}-{}",
            ws.get_symbol(source.path)?,
            source.span.start_line,
            source.span.end_line,
            source.span.start_byte,
            source.span.end_byte
        );
    }
    Ok(())
}

fn print_content_hash(hash: Hash32) {
    if !hash.is_zero() {
        println!("content hash: {}", hash.to_hex());
    }
}

fn print_design(ws: &Workspace, doc: DocHash) -> Result<()> {
    if !doc.is_zero() {
        println!("design:\n{}", ws.get_doc(doc)?);
    }
    Ok(())
}

fn print_optional_symbol(label: &str, symbol: SymbolHash, ws: &Workspace) -> Result<()> {
    if !symbol.is_zero() {
        println!("{label}: {}", ws.get_symbol(symbol)?);
    }
    Ok(())
}

fn print_symbol_list(label: &str, symbols: &[SymbolHash], ws: &Workspace) -> Result<()> {
    if !symbols.is_empty() {
        let names = symbols
            .iter()
            .map(|symbol| ws.get_symbol(*symbol))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        println!("{label}: {}", names.join(", "));
    }
    Ok(())
}

fn print_class_parent(parent: ClassParent, ws: &Workspace) -> Result<()> {
    match parent {
        ClassParent::TopLevel {
            file,
            namespace,
            module,
        } => {
            print_optional_symbol("file", file, ws)?;
            print_optional_symbol("namespace", namespace, ws)?;
            print_optional_symbol("module", module, ws)?;
        }
        ClassParent::Nested { parent } => print_optional_symbol("parent", parent, ws)?,
    }
    Ok(())
}
