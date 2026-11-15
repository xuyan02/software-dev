use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use clap::{Parser, Subcommand};
use design_space::branch;
use design_space::element::Element;
use design_space::error::{Error, Result};
use design_space::gc;
use design_space::hash::{from_hex, is_zero, to_commit_hex, to_hex};
use design_space::node::Node;
use design_space::pack::{self, Pack};
use design_space::remote;
use design_space::source::{SourceRef, SourceRepo};
use design_space::space::{
    self, CommitOptions, collect_view, load_version, parse_commit_hex, resolve_version, view_root,
};
use design_space::store::Store;
use design_space::types::{T_CONCEPT, T_FILE, kind_of, language_of, type_name};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(
    name = "ds",
    version,
    about = "A git-like, versioned design space for conceptual design elements"
)]
struct Cli {
    /// Suppress progress messages on stderr.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    /// Print per-file progress on stderr.
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

static VERBOSITY: AtomicU8 = AtomicU8::new(1);

fn progress(level: u8, message: impl std::fmt::Display) {
    if VERBOSITY.load(Ordering::Relaxed) >= level {
        eprintln!("{message}");
    }
}

fn short_hex(text: &str) -> &str {
    &text[..text.len().min(12)]
}

/// Resolve the metadata directory of the design workspace.
///
/// `--dir` names a workspace root: the directory `ds init` was run in.
/// Without it, the current directory and its parents are searched for
/// `.design`, so commands work from any subdirectory of the workspace.
fn locate_workspace(dir: Option<PathBuf>) -> Result<PathBuf> {
    match dir {
        Some(root) => workspace_meta(&root),
        None => {
            let start = std::env::current_dir()?;
            let mut current = start.as_path();
            loop {
                if let Some(candidate) = workspace_candidate(current)? {
                    branch::ensure_initialized(&candidate)?;
                    return Ok(candidate);
                }
                if current.file_name().is_some_and(|name| name == ".ds") && is_meta_dir(current) {
                    branch::ensure_initialized(current)?;
                    return Ok(current.to_path_buf());
                }
                match current.parent() {
                    Some(parent) => current = parent,
                    None => break,
                }
            }
            Err(Error::Config(format!(
                "no design workspace found from {}; run `ds init` or pass --dir",
                start.display()
            )))
        }
    }
}

fn workspace_meta(root: &Path) -> Result<PathBuf> {
    let meta = if let Some(nested) = workspace_candidate(root)? {
        nested
    } else if is_meta_dir(root) || is_meta_dir(&root.join(".ds")) {
        if is_meta_dir(&root.join(".ds")) {
            let ds = root.join(".ds");
            if !ds.exists() {
                fs::rename(root.join(".ds"), &ds)?;
            }
            ds
        } else {
            root.to_path_buf()
        }
    } else {
        return Err(Error::Config(format!(
            "{} is not a design workspace; run `ds init` there",
            root.display()
        )));
    };
    branch::ensure_initialized(&meta)?;
    Ok(meta)
}

fn is_meta_dir(dir: &Path) -> bool {
    dir.join("config").is_file() && dir.join("objects").is_dir()
}

/// `.ds`, migrating `.design` from older workspaces once.
fn workspace_candidate(root: &Path) -> Result<Option<PathBuf>> {
    let current = root.join(".ds");
    if is_meta_dir(&current) {
        return Ok(Some(current));
    }
    let legacy = root.join(".ds");
    if is_meta_dir(&legacy) {
        fs::rename(&legacy, &current)?;
        return Ok(Some(current));
    }
    Ok(None)
}

#[derive(Subcommand)]
enum Command {
    /// Clone an existing design space from its remote.
    Clone {
        /// Design-space URL, e.g. `ds://host/<project-id>`.
        url: String,
        /// Directory to create (default: the last path segment of the URL).
        directory: Option<PathBuf>,
        /// Code repository this design space describes. Defaults to the
        /// source recorded by the design space server, when it has one.
        #[arg(long)]
        source: Option<String>,
        /// Design branch to check out (default: remote main, master, or the
        /// only branch).
        #[arg(long)]
        branch: Option<String>,
    },
    /// Create a new local design-space repository.
    Init {
        /// Code repository this design space describes. Immutable after init.
        #[arg(long)]
        source: String,
    },
    /// Show or set the design-space remote.
    Remote {
        /// Workspace root (default: nearest `.ds` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
        #[command(subcommand)]
        command: RemoteCommand,
    },
    /// Create one conceptual element and advance the current branch.
    Create {
        /// Unique element name in this branch.
        name: String,
        /// Design text. Omit to create the element undesigned.
        #[arg(long, conflicts_with = "design_file")]
        design: Option<String>,
        /// Read the design text from this file.
        #[arg(long)]
        design_file: Option<PathBuf>,
        /// Optional source anchor path.
        #[arg(long)]
        path: Option<String>,
        /// Source line range START-END (or one line); requires --path.
        #[arg(long, requires = "path")]
        lines: Option<String>,
        /// Optional parent element name. Inferred from `::` names otherwise.
        #[arg(long)]
        parent: Option<String>,
        /// Workspace root (default: nearest `.ds` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// List local branches, create one, or delete one.
    Branch {
        /// Create this branch without switching to it.
        name: Option<String>,
        /// Delete this branch.
        #[arg(short = 'd', long)]
        delete: Option<String>,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Switch the current local branch.
    Checkout {
        name: String,
        /// Create the branch and switch to it.
        #[arg(short = 'b')]
        create: bool,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Inspect or update the source git repository.
    Source {
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long, global = true)]
        dir: Option<PathBuf>,
        #[command(subcommand)]
        command: SourceCommand,
    },
    /// List the elements of the current branch.
    Status {
        /// Only elements that still have no design.
        #[arg(long)]
        missing: bool,
        /// Only elements whose file path contains this text.
        #[arg(long)]
        path: Option<String>,
        /// Only elements with this type label, e.g. `rust:function`.
        #[arg(long = "type")]
        ty: Option<String>,
        /// Print at most this many elements.
        #[arg(long)]
        limit: Option<usize>,
        /// Include file objects (they have no design).
        #[arg(long)]
        files: bool,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Write the design of one element; advances the branch.
    Describe {
        name: String,
        #[arg(long)]
        design: Option<String>,
        #[arg(long)]
        design_file: Option<PathBuf>,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Print one element and its design.
    Show {
        name: String,
        #[arg(long)]
        at: Option<String>,
        /// Print only the element's source text, straight from the git blob.
        #[arg(long)]
        source: bool,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// List elements in the namespaces view, or files in the files view.
    List {
        #[arg(long)]
        at: Option<String>,
        /// Use the files view (file objects and their elements).
        #[arg(long)]
        files: bool,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Walk the version history.
    Log {
        #[arg(long)]
        at: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Pack every loose node into one pack file.
    Pack {
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Verify a pack file.
    Verify { pack: PathBuf },
    /// Prune unreachable nodes and repack everything reachable.
    Gc {
        /// Workspace root (default: nearest `.ds` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Repack every node into one pack without pruning.
    Repack {
        /// Workspace root (default: nearest `.ds` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Push a design branch to origin.
    Push {
        /// Design branch to push; defaults to the current branch.
        #[arg(long)]
        branch: Option<String>,
        /// Workspace root (default: nearest `.ds` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Fetch a design branch from origin into refs/remotes/origin.
    Fetch {
        /// Design branch to fetch; defaults to the current branch.
        #[arg(long)]
        branch: Option<String>,
        /// Workspace root (default: nearest `.ds` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Fetch and fast-forward the current design branch.
    Pull {
        /// Design branch to pull; defaults to the current branch.
        #[arg(long)]
        branch: Option<String>,
        /// Workspace root (default: nearest `.ds` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Print one node by hash.
    Cat {
        hash: String,
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// List loose nodes.
    Ls {
        /// Workspace root (default: nearest `.design` from the current directory).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum RemoteCommand {
    /// Print the configured remote.
    Show,
    /// Set origin to this design-space URL.
    Add { url: String },
    /// Replace the configured origin URL.
    SetUrl { url: String },
}

#[derive(Subcommand)]
enum SourceCommand {
    /// Show the current source ref and its commit.
    Status,
    /// List remote branches known to the local clone.
    Branch,
    /// Fetch the source remote. Never runs implicitly.
    Fetch,
    /// Switch the source to a branch, tag or commit.
    Checkout { git_ref: String },
    /// Show source commit history, newest first.
    Log {
        /// Local source branch, tag or commit; defaults to the current ref.
        git_ref: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    source: SourceConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote: Option<RemoteConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SourceConfig {
    url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteConfig {
    name: String,
    url: String,
}

fn main() -> Result<()> {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    let verbosity = if cli.quiet {
        0
    } else if cli.verbose {
        2
    } else {
        1
    };
    VERBOSITY.store(verbosity, Ordering::Relaxed);

    match cli.command {
        Command::Clone {
            url,
            directory,
            source,
            branch,
        } => clone_space(
            &url,
            directory.as_deref(),
            source.as_deref(),
            branch.as_deref(),
        ),
        Command::Init { source } => init(&source),
        Command::Remote { dir, command } => remote_command(&locate_workspace(dir)?, command),
        Command::Create {
            name,
            design,
            design_file,
            path,
            lines,
            parent,
            dir,
        } => create(
            &locate_workspace(dir)?,
            &name,
            design,
            design_file.as_deref(),
            path.as_deref(),
            lines.as_deref(),
            parent.as_deref(),
        ),
        Command::Branch { name, delete, dir } => {
            branch_command(&locate_workspace(dir)?, name.as_deref(), delete.as_deref())
        }
        Command::Checkout { name, create, dir } => checkout(&locate_workspace(dir)?, &name, create),
        Command::Source { dir, command } => source_command(&locate_workspace(dir)?, command),
        Command::Status {
            missing,
            path,
            ty,
            limit,
            files,
            dir,
        } => status(
            &locate_workspace(dir)?,
            missing,
            path.as_deref(),
            ty.as_deref(),
            limit,
            files,
        ),
        Command::Describe {
            name,
            design,
            design_file,
            dir,
        } => describe(
            &locate_workspace(dir)?,
            &name,
            design,
            design_file.as_deref(),
        ),
        Command::Show {
            name,
            at,
            source,
            dir,
        } => show(&locate_workspace(dir)?, &name, at.as_deref(), source),
        Command::List { at, files, dir } => list(&locate_workspace(dir)?, at.as_deref(), files),
        Command::Log { at, limit, dir } => log(&locate_workspace(dir)?, at.as_deref(), limit),
        Command::Pack { dir, output } => pack_loose(&locate_workspace(dir)?, output.as_deref()),
        Command::Verify { pack } => verify(&pack),
        Command::Gc { dir } => gc_cmd(&locate_workspace(dir)?),
        Command::Repack { dir } => repack_cmd(&locate_workspace(dir)?),
        Command::Push { branch, dir } => push_cmd(&locate_workspace(dir)?, branch.as_deref()),
        Command::Fetch { branch, dir } => {
            fetch_cmd(&locate_workspace(dir)?, branch.as_deref(), false)
        }
        Command::Pull { branch, dir } => {
            fetch_cmd(&locate_workspace(dir)?, branch.as_deref(), true)
        }
        Command::Cat { hash, dir } => cat(&locate_workspace(dir)?, &hash),
        Command::Ls { dir } => list_loose(&locate_workspace(dir)?),
    }
}

fn clone_space(
    url: &str,
    directory: Option<&Path>,
    source: Option<&str>,
    branch: Option<&str>,
) -> Result<()> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::Config("design-space URL must not be empty".into()));
    }
    let mut transport = remote::open_transport(url)?;
    let remote_info = transport.refs_info()?;
    let branch = match branch {
        Some(name) => {
            branch::validate_name(name)?;
            if !remote_info.refs.is_empty() && !remote_info.refs.contains_key(name) {
                return Err(Error::Config(format!(
                    "remote branch {name} does not exist in {url}"
                )));
            }
            name.to_owned()
        }
        None => pick_clone_branch(url, &remote_info.refs)?,
    };
    let source = source
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            remote_info
                .source
                .map(|source| source.trim().to_owned())
                .filter(|source| !source.is_empty())
        })
        .ok_or_else(|| {
            Error::Config(format!(
                "design space {url} does not record its source repository; pass --source <git-url>"
            ))
        })?;
    let dir = match directory {
        Some(dir) => dir.to_path_buf(),
        None => PathBuf::from(default_clone_directory(url)),
    };
    let existed = dir.exists();
    if existed && fs::read_dir(&dir)?.next().is_some() {
        return Err(Error::Config(format!(
            "destination {} already exists and is not empty",
            dir.display()
        )));
    }

    println!("Cloning design space {url} into {}", dir.display());
    fs::create_dir_all(&dir)?;
    let original = std::env::current_dir()?;
    let outcome = (|| -> Result<()> {
        std::env::set_current_dir(&dir)?;
        let setup = create_workspace(&source, Some(url))?;
        println!("Source {} at branch {}", source, setup.default_branch);
        let meta = locate_workspace(None)?;
        if branch::current(&meta)? != branch {
            checkout(&meta, &branch, true)?;
        }
        if remote_info.refs.is_empty() {
            println!("Design space is empty; nothing to pull.");
            return Ok(());
        }
        fetch_cmd(&meta, Some(&branch), true)
    })();
    std::env::set_current_dir(&original)?;
    if outcome.is_err() && !existed {
        let _ = fs::remove_dir_all(&dir);
    }
    if outcome.is_ok() {
        let workspace = fs::canonicalize(&dir)
            .unwrap_or_else(|_| dir.clone())
            .join(".ds");
        println!("Design workspace: {}", workspace.display());
    }
    outcome
}

/// Pick the remote branch a fresh clone should check out.
fn pick_clone_branch(url: &str, refs: &BTreeMap<String, String>) -> Result<String> {
    for candidate in [branch::DEFAULT, "master"] {
        if refs.contains_key(candidate) {
            return Ok(candidate.to_owned());
        }
    }
    match refs.len() {
        0 => Ok(branch::DEFAULT.to_owned()),
        1 => Ok(refs.keys().next().cloned().unwrap_or_default()),
        _ => Err(Error::Config(format!(
            "design space {url} has several branches ({}); pass --branch",
            refs.keys().cloned().collect::<Vec<_>>().join(", ")
        ))),
    }
}

/// Derive a directory name from the last path segment of a URL.
fn default_clone_directory(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let path = after_scheme
        .split_once('/')
        .map(|(_, path)| path)
        .unwrap_or("");
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if last.is_empty() || last == "." || last == ".." {
        "design-space".to_owned()
    } else {
        last.to_owned()
    }
}

struct WorkspaceSetup {
    default_branch: String,
}

/// Create a new local workspace in `./.ds`. Shared by `init` and `clone`;
/// only `init` announces it as an initialization.
fn create_workspace(source: &str, remote: Option<&str>) -> Result<WorkspaceSetup> {
    let base = Path::new(".ds");
    if base.join("config").exists() {
        return Err(Error::Config(".ds is already initialized".into()));
    }
    if source.trim().is_empty() {
        return Err(Error::Config("--source must not be empty".into()));
    }

    for dir in [
        "objects",
        "packs",
        "refs/heads",
        "refs/remotes",
        "tmp",
        "cache",
    ] {
        fs::create_dir_all(base.join(dir))?;
    }
    branch::set_current(base, branch::DEFAULT)?;
    branch::ensure_exists(base, branch::DEFAULT)?;

    let source_repo = SourceRepo::open(base, source)?;
    progress(1, format!("cloning {}...", source.trim()));
    source_repo.clone_bare()?;
    let default_branch = source_repo.default_branch()?;
    source_repo.set_current(&SourceRef::Branch(default_branch.clone()))?;

    let config = Config {
        source: SourceConfig {
            url: source.trim().to_owned(),
        },
        remote: remote.map(|url| RemoteConfig {
            name: "origin".to_owned(),
            url: url.to_owned(),
        }),
    };
    save_config(base, &config)?;
    // If the workspace root is itself a git worktree, keep the design state
    // out of `git status`. `.gitignore` here ignores every entry, itself too.
    fs::write(base.join(".gitignore"), "*\n")?;
    Ok(WorkspaceSetup { default_branch })
}

fn init(source: &str) -> Result<()> {
    let setup = create_workspace(source, None)?;
    println!(
        "Initialized ds workspace in {}",
        std::env::current_dir()?.join(".ds").display()
    );
    println!(
        "Source {} at branch {}",
        source.trim(),
        setup.default_branch
    );
    Ok(())
}

fn remote_command(dir: &Path, command: RemoteCommand) -> Result<()> {
    let mut config = load_config(dir)?;
    match command {
        RemoteCommand::Show => match &config.remote {
            Some(remote) => println!("{}", remote.url),
            None => println!("No remote configured."),
        },
        RemoteCommand::Add { url } => {
            let url = remote_url_arg(&url)?;
            if config.remote.is_some() {
                return Err(Error::Config(
                    "remote origin already exists; use `ds remote set-url <url>`".into(),
                ));
            }
            config.remote = Some(RemoteConfig {
                name: "origin".to_owned(),
                url: url.clone(),
            });
            save_config(dir, &config)?;
            println!("Origin {url}");
        }
        RemoteCommand::SetUrl { url } => {
            let url = remote_url_arg(&url)?;
            if config.remote.is_none() {
                return Err(Error::Config(
                    "no remote configured; run `ds remote add <url>`".into(),
                ));
            }
            config.remote = Some(RemoteConfig {
                name: "origin".to_owned(),
                url: url.clone(),
            });
            save_config(dir, &config)?;
            println!("Origin {url}");
        }
    }
    Ok(())
}

fn remote_url_arg(url: &str) -> Result<String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::Config("remote URL must not be empty".into()));
    }
    Ok(url.to_owned())
}

fn save_config(dir: &Path, config: &Config) -> Result<()> {
    let text = toml::to_string_pretty(config).map_err(|error| Error::Config(error.to_string()))?;
    fs::write(dir.join("config"), text)?;
    Ok(())
}

fn create(
    dir: &Path,
    name: &str,
    design: Option<String>,
    design_file: Option<&Path>,
    path: Option<&str>,
    lines: Option<&str>,
    parent: Option<&str>,
) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Config("element name must not be empty".into()));
    }
    let text = match (design, design_file) {
        (Some(text), None) => Some(text),
        (None, Some(path)) => Some(fs::read_to_string(path)?),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("clap rejects both flags"),
    };
    let (start_line, end_line) = match lines {
        Some(value) => parse_line_range(value)?,
        None => (0, 0),
    };

    let branch = branch::current(dir)?;
    let store = Store::new(dir);
    let mut state = branch_state(&store, dir, &branch)?;
    if state.elements.iter().any(|element| element.name == name) {
        return Err(Error::Config(format!("element {name} already exists")));
    }

    let mut element = Element {
        name: name.to_owned(),
        ty: T_CONCEPT,
        path: path.unwrap_or_default().to_owned(),
        commit: to_commit_hex(&state.source_commit),
        start_byte: 0,
        end_byte: 0,
        start_line,
        end_line,
        own_text: String::new(),
        parent: None,
        children: Vec::new(),
        design: text,
        content_hash: None,
    };

    let parent_name = match parent {
        Some(parent) => {
            if !state.elements.iter().any(|element| element.name == parent) {
                return Err(Error::NotFound(format!("parent element {parent}")));
            }
            Some(parent.to_owned())
        }
        None => infer_parent(name, &state.elements),
    };
    if let Some(parent_name) = &parent_name {
        element.parent = Some(parent_name.clone());
        if let Some(parent) = state
            .elements
            .iter_mut()
            .find(|element| &element.name == parent_name)
        {
            parent.children.push(name.to_owned());
        }
    }
    state.elements.push(element);

    let result = space::commit(
        &store,
        dir,
        &state.elements,
        &CommitOptions {
            message: format!("create {name}"),
            author: "agent".into(),
            source_commit: current_source_commit(dir, state.source_commit),
            pipeline: AGENT_PIPELINE.into(),
            ref_name: branch.clone(),
        },
    )?;
    println!("Created {name} on branch {branch}");
    if let Some(parent) = &parent_name {
        println!("Parent {parent}");
    }
    println!(
        "Created version {} ({} missing design(s))",
        short_hex(&to_hex(&result.version)),
        result.missing
    );
    Ok(())
}

const AGENT_PIPELINE: &str = "agent/v1";

/// The source commit the agent is currently reading, when the source clone is
/// available. Falls back to the branch's previous version.
fn current_source_commit(dir: &Path, fallback: [u8; 32]) -> [u8; 32] {
    let Ok(config) = load_config(dir) else {
        return fallback;
    };
    let Ok(source) = SourceRepo::open(dir, &config.source.url) else {
        return fallback;
    };
    let Ok(commit) = source.resolve_current() else {
        return fallback;
    };
    parse_commit_hex(&commit).unwrap_or(fallback)
}

struct BranchState {
    elements: Vec<Element>,
    source_commit: [u8; 32],
}

/// Current elements with their parent/child links. Versions store hashes and
/// spans, not links; a local tree cache normally has them. After a clone the
/// cache is absent, so links are reconstructed from `::` names.
fn branch_state(store: &Store, dir: &Path, branch_name: &str) -> Result<BranchState> {
    let Some(version) = space::read_ref_optional(dir, branch_name)? else {
        return Ok(BranchState {
            elements: Vec::new(),
            source_commit: [0u8; 32],
        });
    };
    let version_node = load_version(store, &version)?;
    let elements = match space::elements_at(store, dir, &version, true) {
        Ok(elements) => elements,
        Err(_) => {
            let mut elements = space::elements_at(store, dir, &version, false)?;
            link_names(&mut elements);
            elements
        }
    };
    Ok(BranchState {
        elements,
        source_commit: version_node.source_commit,
    })
}

fn link_names(elements: &mut [Element]) {
    let names: std::collections::BTreeSet<String> = elements
        .iter()
        .map(|element| element.name.clone())
        .collect();
    let mut parents: BTreeMap<String, Option<String>> = BTreeMap::new();
    for element in elements.iter() {
        let parts: Vec<&str> = element.name.split("::").collect();
        let mut prefix = String::new();
        let mut found = None;
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !prefix.is_empty() {
                prefix.push_str("::");
            }
            prefix.push_str(part);
            if names.contains(&prefix) {
                found = Some(prefix.clone());
            }
        }
        parents.insert(element.name.clone(), found);
    }
    for element in elements.iter_mut() {
        if element.parent.is_none() {
            element.parent = parents.get(&element.name).cloned().flatten();
        }
        element.children.clear();
    }
    let links: Vec<(String, Option<String>)> = elements
        .iter()
        .map(|element| (element.name.clone(), element.parent.clone()))
        .collect();
    for (child, parent) in links {
        if let Some(parent) = parent
            && let Some(element) = elements.iter_mut().find(|element| element.name == parent)
        {
            element.children.push(child);
        }
    }
}

fn infer_parent(name: &str, elements: &[Element]) -> Option<String> {
    let parts: Vec<&str> = name.split("::").collect();
    let mut prefix = String::new();
    let mut found = None;
    for part in &parts[..parts.len().saturating_sub(1)] {
        if !prefix.is_empty() {
            prefix.push_str("::");
        }
        prefix.push_str(part);
        if elements.iter().any(|element| element.name == prefix) {
            found = Some(prefix.clone());
        }
    }
    found
}

fn parse_line_range(value: &str) -> Result<(u32, u32)> {
    let (start, end) = match value.split_once('-') {
        Some((start, end)) => (start, end),
        None => (value, value),
    };
    let start = start
        .trim()
        .parse::<u32>()
        .map_err(|error| Error::Config(format!("invalid line range {value}: {error}")))?;
    let end = end
        .trim()
        .parse::<u32>()
        .map_err(|error| Error::Config(format!("invalid line range {value}: {error}")))?;
    if start == 0 || end < start {
        return Err(Error::Config(format!("invalid line range {value}")));
    }
    Ok((start, end))
}

fn load_config(dir: &Path) -> Result<Config> {
    let text = fs::read_to_string(dir.join("config"))?;
    toml::from_str(&text).map_err(|error| Error::Config(error.to_string()))
}

fn status(
    dir: &Path,
    missing_only: bool,
    path_filter: Option<&str>,
    type_filter: Option<&str>,
    limit: Option<usize>,
    include_files: bool,
) -> Result<()> {
    let branch = branch::current(dir)?;
    let store = Store::new(dir);
    let Some(version) = space::read_ref_optional(dir, &branch)? else {
        println!("On branch {branch}");
        println!("No versions yet; run `ds create`.");
        return Ok(());
    };
    let elements = space::elements_at(&store, dir, &version, false)?;
    let total = elements.len();
    let mut matched = 0usize;
    let mut shown = 0usize;

    println!("On branch {branch}");
    println!(
        "Version {} (source {})",
        short_hex(&to_hex(&version)),
        short_hex(&to_commit_hex(
            &load_version(&store, &version)?.source_commit
        ))
    );

    for element in &elements {
        let is_file = element.ty == T_FILE;
        if is_file && !include_files {
            continue;
        }
        let needs_design = !is_file && element.design.is_none();
        if missing_only && !needs_design {
            continue;
        }
        if let Some(filter) = path_filter
            && !element.path.contains(filter)
        {
            continue;
        }
        if let Some(filter) = type_filter
            && type_label(element.ty) != filter
        {
            continue;
        }
        matched += 1;
        if limit.is_some_and(|limit| shown >= limit) {
            continue;
        }
        let state = if is_file {
            "file"
        } else if element.design.is_some() {
            "designed"
        } else {
            "not designed"
        };
        println!(
            "  {}  {}  {}:{}-{}  {}",
            type_label(element.ty),
            element.name,
            element.path,
            element.start_line,
            element.end_line,
            state
        );
        shown += 1;
    }

    let missing = elements
        .iter()
        .filter(|element| element.ty != T_FILE && element.design.is_none())
        .count();
    if matched == total {
        println!("{total} element(s), {missing} missing design(s)");
    } else {
        println!("{matched} matching element(s) ({shown} shown), {missing} missing design(s)");
    }
    Ok(())
}

fn describe(
    dir: &Path,
    name: &str,
    design: Option<String>,
    design_file: Option<&Path>,
) -> Result<()> {
    let text = match (design, design_file) {
        (Some(text), None) => text,
        (None, Some(path)) => fs::read_to_string(path)?,
        (Some(_), Some(_)) => {
            return Err(Error::Config(
                "use either --design or --design-file, not both".into(),
            ));
        }
        (None, None) => {
            return Err(Error::Config("missing --design or --design-file".into()));
        }
    };
    let branch = branch::current(dir)?;
    let store = Store::new(dir);
    space::resolve_version(&store, dir, None)?;
    let state = branch_state(&store, dir, &branch)?;
    let source_commit = state.source_commit;
    let mut elements = state.elements;
    let element = elements
        .iter_mut()
        .find(|element| element.name == name)
        .ok_or_else(|| Error::NotFound(name.to_owned()))?;
    if element.ty == T_FILE {
        return Err(Error::Config(format!(
            "{name} is a file object and has no design"
        )));
    }
    element.design = Some(text);

    let result = space::commit(
        &store,
        dir,
        &elements,
        &CommitOptions {
            message: format!("describe {name}"),
            author: "ds".into(),
            source_commit: current_source_commit(dir, source_commit),
            pipeline: AGENT_PIPELINE.into(),
            ref_name: branch.clone(),
        },
    )?;
    println!("Described {name} on branch {branch}");
    println!(
        "Created version {} ({} missing design(s))",
        short_hex(&to_hex(&result.version)),
        result.missing
    );
    Ok(())
}

fn branch_command(dir: &Path, create: Option<&str>, delete: Option<&str>) -> Result<()> {
    if create.is_some() && delete.is_some() {
        return Err(Error::Config(
            "use either a branch name or --delete, not both".into(),
        ));
    }
    if let Some(name) = delete {
        branch::delete(dir, name)?;
        println!("Deleted branch {name}");
        return Ok(());
    }
    if let Some(name) = create {
        branch::create_unborn(dir, name)?;
        println!("Created branch {name}");
        return Ok(());
    }

    let current = branch::current(dir)?;
    for name in branch::list(dir)? {
        let marker = if name == current { "*" } else { " " };
        let version = space::read_ref_optional(dir, &name)?
            .map(|hash| short_hex(&to_hex(&hash)).to_owned())
            .unwrap_or_else(|| "-".to_owned());
        println!("{marker} {name} {version}");
    }
    Ok(())
}

fn checkout(dir: &Path, name: &str, create: bool) -> Result<()> {
    if !branch::exists(dir, name) {
        if !create {
            return Err(Error::Config(format!(
                "branch {name} does not exist; pass -b to create it"
            )));
        }
        branch::create(dir, name)?;
        println!("Switched to a new branch '{name}'");
        return Ok(());
    }
    if create {
        return Err(Error::Config(format!("branch {name} already exists")));
    }
    branch::set_current(dir, name)?;
    println!("Switched to branch '{name}'");
    Ok(())
}

fn source_command(dir: &Path, command: SourceCommand) -> Result<()> {
    let config = load_config(dir)?;
    let source = SourceRepo::open(dir, &config.source.url)?;

    match command {
        SourceCommand::Status => {
            if !source.exists() {
                return Err(Error::Config(
                    "source repository is missing; run `ds source fetch`".into(),
                ));
            }
            let current = source.current()?;
            let commit = source.resolve_current()?;
            println!("Source {}", config.source.url);
            match &current {
                SourceRef::Branch(name) => println!("On branch {name}"),
                SourceRef::Commit(_) => {
                    println!("HEAD detached at {}", short_hex(&commit))
                }
            }
            println!("Commit {commit}");
            Ok(())
        }
        SourceCommand::Branch => {
            if !source.exists() {
                return Err(Error::Config(
                    "source repository is missing; run `ds source fetch`".into(),
                ));
            }
            let current = source.current()?;
            for entry in source.branches()? {
                let name = entry
                    .name
                    .strip_prefix("origin/")
                    .unwrap_or(entry.name.as_str());
                let marker = match &current {
                    SourceRef::Branch(branch) if branch == name => "* ",
                    _ => "  ",
                };
                println!("{marker}{name}");
            }
            if let SourceRef::Commit(commit) = &current {
                let short = &commit[..commit.len().min(7)];
                println!("* (HEAD detached at {short})");
            }
            Ok(())
        }
        SourceCommand::Fetch => {
            if source.exists() {
                progress(1, format!("fetching {}...", config.source.url));
                source.fetch()?;
            } else {
                progress(1, format!("cloning {}...", config.source.url));
                source.clone_bare()?;
            }
            let current = source.current()?;
            println!("Fetched {}", config.source.url);
            match &current {
                SourceRef::Branch(name) => println!("On branch {name}"),
                SourceRef::Commit(commit) => {
                    println!("HEAD detached at {}", short_hex(commit))
                }
            }
            Ok(())
        }
        SourceCommand::Checkout { git_ref } => {
            if !source.exists() {
                return Err(Error::Config(
                    "source repository is missing; run `ds source fetch`".into(),
                ));
            }
            let target = source.checkout(&git_ref)?;
            let commit = source.resolve_current()?;
            match &target {
                SourceRef::Branch(name) => println!("Switched source to branch '{name}'"),
                SourceRef::Commit(_) => {
                    println!("Switched source to commit {}", short_hex(&commit))
                }
            }
            println!("Commit {commit}");
            Ok(())
        }
        SourceCommand::Log { git_ref, limit } => {
            if !source.exists() {
                return Err(Error::Config(
                    "source repository is missing; run `ds source fetch`".into(),
                ));
            }
            let commit = match git_ref {
                Some(reference) => source.resolve_source(&reference)?,
                None => source.resolve_current()?,
            };
            let entries = source.log(&commit, limit)?;
            for entry in &entries {
                println!("commit {}", short_hex(&entry.commit));
                println!("Author: {}", entry.author);
                println!("Date:   {}", entry.timestamp);
                println!();
                println!("    {}", entry.subject);
                println!();
            }
            Ok(())
        }
    }
}

fn show(dir: &Path, name: &str, at: Option<&str>, source_only: bool) -> Result<()> {
    let store = Store::new(dir);
    let version = resolve_version(&store, dir, at)?;
    let (object, design) = space::lookup_object(&store, &version, name)?;

    if source_only {
        if object.ty == T_FILE {
            return print_whole_file(dir, &to_commit_hex(&object.commit), &object.path);
        }
        if object.path.is_empty() || (object.start_byte == 0 && object.end_byte == 0) {
            return Err(Error::Config(format!(
                "{name} has no source anchor; create it with --path and --lines"
            )));
        }
        return print_source(
            dir,
            &to_commit_hex(&object.commit),
            &object.path,
            object.start_byte,
            object.end_byte,
        );
    }

    println!("{} {}", type_label(object.ty), object.name);
    println!("  file:    {}", object.path);
    println!(
        "  lines:   {}-{} (bytes {}..{})",
        object.start_line, object.end_line, object.start_byte, object.end_byte
    );
    println!("  commit:  {}", to_commit_hex(&object.commit));
    println!("  content: {}", to_hex(&object.content_hash));
    println!("  input:   {}", to_hex(&object.input_hash));
    if design.text.is_empty() {
        println!("  design:  (none)");
    } else {
        println!("  design:");
        print!("{}", design.text);
        if !design.text.ends_with('\n') {
            println!();
        }
    }
    Ok(())
}

/// Print exactly the bytes the element spans in its git blob.
fn print_source(dir: &Path, commit: &str, path: &str, start: u64, end: u64) -> Result<()> {
    let config = load_config(dir)?;
    let source = SourceRepo::open(dir, &config.source.url)?;
    let bytes = source.read_path(commit, path)?;

    let start = usize::try_from(start)
        .map_err(|_| Error::Config(format!("span start {start} is too large")))?;
    let end =
        usize::try_from(end).map_err(|_| Error::Config(format!("span end {end} is too large")))?;
    let slice = bytes.get(start..end).ok_or_else(|| {
        Error::Config(format!(
            "span {start}..{end} is outside {path} ({} bytes) at {commit}",
            bytes.len()
        ))
    })?;
    print_bytes(slice)
}

/// Print a whole file object from its git blob.
fn print_whole_file(dir: &Path, commit: &str, path: &str) -> Result<()> {
    let config = load_config(dir)?;
    let source = SourceRepo::open(dir, &config.source.url)?;
    let bytes = source.read_path(commit, path)?;
    print_bytes(&bytes)
}

fn print_bytes(bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| Error::Config(format!("source is not UTF-8: {error}")))?;
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn list(dir: &Path, at: Option<&str>, files: bool) -> Result<()> {
    let store = Store::new(dir);
    let version = resolve_version(&store, dir, at)?;
    let version_node = load_version(&store, &version)?;
    let view_name = if files { "files" } else { "namespaces" };
    let view = view_root(&store, &version_node, view_name.as_bytes())?
        .ok_or_else(|| Error::InvalidNode(format!("version has no {view_name} view")))?;

    let mut entries = Vec::new();
    collect_view(&store, &view, &mut entries)?;
    for (key, _) in &entries {
        let key = String::from_utf8_lossy(key).replace('\0', " ");
        println!("{key}");
    }
    Ok(())
}

fn log(dir: &Path, at: Option<&str>, limit: usize) -> Result<()> {
    let store = Store::new(dir);
    let mut current = resolve_version(&store, dir, at)?;

    for _ in 0..limit {
        let version = load_version(&store, &current)?;
        println!("commit {}", short_hex(&to_hex(&current)));
        println!("Source: {}", to_commit_hex(&version.source_commit));
        println!("Date:   {}", version.timestamp);
        println!();
        println!("    {}", version.message);
        println!();
        if is_zero(&version.parent) {
            break;
        }
        current = version.parent;
    }
    Ok(())
}

fn pack_loose(dir: &Path, output: Option<&Path>) -> Result<()> {
    let store = Store::new(dir);
    let nodes = if output.is_some() {
        progress(1, "loading all nodes...");
        store.load_all()?
    } else {
        progress(1, "loading loose nodes...");
        store.load_loose()?
    };
    if nodes.is_empty() {
        println!("No nodes to pack.");
        return Ok(());
    }
    progress(1, format!("packing {} node(s)...", nodes.len()));
    let written = pack::write(&nodes)?;

    let destination = match output {
        Some(path) => path.to_path_buf(),
        None => dir
            .join("packs")
            .join(format!("{}.pack", to_hex(&written.checksum))),
    };
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&destination, &written.bytes)?;

    if output.is_some() {
        println!(
            "Wrote {} node(s) to {} (export; loose objects kept)",
            written.count,
            destination.display()
        );
        return Ok(());
    }

    let index = pack::PackIndex::open(&destination)?;
    for node in &nodes {
        let hash = node.hash();
        if !index.contains(&hash) {
            return Err(Error::InvalidPack(format!(
                "new pack misses {}",
                to_hex(&hash)
            )));
        }
    }
    let removed = store.remove_loose()?;
    println!(
        "Wrote pack {} ({} node(s)); removed {removed} loose node(s)",
        short_hex(&to_hex(&written.checksum)),
        written.count
    );
    Ok(())
}

fn verify(path: &Path) -> Result<()> {
    let pack = Pack::open(path)?;
    println!(
        "Pack OK: {} node(s), checksum {}",
        pack.len(),
        short_hex(&to_hex(pack.checksum()))
    );
    Ok(())
}

fn gc_cmd(dir: &Path) -> Result<()> {
    let store = Store::new(dir);
    let report = gc::gc(&store, dir)?;
    println!("Collected {} reachable node(s)", report.reachable);
    match &report.pack {
        Some(checksum) => {
            println!(
                "Wrote pack {} ({} node(s))",
                short_hex(checksum),
                report.pack_nodes
            );
        }
        None => println!("No reachable nodes; no pack written"),
    }
    println!(
        "Removed {} pack(s), {} loose node(s), {} stale cache file(s)",
        report.removed_packs, report.removed_loose, report.removed_cache
    );
    Ok(())
}

fn repack_cmd(dir: &Path) -> Result<()> {
    let store = Store::new(dir);
    let report = gc::repack(&store, dir)?;
    match &report.pack {
        Some(checksum) => println!(
            "Packed {} node(s) into {}",
            report.nodes,
            short_hex(checksum)
        ),
        None => println!("No nodes to pack"),
    }
    println!(
        "Removed {} pack(s), {} loose node(s)",
        report.removed_packs, report.removed_loose
    );
    Ok(())
}

fn remote_url(dir: &Path) -> Result<String> {
    load_config(dir)?
        .remote
        .map(|remote| remote.url)
        .ok_or_else(|| Error::Config("no remote configured; run `ds remote add <url>`".into()))
}

fn remote_branch(dir: &Path, branch: Option<&str>) -> Result<String> {
    match branch {
        Some(name) => {
            branch::validate_name(name)?;
            Ok(name.to_owned())
        }
        None => branch::current(dir),
    }
}

fn push_cmd(dir: &Path, branch: Option<&str>) -> Result<()> {
    let branch = remote_branch(dir, branch)?;
    let url = remote_url(dir)?;
    let store = Store::new(dir);
    let mut transport = remote::open_transport(&url)?;
    progress(1, format!("pushing {branch} to {url}..."));
    let summary = remote::push(transport.as_mut(), &store, dir, &branch)?;
    if summary.up_to_date {
        println!("Everything up-to-date on branch {branch}.");
    } else {
        println!(
            "Pushed {branch} to origin: {} ({} node(s))",
            short_hex(&summary.version),
            summary.nodes
        );
    }
    Ok(())
}

fn fetch_cmd(dir: &Path, branch: Option<&str>, pull: bool) -> Result<()> {
    let branch = remote_branch(dir, branch)?;
    let url = remote_url(dir)?;
    let store = Store::new(dir);
    let mut transport = remote::open_transport(&url)?;
    progress(
        1,
        format!(
            "{} {branch} from {url}...",
            if pull { "pulling" } else { "fetching" }
        ),
    );
    let summary = remote::fetch(transport.as_mut(), &store, dir, &branch, pull)?;
    if summary.up_to_date {
        println!("Already up to date on branch {branch}.");
    } else if pull {
        println!(
            "Fast-forwarded {branch} to {} ({} node(s))",
            short_hex(&summary.version),
            summary.nodes
        );
    } else {
        println!(
            "Fetched {branch} from origin: {} ({} node(s))",
            short_hex(&summary.version),
            summary.nodes
        );
    }
    Ok(())
}

fn cat(dir: &Path, hash: &str) -> Result<()> {
    let address = from_hex(hash)?;
    let node = Store::new(dir).get(&address)?;
    print_node(&node);
    Ok(())
}

fn list_loose(dir: &Path) -> Result<()> {
    let store = Store::new(dir);
    let mut nodes = store.load_all()?;
    nodes.sort_by(|a, b| {
        let left = (a.ty(), node_name(a), to_hex(&a.hash()));
        let right = (b.ty(), node_name(b), to_hex(&b.hash()));
        left.cmp(&right)
    });
    for node in &nodes {
        let hash = to_hex(&node.hash());
        println!(
            "{} {} {}",
            type_label(node.ty()),
            if node_name(node).is_empty() {
                "-"
            } else {
                node_name(node)
            },
            short_hex(&hash)
        );
    }
    Ok(())
}

fn print_node(node: &Node) {
    let hash = to_hex(&node.hash());
    match node {
        Node::Object(object) => {
            println!("{} {}", type_label(node.ty()), object.name);
            println!("  file:    {}", object.path);
            println!(
                "  lines:   {}-{} (bytes {}..{})",
                object.start_line, object.end_line, object.start_byte, object.end_byte
            );
            println!("  commit:  {}", to_commit_hex(&object.commit));
            println!("  content: {}", to_hex(&object.content_hash));
            println!("  input:   {}", to_hex(&object.input_hash));
            println!("  design:  {}", to_hex(&object.design_hash));
        }
        Node::Design(design) => {
            println!("design {}", short_hex(&hash));
            println!("  length:  {}", design.text.len());
        }
        Node::Index(index) => {
            println!(
                "index {} ({})",
                short_hex(&hash),
                if index.leaf { "leaf" } else { "internal" }
            );
            println!("  entries: {}", index.entries.len());
        }
        Node::Version(version) => {
            println!("version {}", short_hex(&hash));
            println!("  parent:  {}", short_hex(&to_hex(&version.parent)));
            println!("  root:    {}", to_hex(&version.root));
            println!("  source:  {}", to_commit_hex(&version.source_commit));
            println!("  pipeline: {}", version.pipeline);
            println!("  date:    {}", version.timestamp);
        }
    }
}

fn type_label(ty: u32) -> String {
    if let Some(name) = type_name(ty) {
        return name.to_owned();
    }
    format!("unknown/{:04x}:{:04x}", language_of(ty), kind_of(ty))
}

fn node_name(node: &Node) -> &str {
    match node {
        Node::Object(object) => &object.name,
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_clone_directories_from_urls() {
        assert_eq!(
            default_clone_directory("ds://xuyan@localhost/ad86e6c58f818f9f"),
            "ad86e6c58f818f9f"
        );
        assert_eq!(
            default_clone_directory("http://host:7000/v1/projects/foo/"),
            "foo"
        );
        assert_eq!(default_clone_directory("ssh://host/srv/ds/demo"), "demo");
        assert_eq!(default_clone_directory("ds://host/"), "design-space");
    }
}
