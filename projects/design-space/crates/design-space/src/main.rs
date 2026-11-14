use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use clap::{Parser, Subcommand};
use design_space::branch;
use design_space::element::Element;
use design_space::error::{Error, Result};
use design_space::gc;
use design_space::hash::{from_hex, is_zero, to_commit_hex, to_hex};
use design_space::language;
use design_space::node::Node;
use design_space::pack::{self, Pack};
use design_space::remote;
use design_space::source::{SourceRef, SourceRepo};
use design_space::space::{
    self, CommitOptions, collect_view, load_version, parse_commit_hex, resolve_version, view_root,
};
use design_space::store::Store;
use design_space::types::{T_FILE, kind_of, language_of, type_name};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(
    name = "ds",
    version,
    about = "A git-like, versioned design space for code elements"
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

fn source_label_human(label: &str) -> String {
    if let Some(name) = label.strip_prefix("branch:") {
        format!("Source branch {name}")
    } else if let Some(commit) = label.strip_prefix("commit:") {
        format!("Source commit {}", short_hex(commit))
    } else {
        format!("Source {label}")
    }
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
    /// Create a local design-space repository.
    Init {
        /// Code repository this design space describes. Immutable after init.
        #[arg(long)]
        source: String,
        /// Optional remote design-space URL, stored as origin.
        #[arg(long)]
        remote: Option<String>,
    },
    /// Scan the current source ref into the current design branch.
    Scan {
        /// Workspace root (default: nearest `.design` from the current directory).
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
    /// Serve a design-space repository over HTTP or over SSH stdio.
    Serve {
        /// Server repository directory (created if missing).
        #[arg(long)]
        repo: PathBuf,
        /// HTTP listen address, e.g. 127.0.0.1:7000.
        #[arg(long)]
        listen: Option<String>,
        /// Speak the framed protocol on stdin/stdout (for sshd / ssh).
        #[arg(long)]
        stdio: bool,
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
        Command::Init { source, remote } => init(&source, remote.as_deref()),
        Command::Scan { dir } => scan(&locate_workspace(dir)?),
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
        Command::Serve {
            repo,
            listen,
            stdio,
        } => serve(&repo, listen.as_deref(), stdio),
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

fn init(source: &str, remote: Option<&str>) -> Result<()> {
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
            url: source.to_owned(),
        },
        remote: remote.map(|url| RemoteConfig {
            name: "origin".to_owned(),
            url: url.to_owned(),
        }),
    };
    let text = toml::to_string_pretty(&config).map_err(|error| Error::Config(error.to_string()))?;
    fs::write(base.join("config"), text)?;
    // If the workspace root is itself a git worktree, keep the design state
    // out of `git status`. `.gitignore` here ignores every entry, itself too.
    fs::write(base.join(".gitignore"), "*\n")?;

    println!(
        "Initialized ds workspace in {}",
        std::env::current_dir()?.join(".ds").display()
    );
    println!("Source {source} at branch {default_branch}");
    if let Some(url) = remote {
        println!("Origin {url}");
    }
    Ok(())
}

fn scan(dir: &Path) -> Result<()> {
    let branch = branch::current(dir)?;

    let config = load_config(dir)?;
    let source = SourceRepo::open(dir, &config.source.url)?;
    if !source.exists() {
        return Err(Error::Config(
            "source repository is missing; run `ds source fetch`".into(),
        ));
    }

    let current = source.current()?;
    let source_label = current.label();
    let commit = source.resolve_current()?;
    parse_commit_hex(&commit)?;
    progress(1, format!("source {source_label} -> {commit}"));

    let store = Store::new(dir);
    let base_version = space::read_ref_optional(dir, &branch)?;
    let base_commit = match &base_version {
        Some(version) => {
            let node = load_version(&store, version)?;
            Some(to_commit_hex(&node.source_commit))
        }
        None => None,
    };
    if let Some(base) = &base_commit {
        progress(1, format!("base commit {base}"));
        if base == &commit {
            progress(1, "branch is already at this commit");
            println!("On branch {branch}");
            println!(
                "{} at {}",
                source_label_human(&source_label),
                short_hex(&commit)
            );
            println!("Already up to date.");
            return Ok(());
        }
        if !source.is_ancestor(base, &commit)? {
            return Err(Error::Config(format!(
                "scan only moves forward: {commit} is not a descendant of {base} on branch {branch}"
            )));
        }
    }

    let new_blobs = source.list_blobs(&commit)?;
    let mut unchanged = BTreeSet::new();
    let mut changed = BTreeSet::new();
    if let Some(base) = &base_commit {
        let old_blobs = source.list_blobs(base)?;
        let old: BTreeMap<&str, &str> = old_blobs
            .iter()
            .map(|entry| (entry.path.as_str(), entry.oid.as_str()))
            .collect();
        for entry in &new_blobs {
            match old.get(entry.path.as_str()) {
                Some(oid) if *oid == entry.oid.as_str() => {
                    unchanged.insert(entry.path.clone());
                }
                _ => {
                    changed.insert(entry.path.clone());
                }
            }
        }
    } else {
        changed.extend(new_blobs.iter().map(|entry| entry.path.clone()));
    }
    progress(
        1,
        format!(
            "{} changed file(s), {} unchanged",
            changed.len(),
            unchanged.len()
        ),
    );

    let (mut elements, failed) = match &base_version {
        Some(base) => space::reuse_file_elements(&store, dir, base, &unchanged, &commit)?,
        None => (Vec::new(), BTreeSet::new()),
    };
    for path in failed {
        unchanged.remove(&path);
        changed.insert(path);
    }
    let retained = elements.len();
    let report = language::scan_commit_paths(&source, &commit, &changed, &mut |path, count| {
        progress(2, format!("  {path}: {count} element(s)"));
    })?;
    let not_parsed_now = report.skipped_files();
    let parsed = report.elements.len();
    elements.extend(report.elements);
    for entry in &new_blobs {
        let content_hash = parse_commit_hex(&entry.oid)?;
        elements.push(Element {
            name: entry.path.clone(),
            ty: T_FILE,
            path: entry.path.clone(),
            commit: commit.clone(),
            start_byte: 0,
            end_byte: 0,
            start_line: 0,
            end_line: 0,
            own_text: String::new(),
            parent: None,
            children: Vec::new(),
            design: None,
            content_hash: Some(to_hex(&content_hash)),
        });
    }
    elements.sort_by(|a, b| a.name.cmp(&b.name));
    let parsed_paths: BTreeSet<&str> = elements
        .iter()
        .filter(|element| element.ty != T_FILE)
        .map(|element| element.path.as_str())
        .collect();
    let unparsed = new_blobs
        .iter()
        .filter(|entry| !parsed_paths.contains(entry.path.as_str()))
        .count();
    progress(
        1,
        format!(
            "parsed {parsed} element(s) from {} changed file(s); kept {retained} element(s) from {} unchanged file(s)",
            report.extracted_files,
            unchanged.len()
        ),
    );
    if not_parsed_now > 0 {
        progress(1, format!("{not_parsed_now} changed file(s) not parsed"));
        for path in &report.unsupported {
            progress(2, format!("  unparsed {path}: no language adapter"));
        }
        for path in &report.empty {
            progress(2, format!("  unparsed {path}: no elements"));
        }
        for path in &report.skipped_blobs {
            progress(2, format!("  unparsed {path}: not readable text"));
        }
    }

    let mut reused = 0;
    if let Some(base) = &base_version {
        reused = space::reuse_designs(&store, base, &mut elements)?;
        if reused > 0 {
            progress(1, format!("reused {reused} design(s) from base"));
        }
    }

    let scanned = elements.len();
    let result = space::commit(
        &store,
        dir,
        &elements,
        &CommitOptions {
            message: format!("scan {}", &commit[..commit.len().min(12)]),
            author: "ds".into(),
            source_commit: parse_commit_hex(&commit)?,
            pipeline: "tree-sitter/v1".into(),
            ref_name: branch.clone(),
        },
    )?;

    println!("On branch {branch}");
    println!(
        "{} at {}",
        source_label_human(&source_label),
        short_hex(&commit)
    );
    println!(
        "{} file(s) changed, {} unchanged, {unparsed} not parsed",
        changed.len(),
        unchanged.len()
    );
    println!("Scanned {scanned} element(s); {reused} design(s) reused",);
    println!(
        "Created version {} ({} missing design(s))",
        short_hex(&to_hex(&result.version)),
        result.missing
    );
    Ok(())
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
    let version = space::resolve_version(&store, dir, None)?;
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
    let version = space::resolve_version(&store, dir, None)?;
    let current = load_version(&store, &version)?;
    let mut elements = match space::elements_at(&store, dir, &version, true) {
        Ok(elements) => elements,
        Err(error) => {
            progress(
                1,
                format!("tree cache unavailable ({error}); re-parsing source..."),
            );
            elements_from_source(dir, &store, &version, &current)?
        }
    };
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
            source_commit: current.source_commit,
            pipeline: current.pipeline,
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

/// Rebuild the element hierarchy by re-parsing the version's source commit.
/// Used when the local tree cache is missing (for example after a fetch).
fn elements_from_source(
    dir: &Path,
    store: &Store,
    version: &design_space::hash::Hash,
    node: &design_space::node::VersionNode,
) -> Result<Vec<Element>> {
    let config = load_config(dir)?;
    let source = SourceRepo::open(dir, &config.source.url)?;
    if !source.exists() {
        return Err(Error::Config(
            "tree cache is missing and the source repository is unavailable; run `ds scan`".into(),
        ));
    }
    let commit_hex = to_commit_hex(&node.source_commit);
    let report = language::scan_commit(&source, &commit_hex)?;
    let mut elements = report.elements;
    let blobs = source.list_blobs(&commit_hex)?;
    for entry in &blobs {
        let content_hash = parse_commit_hex(&entry.oid)?;
        elements.push(Element {
            name: entry.path.clone(),
            ty: T_FILE,
            path: entry.path.clone(),
            commit: commit_hex.clone(),
            start_byte: 0,
            end_byte: 0,
            start_line: 0,
            end_line: 0,
            own_text: String::new(),
            parent: None,
            children: Vec::new(),
            design: None,
            content_hash: Some(to_hex(&content_hash)),
        });
    }
    let designs: BTreeMap<String, Option<String>> = space::elements_at(store, dir, version, false)?
        .into_iter()
        .map(|element| (element.name, element.design))
        .collect();
    for element in &mut elements {
        if let Some(design) = designs.get(&element.name) {
            element.design = design.clone();
        }
    }
    elements.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(elements)
}

fn show(dir: &Path, name: &str, at: Option<&str>, source_only: bool) -> Result<()> {
    let store = Store::new(dir);
    let version = resolve_version(&store, dir, at)?;
    let (object, design) = space::lookup_object(&store, &version, name)?;

    if source_only {
        if object.ty == T_FILE {
            return print_whole_file(dir, &to_commit_hex(&object.commit), &object.path);
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

fn serve(repo: &Path, listen: Option<&str>, stdio: bool) -> Result<()> {
    match (listen, stdio) {
        (Some(_), true) => Err(Error::Config(
            "use either --listen or --stdio, not both".into(),
        )),
        (None, false) => Err(Error::Config(
            "pass --listen <addr> for HTTP or --stdio for SSH".into(),
        )),
        (Some(address), false) => {
            progress(1, format!("serving {} on http {address}", repo.display()));
            remote::serve_http(repo, address)
        }
        (None, true) => {
            progress(1, format!("serving {} on stdio", repo.display()));
            remote::serve_stdio(repo)
        }
    }
}

fn remote_url(dir: &Path) -> Result<String> {
    load_config(dir)?
        .remote
        .map(|remote| remote.url)
        .ok_or_else(|| Error::Config("no remote configured; run `ds init --remote <url>`".into()))
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
