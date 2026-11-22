//! command-line front end (`ds`).
//!
//! Source checkouts are read-only. Remote sync supports local directories,
//! HTTP endpoints served by `ds-server`, and SSH.

use crate::catalog::{Catalog, structural_dependencies};
use crate::format::{self, content_hash, symbol_hash};
use crate::lock::RepoLock;
use crate::model::*;
use crate::remote::Remote;
use crate::source::SourceRepo;
use crate::workspace::{ObjectFormat, REPO_CONFIG_FILE, RepoConfig, State, Workspace};
use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    #[default]
    Text,
    Json,
    Md,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Initialize a design workspace.
    Init {
        /// Source git URL recorded in .ds/repo.toml.
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
        /// Remote branch to clone.
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// Create one design object.
    Create(CreateArgs),
    /// Edit one design object.
    Edit(EditArgs),
    /// Manage documents and document fragments.
    Doc {
        #[command(subcommand)]
        command: DocCommand,
    },
    /// Manage concepts and the concept space.
    Concept {
        #[command(subcommand)]
        command: ConceptCommand,
    },
    /// Manage systems and modules.
    Structure {
        #[command(subcommand)]
        command: StructureCommand,
    },
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
        /// Allow a non-fast-forward ref update.
        #[arg(long)]
        force: bool,
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
        /// Stage every object associated with this source file.
        #[arg(long, conflicts_with_all = ["all", "dir"])]
        file: Option<String>,
        /// Stage the Directory object for this source directory.
        #[arg(long = "directory", conflicts_with_all = ["all", "file"])]
        directory: Option<String>,
    },
    /// Unstage every object associated with a source file/directory.
    Unstage {
        #[arg(long, conflicts_with = "directory")]
        file: Option<String>,
        #[arg(long = "directory", conflicts_with = "file")]
        directory: Option<String>,
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
    /// Remove a worktree object or subtree.
    Rm {
        name: String,
        /// Remove incoming relations and dependencies before deleting.
        #[arg(long)]
        force: bool,
        /// Remove the object and all containment descendants.
        #[arg(short = 'r', long)]
        recursive: bool,
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
    /// Delete a local branch.
    BranchDelete {
        /// Branch to delete. The current branch cannot be deleted.
        branch: String,
    },
    /// List design objects in the worktree.
    Ls {
        /// List directory and file objects.
        #[arg(long, conflicts_with_all = ["namespaces", "documents", "design"])]
        files: bool,
        /// List namespace and code objects.
        #[arg(long, conflicts_with_all = ["files", "documents", "design"])]
        namespaces: bool,
        /// List document objects.
        #[arg(long, conflicts_with_all = ["files", "namespaces", "design"])]
        documents: bool,
        /// List objects that have a design document.
        #[arg(long, conflicts_with_all = ["files", "namespaces", "documents"])]
        design: bool,
        /// Read the index instead of the worktree.
        #[arg(long)]
        staged: bool,
    },
    /// Show HEAD, index, and worktree roots plus changed objects.
    Status {
        /// Machine-readable unstaged changes only:
        /// status and qname per line, tab-separated.
        #[arg(long)]
        porcelain: bool,
        /// Add kind and source after qname. Only for scripts that need them.
        #[arg(long, requires = "porcelain")]
        detail: bool,
    },
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
        /// Output format.
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
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
    InnerClass,
    Struct,
    InnerStruct,
    Enum,
    InnerEnum,
    Interface,
    InnerInterface,
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
    /// class/struct/enum/interface/mixin, or the parent of a method/part.
    /// Top-level types use --namespace (plus optional --file/--module), not --parent.
    #[arg(long)]
    parent: Option<String>,
    /// Namespace parent for a top-level class/struct/enum/interface/function/mixin.
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
    /// Document or section qname that describes this object.
    #[arg(long)]
    document: Option<String>,
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
    /// Replace the document or section reference.
    #[arg(long, conflicts_with = "clear_document")]
    document: Option<String>,
    /// Remove the document reference.
    #[arg(long = "clear-document", conflicts_with = "document")]
    clear_document: bool,
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
    Add { url: String },
    /// Print the configured remote.
    Show,
    /// Replace the configured remote URL.
    SetUrl { url: String },
}

#[derive(Debug, Clone, Subcommand)]
enum SourceCommand {
    /// Clone if missing, otherwise fetch branches and tags.
    Fetch,
    /// Switch the current source ref.
    Checkout { git_ref: String },
    /// Create the workspace-root source worktree if needed.
    Worktree,
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

#[derive(Debug, Clone, Subcommand)]
enum StructureCommand {
    /// Create the design-structure root.
    Init,
    /// Manage systems.
    System {
        #[command(subcommand)]
        command: StructureSystemCommand,
    },
    /// Manage modules.
    Module {
        #[command(subcommand)]
        command: StructureModuleCommand,
    },
    /// Manage layers inside a system or module.
    Layer {
        #[command(subcommand)]
        command: StructureLayerCommand,
    },
    /// Add top-level code elements to a system, module or layer.
    Assign {
        /// System, module or layer qualified name.
        module: String,
        /// Top-level class/struct/enum/interface/function/mixin qualified names.
        members: Vec<String>,
    },
    /// Remove members from a system, module or layer.
    Unassign {
        /// System, module or layer qualified name.
        module: String,
        /// Member qualified names.
        members: Vec<String>,
    },
    /// List systems, modules and layers.
    List,
    /// Show one system, module or layer.
    Show {
        /// System, module or layer qualified name.
        qname: String,
    },
    /// Report structure problems.
    Check,
    /// List top-level elements that belong to no container.
    Unassigned,
}

#[derive(Debug, Clone, Subcommand)]
enum StructureSystemCommand {
    /// Create a system under the design structure.
    Create {
        /// Local system name.
        name: String,
        /// Display title. Defaults to the name.
        #[arg(long)]
        title: Option<String>,
        /// Document or section qname describing the system.
        #[arg(long)]
        document: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum StructureModuleCommand {
    /// Create a module under a system, module or layer.
    Create {
        /// Local module name.
        name: String,
        /// Parent system, module or layer qualified name.
        #[arg(long)]
        parent: String,
        /// Display title. Defaults to the name.
        #[arg(long)]
        title: Option<String>,
        /// Document or section qname describing the module.
        #[arg(long)]
        document: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum StructureLayerCommand {
    /// Create a layer under a system or module.
    Create {
        /// Local layer name.
        name: String,
        /// Parent system or module qualified name.
        #[arg(long)]
        parent: String,
        /// Display title. Defaults to the name.
        #[arg(long)]
        title: Option<String>,
        /// 1-based order among sibling layers. Defaults to the next order.
        #[arg(long)]
        order: Option<u32>,
        /// Document or section qname describing the layer.
        #[arg(long)]
        document: Option<String>,
    },
    /// Change a layer's title, order or document.
    Set {
        /// Layer qualified name.
        qname: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        order: Option<u32>,
        #[arg(long)]
        document: Option<String>,
    },
    /// Remove an empty layer.
    Rm {
        /// Layer qualified name.
        qname: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocCommand {
    /// Create a document directory.
    Mkdir {
        /// Full qualified name, for example documents/specs.
        qname: String,
    },
    /// Create a document with a title.
    Create {
        /// Full qualified name, for example documents/specs/auth.
        qname: String,
        #[arg(long)]
        title: String,
    },
    /// Rename or move a document directory or document, including its subtree.
    Mv {
        /// Current qname.
        qname: String,
        /// New qname.
        new_qname: String,
    },
    /// Add a section.
    Section {
        #[command(subcommand)]
        command: DocSectionCommand,
    },
    /// Add a titled topic.
    Topic {
        #[command(subcommand)]
        command: DocTopicCommand,
    },
    /// Add or edit a text paragraph.
    Text {
        #[command(subcommand)]
        command: DocTextCommand,
    },
    /// Add or edit a fenced code block.
    Block {
        #[command(subcommand)]
        command: DocBlockCommand,
    },
    /// Add or edit a Mermaid diagram.
    Mermaid {
        #[command(subcommand)]
        command: DocMermaidCommand,
    },
    /// Add or edit an HTML/SVG preview.
    Html {
        #[command(subcommand)]
        command: DocHtmlCommand,
    },
    /// Add a list.
    List {
        #[command(subcommand)]
        command: DocListCommand,
    },
    /// Add, resolve, or list comments on a document object.
    Comment {
        #[command(subcommand)]
        command: DocCommentCommand,
    },
    /// Import a document subtree from a JSON plan in one process.
    Import {
        /// JSON plan path; `-` reads standard input.
        #[arg(long)]
        file: PathBuf,
    },
    /// Search document text and return qnames instead of Markdown line numbers.
    Grep {
        /// Substring to search for.
        pattern: String,
        /// Case-insensitive matching.
        #[arg(long)]
        ignore_case: bool,
        /// Output format.
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    /// Check document field formats and return structured violations.
    Lint {
        /// Comma-separated rules: parameter,return.
        #[arg(long, default_value = "parameter,return")]
        rules: String,
        /// Output format.
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    /// Validate that the technical design sections match the structure tree.
    Check,
    /// Move/create technical design sections so they match the structure tree.
    Layout,
    /// Render a document or fragment.
    Show {
        /// Document, section, topic, list, text, block or document directory qname.
        qname: String,
        /// Output format.
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocCommentCommand {
    /// Add an open comment.
    Add {
        /// Any document object qname.
        qname: String,
        #[arg(long)]
        text: String,
    },
    /// Resolve one comment.
    Resolve {
        /// Any document object qname.
        qname: String,
        /// Comment id: `1` or `comment1`.
        comment: String,
    },
    /// List comments and their status.
    List {
        /// Any document object qname; omit to scan all document objects.
        qname: Option<String>,
        /// Only list open comments.
        #[arg(long)]
        open: bool,
        /// Include comments from the object's whole document subtree.
        #[arg(long)]
        subtree: bool,
        /// Output format.
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    /// Remove every comment from the worktree.
    Clear,
}

#[derive(Debug, Clone, Subcommand)]
enum ConceptCommand {
    /// Create a directory in the concept space.
    Mkdir {
        /// Concept directory qname, for example `concepts/agent`.
        qname: String,
    },
    /// Create a concept.
    Create {
        /// Concept qname, for example `concepts/agent/agent`.
        qname: String,
        /// Term shown to readers; defaults to the last qname segment.
        #[arg(long)]
        name: Option<String>,
        /// One-sentence definition.
        #[arg(long)]
        summary: String,
        /// What the concept is, in more detail.
        #[arg(long)]
        explanation: Option<String>,
    },
    /// Update a concept's name, summary or explanation.
    Set {
        /// Concept qname.
        qname: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        explanation: Option<String>,
    },
    /// Add or remove a characteristic.
    Feature {
        #[command(subcommand)]
        command: ConceptListItemCommand,
    },
    /// Add or remove a relationship/difference.
    Relation {
        #[command(subcommand)]
        command: ConceptListItemCommand,
    },
    /// Add or remove an alias.
    Alias {
        #[command(subcommand)]
        command: ConceptListItemCommand,
    },
    /// Show one concept or concept directory.
    Show {
        /// Concept qname.
        qname: String,
    },
    /// List the children of a concept directory.
    Ls {
        /// Concept directory qname; defaults to `concepts`.
        qname: Option<String>,
    },
    /// Remove a concept or an empty concept directory.
    Rm {
        /// Concept or directory qname.
        qname: String,
    },
    /// Move a concept or an empty directory to another concept directory.
    Mv {
        /// Concept or directory qname.
        qname: String,
        /// New parent concept directory qname.
        parent: String,
        /// Optional new local name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Check that `[[concept:...]]` references resolve to concepts.
    Check,
}

#[derive(Debug, Clone, Subcommand)]
enum ConceptListItemCommand {
    /// Append a list item.
    Add {
        /// Concept qname.
        qname: String,
        /// Item text.
        text: String,
    },
    /// Remove one item by its 1-based position.
    Rm {
        /// Concept qname.
        qname: String,
        /// 1-based position.
        index: usize,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocSectionCommand {
    /// Add a section after a target position.
    Add {
        /// Anchor qname (`.../section`) or an existing sibling section qname.
        target: String,
        #[arg(long)]
        title: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocTopicCommand {
    /// Add a topic after a target position in a section body.
    Add {
        /// Anchor qname (`.../body`) or an existing section body element qname.
        target: String,
        #[arg(long)]
        title: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocTextCommand {
    /// Add a text paragraph after a target position.
    Add {
        /// Anchor qname (`.../body`) or an existing body element qname.
        target: String,
        #[arg(long)]
        text: String,
    },
    /// Replace the text of one paragraph.
    Set {
        /// Text paragraph qname.
        qname: String,
        #[arg(long)]
        text: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocBlockCommand {
    /// Add a fenced code block after a target position.
    Add {
        /// Anchor qname (`.../body`) or an existing body element qname.
        target: String,
        /// Markdown fence info string, for example `rust`; empty for a plain block.
        #[arg(long)]
        language: Option<String>,
        /// Code text; may contain newlines.
        #[arg(long)]
        text: String,
    },
    /// Change a block's language or text.
    Set {
        /// Block qname.
        qname: String,
        /// Markdown fence info string; empty for a plain block.
        #[arg(long)]
        language: Option<String>,
        /// Replace the code text.
        #[arg(long)]
        text: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocMermaidCommand {
    /// Add a Mermaid diagram after a target position.
    Add {
        /// Anchor qname (`.../body`) or an existing body element qname.
        target: String,
        /// Mermaid source text; may contain newlines.
        #[arg(long)]
        text: String,
    },
    /// Replace a Mermaid diagram's source.
    Set {
        /// Mermaid block qname.
        qname: String,
        /// Mermaid source text.
        #[arg(long)]
        text: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocHtmlCommand {
    /// Add an HTML/SVG preview after a target position.
    Add {
        /// Anchor qname (`.../body`) or an existing body element qname.
        target: String,
        /// Raw HTML or SVG source; may contain newlines.
        #[arg(long)]
        text: String,
    },
    /// Replace an HTML/SVG preview's source.
    Set {
        /// HTML block qname.
        qname: String,
        /// Raw HTML or SVG source.
        #[arg(long)]
        text: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocListCommand {
    /// Add an empty list after a target position.
    Add {
        /// Anchor qname (`.../body`) or an existing body element qname.
        target: String,
    },
    /// Add or edit list items.
    Item {
        #[command(subcommand)]
        command: DocListItemCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DocListItemCommand {
    /// Add a text item after a target position.
    Add {
        /// Anchor qname (`.../item`) or an existing list item qname.
        target: String,
        #[arg(long)]
        text: String,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { source, format } => init(cli.dir, &source, format.into()),
        Command::Clone { remote, branch } => clone(cli.dir, &remote, &branch),
        Command::Create(args) => create(cli.dir, args),
        Command::Edit(args) => edit(cli.dir, args),
        Command::Doc { command } => doc(cli.dir, command),
        Command::Concept { command } => concept(cli.dir, command),
        Command::Structure { command } => structure(cli.dir, command),
        Command::Dep { command } => dep(cli.dir, command),
        Command::Source { command } => source(cli.dir, command),
        Command::Remote { command } => remote(cli.dir, command),
        Command::Push { branch, force } => push(cli.dir, branch, force),
        Command::Fetch => fetch(cli.dir),
        Command::Pull { branch } => pull(cli.dir, branch),
        Command::Add {
            names,
            all,
            file,
            directory,
        } => add(cli.dir, names, all, file, directory),
        Command::Unstage { file, directory } => unstage(cli.dir, file, directory),
        Command::Reset { names, hard } => reset(cli.dir, names, hard),
        Command::Diff { staged } => diff(cli.dir, staged),
        Command::Rm {
            name,
            force,
            recursive,
        } => rm(cli.dir, &name, force, recursive),
        Command::Checkout {
            branch,
            create,
            force,
        } => checkout(cli.dir, &branch, create, force),
        Command::BranchDelete { branch } => branch_delete(cli.dir, &branch),
        Command::Ls {
            files,
            namespaces,
            documents,
            design,
            staged,
        } => ls(cli.dir, files, namespaces, documents, design, staged),
        Command::Status { porcelain, detail } => status(cli.dir, porcelain, detail),
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
            format,
        } => show(cli.dir, &name, source, staged, format),
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
    let documents = ws.put_symbol("documents")?;
    catalog.insert(
        files,
        DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: files,
                parent: ParentRef::View,
                source: None,
                document: SymbolHash::ZERO,
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
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        ),
    )?;
    catalog.insert(
        documents,
        DesignObject::new(
            Kind::DocumentDirectory,
            ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                qualified_name: documents,
                parent: ParentRef::View,
                children: vec![],
                comments: vec![],
            }),
        ),
    )?;

    let concepts = ws.put_symbol("concepts")?;
    catalog.insert(
        concepts,
        DesignObject::new(
            Kind::ConceptDirectory,
            ObjectPayload::ConceptDirectory(ConceptDirectoryPayload {
                qualified_name: concepts,
                parent: ParentRef::View,
                children: vec![],
            }),
        ),
    )?;

    // The same preset design objects are created by ds-server repositories.
    let design_dir = ws.put_symbol("documents/design")?;
    catalog.insert(
        design_dir,
        DesignObject::new(
            Kind::DocumentDirectory,
            ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                qualified_name: design_dir,
                parent: ParentRef::Object(documents),
                children: vec![],
                comments: vec![],
            }),
        ),
    )?;
    for (qname, title) in [
        ("documents/design/technical", "技术设计"),
        ("documents/design/directory", "目录设计"),
        ("documents/design/namespace", "命名空间设计"),
    ] {
        let document = ws.put_symbol(qname)?;
        catalog.insert(
            document,
            DesignObject::new(
                Kind::Document,
                ObjectPayload::Document(DocumentPayload {
                    qualified_name: document,
                    parent: design_dir,
                    title: title.to_owned(),
                    sections: vec![],
                    children: vec![],
                    comments: vec![],
                }),
            ),
        )?;
    }
    let proposals = ws.put_symbol("documents/design/drafts")?;
    catalog.insert(
        proposals,
        DesignObject::new(
            Kind::DocumentDirectory,
            ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                qualified_name: proposals,
                parent: ParentRef::Object(design_dir),
                children: vec![],
                comments: vec![],
            }),
        ),
    )?;
    let design = ws.put_symbol("design")?;
    catalog.insert(
        design,
        DesignObject::new(
            Kind::DesignStructure,
            ObjectPayload::DesignStructure(DesignStructurePayload {
                qualified_name: design,
                parent: ParentRef::View,
                title: "设计结构".to_owned(),
                document: SymbolHash::ZERO,
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

fn clone(dir: Option<PathBuf>, remote_url: &str, branch: &str) -> Result<()> {
    let root = dir.unwrap_or(std::env::current_dir()?);
    let meta = root.join(".ds");
    if meta.join("config").exists() {
        bail!("{} already exists", meta.display());
    }
    let mut ws = Workspace::bootstrap(&meta)?;
    ws.set_remote_url(remote_url)?;
    let remote = Remote::open(remote_url)?;
    remote.fetch(&ws)?;

    let config_path = meta.join(REPO_CONFIG_FILE);
    let text = match fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(_) => {
            let _ = fs::remove_dir_all(&meta);
            bail!(
                "design-space remote has no {REPO_CONFIG_FILE}; \
                 the server must create the repository with its source URL"
            );
        }
    };
    let config = match RepoConfig::from_toml(&text) {
        Ok(config) => config,
        Err(error) => {
            let _ = fs::remove_dir_all(&meta);
            return Err(error.into());
        }
    };
    ws.install_repo_config(&config.source.url, config.source.object_format)?;

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

fn doc(dir: Option<PathBuf>, command: DocCommand) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    match command {
        DocCommand::Mkdir { qname } => {
            let (parent, local) = doc_parent_and_local(&ws, &catalog, &qname)?;
            let parent_object = catalog
                .get(parent)
                .with_context(|| format!("parent not found: {}", qname))?;
            if parent_object.kind != Kind::DocumentDirectory {
                bail!("document directories must be created under a document directory: {qname}");
            }
            let qname_hash = child_qname(&ws, parent, &local)?;
            if catalog.get(qname_hash).is_some() {
                bail!("object already exists: {qname}");
            }
            let object = DesignObject::new(
                Kind::DocumentDirectory,
                ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                    qualified_name: qname_hash,
                    parent: ParentRef::Object(parent),
                    children: vec![],
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname_hash, object)?;
            save_worktree(&ws, state, &catalog, &qname)?;
        }
        DocCommand::Create { qname, title } => {
            let (parent, local) = doc_parent_and_local(&ws, &catalog, &qname)?;
            let parent_object = catalog
                .get(parent)
                .with_context(|| format!("parent not found: {qname}"))?;
            if parent_object.kind != Kind::DocumentDirectory {
                bail!("documents must be created under a document directory: {qname}");
            }
            let qname_hash = child_qname(&ws, parent, &local)?;
            if catalog.get(qname_hash).is_some() {
                bail!("object already exists: {qname}");
            }
            let object = DesignObject::new(
                Kind::Document,
                ObjectPayload::Document(DocumentPayload {
                    qualified_name: qname_hash,
                    parent,
                    title,
                    sections: vec![],
                    children: vec![],
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname_hash, object)?;
            save_worktree(&ws, state, &catalog, &qname)?;
        }
        DocCommand::Mv { qname, new_qname } => {
            let old = ws.put_symbol(&qname)?;
            let object = catalog
                .get(old)
                .with_context(|| format!("object not found: {qname}"))?;
            if !matches!(object.kind, Kind::DocumentDirectory | Kind::Document) {
                bail!("only document directories and documents can be moved: {qname}");
            }
            let new = ws.put_symbol(&new_qname)?;
            if catalog.get(new).is_some() {
                bail!("object already exists: {new_qname}");
            }
            let (new_parent_text, new_local) = new_qname
                .rsplit_once('/')
                .with_context(|| format!("invalid document qname: {new_qname}"))?;
            if new_local.is_empty() {
                bail!("invalid document qname: {new_qname}");
            }
            let new_parent = ws.put_symbol(new_parent_text)?;
            let parent_object = catalog
                .get(new_parent)
                .with_context(|| format!("parent not found: {new_parent_text}"))?;
            if parent_object.kind != Kind::DocumentDirectory {
                bail!("documents must be moved under a document directory: {new_qname}");
            }
            let mapping = rename_mapping(&ws, &catalog, old, new)?;
            for target in mapping.values() {
                if *target != new && catalog.get(*target).is_some() && !mapping.contains_key(target)
                {
                    bail!("rename target already exists: {}", ws.get_symbol(*target)?);
                }
            }
            {
                let object = catalog
                    .get_mut(old)
                    .with_context(|| format!("object not found: {qname}"))?;
                match &mut object.payload {
                    ObjectPayload::DocumentDirectory(p) => {
                        p.parent = ParentRef::Object(new_parent);
                    }
                    ObjectPayload::Document(p) => p.parent = new_parent,
                    _ => bail!("only document directories and documents can be moved: {qname}"),
                }
            }
            catalog.rename_symbols(&mapping)?;
            save_worktree(&ws, state, &catalog, &new_qname)?;
        }
        DocCommand::Section {
            command: DocSectionCommand::Add { target, title },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Sections,
                "section",
                &[Kind::DocumentSection],
                &[Kind::Document, Kind::DocumentSection],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "section")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::DocumentSection,
                ObjectPayload::DocumentSection(DocumentSectionPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    title,
                    body: vec![],
                    sections: vec![],
                    children: vec![],
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Sections,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Topic {
            command: DocTopicCommand::Add { target, title },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Body,
                "body",
                &[
                    Kind::DocumentText,
                    Kind::DocumentList,
                    Kind::DocumentTopic,
                    Kind::Block,
                    Kind::MermaidBlock,
                    Kind::HtmlBlock,
                ],
                &[Kind::DocumentSection],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "body")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::DocumentTopic,
                ObjectPayload::DocumentTopic(DocumentTopicPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    title,
                    paragraphs: vec![],
                    children: vec![],
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Body,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Text {
            command: DocTextCommand::Add { target, text },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Body,
                "body",
                &[
                    Kind::DocumentText,
                    Kind::DocumentList,
                    Kind::DocumentTopic,
                    Kind::Block,
                    Kind::MermaidBlock,
                    Kind::HtmlBlock,
                ],
                &[Kind::DocumentSection, Kind::DocumentTopic],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "body")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::DocumentText,
                ObjectPayload::DocumentText(DocumentTextPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    text,
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Body,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Text {
            command: DocTextCommand::Set { qname, text },
        } => {
            let qname_hash = ws.put_symbol(&qname)?;
            let object = catalog
                .get_mut(qname_hash)
                .with_context(|| format!("object not found: {qname}"))?;
            let ObjectPayload::DocumentText(payload) = &mut object.payload else {
                bail!("not a text paragraph: {qname}");
            };
            payload.text = text;
            format::validate_object(object)?;
            save_worktree(&ws, state, &catalog, &qname)?;
        }
        DocCommand::Block {
            command:
                DocBlockCommand::Add {
                    target,
                    language,
                    text,
                },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Body,
                "body",
                &[
                    Kind::DocumentText,
                    Kind::DocumentList,
                    Kind::DocumentTopic,
                    Kind::Block,
                    Kind::MermaidBlock,
                    Kind::HtmlBlock,
                ],
                &[Kind::DocumentSection, Kind::DocumentTopic],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "body")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::Block,
                ObjectPayload::Block(BlockPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    language: language.unwrap_or_default(),
                    text,
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Body,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Block {
            command:
                DocBlockCommand::Set {
                    qname,
                    language,
                    text,
                },
        } => {
            if language.is_none() && text.is_none() {
                bail!("nothing to update: pass --language or --text");
            }
            let qname_hash = ws.put_symbol(&qname)?;
            let object = catalog
                .get_mut(qname_hash)
                .with_context(|| format!("object not found: {qname}"))?;
            let ObjectPayload::Block(payload) = &mut object.payload else {
                bail!("not a block: {qname}");
            };
            if let Some(language) = language {
                payload.language = language;
            }
            if let Some(text) = text {
                payload.text = text;
            }
            format::validate_object(object)?;
            save_worktree(&ws, state, &catalog, &qname)?;
        }
        DocCommand::Mermaid {
            command: DocMermaidCommand::Add { target, text },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Body,
                "body",
                &[
                    Kind::DocumentText,
                    Kind::DocumentList,
                    Kind::DocumentTopic,
                    Kind::Block,
                    Kind::MermaidBlock,
                    Kind::HtmlBlock,
                ],
                &[Kind::DocumentSection, Kind::DocumentTopic],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "body")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::MermaidBlock,
                ObjectPayload::MermaidBlock(MermaidBlockPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    text,
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Body,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Mermaid {
            command: DocMermaidCommand::Set { qname, text },
        } => {
            let qname_hash = ws.put_symbol(&qname)?;
            let object = catalog
                .get_mut(qname_hash)
                .with_context(|| format!("object not found: {qname}"))?;
            let ObjectPayload::MermaidBlock(payload) = &mut object.payload else {
                bail!("not a mermaid block: {qname}");
            };
            payload.text = text;
            format::validate_object(object)?;
            save_worktree(&ws, state, &catalog, &qname)?;
        }
        DocCommand::Html {
            command: DocHtmlCommand::Add { target, text },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Body,
                "body",
                &[
                    Kind::DocumentText,
                    Kind::DocumentList,
                    Kind::DocumentTopic,
                    Kind::Block,
                    Kind::MermaidBlock,
                    Kind::HtmlBlock,
                ],
                &[Kind::DocumentSection, Kind::DocumentTopic],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "body")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::HtmlBlock,
                ObjectPayload::HtmlBlock(HtmlBlockPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    text,
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Body,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Html {
            command: DocHtmlCommand::Set { qname, text },
        } => {
            let qname_hash = ws.put_symbol(&qname)?;
            let object = catalog
                .get_mut(qname_hash)
                .with_context(|| format!("object not found: {qname}"))?;
            let ObjectPayload::HtmlBlock(payload) = &mut object.payload else {
                bail!("not an html block: {qname}");
            };
            payload.text = text;
            format::validate_object(object)?;
            save_worktree(&ws, state, &catalog, &qname)?;
        }
        DocCommand::List {
            command: DocListCommand::Add { target },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Body,
                "body",
                &[
                    Kind::DocumentText,
                    Kind::DocumentList,
                    Kind::DocumentTopic,
                    Kind::Block,
                    Kind::MermaidBlock,
                    Kind::HtmlBlock,
                ],
                &[Kind::DocumentSection, Kind::DocumentTopic],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "body")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::DocumentList,
                ObjectPayload::DocumentList(DocumentListPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    items: vec![],
                    children: vec![],
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Body,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::List {
            command:
                DocListCommand::Item {
                    command: DocListItemCommand::Add { target, text },
                },
        } => {
            let position = resolve_doc_position(
                &ws,
                &catalog,
                &target,
                DocRole::Items,
                "item",
                &[Kind::DocumentText, Kind::DocumentList],
                &[Kind::DocumentList],
            )?;
            let local = next_doc_id(&ws, &catalog, position.parent, "item")?;
            let qname = child_qname(&ws, position.parent, &local)?;
            let object = DesignObject::new(
                Kind::DocumentText,
                ObjectPayload::DocumentText(DocumentTextPayload {
                    qualified_name: qname,
                    parent: position.parent,
                    text,
                    comments: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            insert_doc_ref(
                &mut catalog,
                position.parent,
                DocRole::Items,
                position.index,
                qname,
            )?;
            let name = ws.get_symbol(qname)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Comment { command } => match command {
            DocCommentCommand::Add { qname, text } => {
                let hash = ws.put_symbol(&qname)?;
                let id;
                {
                    let object = catalog
                        .get_mut(hash)
                        .with_context(|| format!("object not found: {qname}"))?;
                    let comments = doc_comments_mut(object)
                        .with_context(|| format!("not a document object: {qname}"))?;
                    id = comments
                        .last()
                        .map(|comment| comment.id.saturating_add(1))
                        .unwrap_or(1);
                    comments.push(DocComment {
                        id,
                        text,
                        resolved: false,
                    });
                    format::validate_object(object)?;
                }
                println!("Added comment{id}");
                save_worktree(&ws, state, &catalog, &qname)?;
            }
            DocCommentCommand::Resolve { qname, comment } => {
                let hash = ws.put_symbol(&qname)?;
                let id = parse_comment_id(&comment)?;
                let mut resolve = false;
                {
                    let object = catalog
                        .get_mut(hash)
                        .with_context(|| format!("object not found: {qname}"))?;
                    let comments = doc_comments_mut(object)
                        .with_context(|| format!("not a document object: {qname}"))?;
                    let target = comments
                        .iter_mut()
                        .find(|comment| comment.id == id)
                        .with_context(|| format!("comment not found: {comment}"))?;
                    if target.resolved {
                        println!("comment{id} is already resolved");
                    } else {
                        target.resolved = true;
                        resolve = true;
                    }
                    format::validate_object(object)?;
                }
                if resolve {
                    println!("Resolved comment{id}");
                    save_worktree(&ws, state, &catalog, &qname)?;
                }
            }
            DocCommentCommand::List {
                qname,
                open,
                subtree,
                format,
            } => {
                let mut printed = 0usize;
                let mut values = Vec::new();
                match qname {
                    Some(qname) => {
                        let qname = qname.trim_end_matches('/').to_owned();
                        let hash = ws.put_symbol(&qname)?;
                        catalog
                            .get(hash)
                            .with_context(|| format!("object not found: {qname}"))?;
                        if subtree {
                            let prefix = format!("{qname}/");
                            let mut objects = catalog
                                .iter()
                                .filter(|(candidate, object)| {
                                    let name = ws.get_symbol(*candidate).unwrap_or_default();
                                    (name == qname || name.starts_with(&prefix))
                                        && doc_comments(object)
                                            .is_some_and(|comments| !comments.is_empty())
                                })
                                .collect::<Vec<_>>();
                            objects.sort_by_key(|(qname, _)| {
                                ws.get_symbol(*qname).unwrap_or_default()
                            });
                            for (candidate, object) in objects {
                                let name = ws.get_symbol(candidate)?;
                                for comment in doc_comments(object).unwrap_or_default() {
                                    if open && comment.resolved {
                                        continue;
                                    }
                                    if format == OutputFormat::Json {
                                        values.push(comment_json(
                                            &ws, &catalog, candidate, object, comment,
                                        )?);
                                    } else {
                                        print_comment(Some(&name), comment);
                                    }
                                    printed += 1;
                                }
                            }
                        } else {
                            let object = catalog
                                .get(hash)
                                .with_context(|| format!("object not found: {qname}"))?;
                            let comments = doc_comments(object)
                                .with_context(|| format!("not a document object: {qname}"))?;
                            for comment in comments {
                                if open && comment.resolved {
                                    continue;
                                }
                                if format == OutputFormat::Json {
                                    values
                                        .push(comment_json(&ws, &catalog, hash, object, comment)?);
                                } else {
                                    print_comment(None, comment);
                                }
                                printed += 1;
                            }
                        }
                    }
                    None => {
                        let mut objects = catalog
                            .iter()
                            .filter(|(_, object)| {
                                doc_comments(object).is_some_and(|comments| !comments.is_empty())
                            })
                            .collect::<Vec<_>>();
                        objects.sort_by_key(|(qname, _)| ws.get_symbol(*qname).unwrap_or_default());
                        for (qname, object) in objects {
                            let name = ws.get_symbol(qname)?;
                            for comment in doc_comments(object).unwrap_or_default() {
                                if open && comment.resolved {
                                    continue;
                                }
                                if format == OutputFormat::Json {
                                    values
                                        .push(comment_json(&ws, &catalog, qname, object, comment)?);
                                } else {
                                    print_comment(Some(&name), comment);
                                }
                                printed += 1;
                            }
                        }
                    }
                }
                if format == OutputFormat::Json {
                    println!("{}", serde_json::to_string_pretty(&values)?);
                } else if printed == 0 {
                    println!("no comments");
                }
            }
            DocCommentCommand::Clear => {
                let targets = catalog
                    .iter()
                    .filter(|(_, object)| {
                        doc_comments(object).is_some_and(|comments| !comments.is_empty())
                    })
                    .map(|(qname, _)| qname)
                    .collect::<Vec<_>>();
                if targets.is_empty() {
                    println!("no comments");
                    return Ok(());
                }
                let mut removed = 0usize;
                for qname in &targets {
                    let Some(object) = catalog.get_mut(*qname) else {
                        continue;
                    };
                    if let Some(comments) = doc_comments_mut(object) {
                        removed += comments.len();
                        comments.clear();
                    }
                    format::validate_object(object)?;
                }
                println!(
                    "Cleared {removed} comment(s) from {} object(s)",
                    targets.len()
                );
                save_worktree(&ws, state, &catalog, "comments cleared")?;
            }
        },
        DocCommand::Import { file } => {
            let root = import_doc_plan(&ws, &mut catalog, &file)?;
            let name = ws.get_symbol(root)?;
            save_worktree(&ws, state, &catalog, &name)?;
        }
        DocCommand::Grep {
            pattern,
            ignore_case,
            format,
        } => {
            doc_grep(&ws, &catalog, &pattern, ignore_case, format)?;
        }
        DocCommand::Lint { rules, format } => {
            doc_lint(&ws, &catalog, &rules, format)?;
        }
        DocCommand::Check => {
            doc_check(&ws, &catalog)?;
        }
        DocCommand::Layout => {
            doc_layout(&ws, &mut catalog)?;
            save_worktree(&ws, state, &catalog, "documents/design/technical")?;
        }
        DocCommand::Show { qname, format } => {
            doc_show(&ws, &catalog, &qname, format)?;
        }
    }
    Ok(())
}

fn concept(dir: Option<PathBuf>, command: ConceptCommand) -> Result<()> {
    let dir_arg = dir.clone();
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;

    let root_text = "concepts";
    let root = ws.put_symbol(root_text)?;
    if catalog.get(root).is_none() {
        let object = DesignObject::new(
            Kind::ConceptDirectory,
            ObjectPayload::ConceptDirectory(ConceptDirectoryPayload {
                qualified_name: root,
                parent: ParentRef::View,
                children: vec![],
            }),
        );
        format::validate_object(&object)?;
        catalog.insert(root, object)?;
    }

    match command {
        ConceptCommand::Mkdir { qname } => {
            let qname = normalize_concept_qname(&qname)?;
            ensure_concept_absent(&catalog, &qname)?;
            let parent = concept_parent(&ws, &catalog, &qname)?;
            let hash = ws.put_symbol(&qname)?;
            let object = DesignObject::new(
                Kind::ConceptDirectory,
                ObjectPayload::ConceptDirectory(ConceptDirectoryPayload {
                    qualified_name: hash,
                    parent: ParentRef::Object(parent),
                    children: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(hash, object)?;
            attach_concept_child(&ws, &mut catalog, parent, hash)?;
            save_worktree(&ws, state, &catalog, &qname)?;
            println!("Created concept directory {qname}");
        }
        ConceptCommand::Create {
            qname,
            name,
            summary,
            explanation,
        } => {
            let qname = normalize_concept_qname(&qname)?;
            ensure_concept_absent(&catalog, &qname)?;
            let parent = concept_parent(&ws, &catalog, &qname)?;
            let local = local_name(&qname);
            let hash = ws.put_symbol(&qname)?;
            let object = DesignObject::new(
                Kind::Concept,
                ObjectPayload::Concept(ConceptPayload {
                    qualified_name: hash,
                    parent,
                    name: name.unwrap_or(local),
                    summary,
                    explanation: explanation.unwrap_or_default(),
                    characteristics: vec![],
                    relations: vec![],
                    aliases: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(hash, object)?;
            attach_concept_child(&ws, &mut catalog, parent, hash)?;
            save_worktree(&ws, state, &catalog, &qname)?;
            println!("Created concept {qname}");
        }
        ConceptCommand::Set {
            qname,
            name,
            summary,
            explanation,
        } => {
            let qname = normalize_concept_qname(&qname)?;
            let hash = ws.put_symbol(&qname)?;
            let object = catalog
                .get_mut(hash)
                .with_context(|| format!("concept not found: {qname}"))?;
            let ObjectPayload::Concept(p) = &mut object.payload else {
                bail!("not a concept: {qname}");
            };
            if let Some(name) = name {
                p.name = name;
            }
            if let Some(summary) = summary {
                p.summary = summary;
            }
            if let Some(explanation) = explanation {
                p.explanation = explanation;
            }
            format::validate_object(object)?;
            save_worktree(&ws, state, &catalog, &qname)?;
            println!("Updated concept {qname}");
        }
        ConceptCommand::Feature { command } => {
            concept_list_item(&ws, state, &mut catalog, command, ConceptList::Feature)?;
        }
        ConceptCommand::Relation { command } => {
            concept_list_item(&ws, state, &mut catalog, command, ConceptList::Relation)?;
        }
        ConceptCommand::Alias { command } => {
            concept_list_item(&ws, state, &mut catalog, command, ConceptList::Alias)?;
        }
        ConceptCommand::Show { qname } => {
            drop(catalog);
            return show(dir_arg, &qname, false, false, OutputFormat::Text);
        }
        ConceptCommand::Ls { qname } => {
            let qname = qname.as_deref().unwrap_or(root_text);
            let hash = ws.put_symbol(qname)?;
            let object = catalog
                .get(hash)
                .with_context(|| format!("concept directory not found: {qname}"))?;
            if object.kind != Kind::ConceptDirectory {
                bail!("not a concept directory: {qname}");
            }
            let mut children = catalog.children(hash);
            children.sort_by_key(|child| ws.get_symbol(*child).unwrap_or_default());
            if children.is_empty() {
                println!("no concepts");
                return Ok(());
            }
            for child in children {
                let Some(object) = catalog.get(child) else {
                    continue;
                };
                let child_text = ws.get_symbol(child)?;
                match &object.payload {
                    ObjectPayload::Concept(p) => {
                        println!("concept\t{child_text}\t{}\t{}", p.name, p.summary);
                    }
                    ObjectPayload::ConceptDirectory(_) => {
                        println!("concept-directory\t{child_text}");
                    }
                    _ => {}
                }
            }
        }
        ConceptCommand::Rm { qname } => {
            let qname = normalize_concept_qname(&qname)?;
            let hash = ws.put_symbol(&qname)?;
            let object = catalog
                .get(hash)
                .with_context(|| format!("concept not found: {qname}"))?;
            if object.kind == Kind::ConceptDirectory && !catalog.children(hash).is_empty() {
                bail!("concept directory is not empty: {qname}");
            }
            let parent = concept_parent(&ws, &catalog, &qname)?;
            detach_concept_child(&ws, &mut catalog, parent, hash)?;
            catalog.remove(hash);
            save_worktree(&ws, state, &catalog, &qname)?;
            println!("Removed {qname}");
        }
        ConceptCommand::Mv {
            qname,
            parent,
            name,
        } => {
            let qname = normalize_concept_qname(&qname)?;
            let old = ws.put_symbol(&qname)?;
            let old_object = catalog
                .get(old)
                .cloned()
                .with_context(|| format!("concept not found: {qname}"))?;
            if old_object.kind == Kind::ConceptDirectory && !catalog.children(old).is_empty() {
                bail!("cannot move a non-empty concept directory; move its children first");
            }
            let old_parent = concept_parent(&ws, &catalog, &qname)?;
            let parent = parent.trim_end_matches('/').to_owned();
            let parent_hash = ws.put_symbol(&parent)?;
            let parent_object = catalog
                .get(parent_hash)
                .with_context(|| format!("concept directory not found: {parent}"))?;
            if parent_object.kind != Kind::ConceptDirectory {
                bail!("not a concept directory: {parent}");
            }
            let local = name.unwrap_or_else(|| local_name(&qname));
            if local.is_empty() || local.contains('/') {
                bail!("invalid concept local name: {local}");
            }
            let new_qname = format!("{parent}/{local}");
            ensure_concept_absent(&catalog, &new_qname)?;
            let new_hash = ws.put_symbol(&new_qname)?;
            let mut new_object = old_object;
            match &mut new_object.payload {
                ObjectPayload::ConceptDirectory(p) => {
                    p.qualified_name = new_hash;
                    p.parent = ParentRef::Object(parent_hash);
                    p.children.clear();
                }
                ObjectPayload::Concept(p) => {
                    p.qualified_name = new_hash;
                    p.parent = parent_hash;
                }
                _ => bail!("not a concept: {qname}"),
            }
            format::validate_object(&new_object)?;
            catalog.insert(new_hash, new_object)?;
            attach_concept_child(&ws, &mut catalog, parent_hash, new_hash)?;
            detach_concept_child(&ws, &mut catalog, old_parent, old)?;
            catalog.remove(old);
            save_worktree(&ws, state, &catalog, &new_qname)?;
            println!("Moved {qname} to {new_qname}");
        }
        ConceptCommand::Check => {
            let mut problems = Vec::new();
            for (qname, object) in catalog.iter() {
                let name = ws.get_symbol(qname)?;
                match &object.payload {
                    ObjectPayload::DocumentText(p) => {
                        collect_concept_problems(&ws, &catalog, &name, &p.text, &mut problems);
                    }
                    ObjectPayload::Concept(p) => {
                        for text in p
                            .characteristics
                            .iter()
                            .chain(p.relations.iter())
                            .chain(std::iter::once(&p.explanation))
                        {
                            collect_concept_problems(&ws, &catalog, &name, text, &mut problems);
                        }
                    }
                    _ => {}
                }
            }
            if problems.is_empty() {
                println!("concept references ok");
            } else {
                for problem in &problems {
                    println!("problem: {problem}");
                }
                bail!("concept check found {} problem(s)", problems.len());
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ConceptList {
    Feature,
    Relation,
    Alias,
}

fn concept_list_item(
    ws: &Workspace,
    state: State,
    catalog: &mut Catalog,
    command: ConceptListItemCommand,
    list: ConceptList,
) -> Result<()> {
    match command {
        ConceptListItemCommand::Add { qname, text } => {
            let qname = normalize_concept_qname(&qname)?;
            let hash = ws.put_symbol(&qname)?;
            let values = concept_list_mut(catalog, hash, &qname, list)?;
            values.push(text);
            let object = catalog
                .get(hash)
                .with_context(|| format!("concept not found: {qname}"))?;
            format::validate_object(object)?;
            save_worktree(ws, state, catalog, &qname)?;
            println!("Updated concept {qname}");
        }
        ConceptListItemCommand::Rm { qname, index } => {
            let qname = normalize_concept_qname(&qname)?;
            let hash = ws.put_symbol(&qname)?;
            let values = concept_list_mut(catalog, hash, &qname, list)?;
            if index == 0 || index > values.len() {
                bail!("list index out of range: {index}");
            }
            values.remove(index - 1);
            let object = catalog
                .get(hash)
                .with_context(|| format!("concept not found: {qname}"))?;
            format::validate_object(object)?;
            save_worktree(ws, state, catalog, &qname)?;
            println!("Updated concept {qname}");
        }
    }
    Ok(())
}

fn concept_list_mut<'a>(
    catalog: &'a mut Catalog,
    hash: SymbolHash,
    qname: &str,
    list: ConceptList,
) -> Result<&'a mut Vec<String>> {
    let object = catalog
        .get_mut(hash)
        .with_context(|| format!("concept not found: {qname}"))?;
    let ObjectPayload::Concept(p) = &mut object.payload else {
        bail!("not a concept: {qname}");
    };
    Ok(match list {
        ConceptList::Feature => &mut p.characteristics,
        ConceptList::Relation => &mut p.relations,
        ConceptList::Alias => &mut p.aliases,
    })
}

fn normalize_concept_qname(qname: &str) -> Result<String> {
    let qname = qname.trim_end_matches('/');
    if qname == "concepts" || !qname.starts_with("concepts/") {
        bail!("concept qname must be under concepts/: {qname}");
    }
    if qname.split('/').any(|part| part.is_empty()) {
        bail!("invalid concept qname: {qname}");
    }
    Ok(qname.to_owned())
}

fn ensure_concept_absent(catalog: &Catalog, qname: &str) -> Result<()> {
    let hash = symbol_hash(qname);
    if catalog.get(hash).is_some() {
        bail!("object already exists: {qname}");
    }
    Ok(())
}

fn concept_parent(ws: &Workspace, catalog: &Catalog, qname: &str) -> Result<SymbolHash> {
    let (parent, local) = qname
        .rsplit_once('/')
        .with_context(|| format!("concept qname has no parent: {qname}"))?;
    if local.is_empty() {
        bail!("concept qname has no local name: {qname}");
    }
    let parent = ws.put_symbol(parent)?;
    let parent_text = ws.get_symbol(parent)?;
    let object = catalog
        .get(parent)
        .with_context(|| format!("concept directory not found: {parent_text}"))?;
    if object.kind != Kind::ConceptDirectory {
        bail!("not a concept directory: {parent_text}");
    }
    Ok(parent)
}

fn attach_concept_child(
    ws: &Workspace,
    catalog: &mut Catalog,
    parent: SymbolHash,
    child: SymbolHash,
) -> Result<()> {
    let child_text = ws.get_symbol(child)?;
    let child_hash = format::object_hash(
        catalog
            .get(child)
            .with_context(|| format!("concept not found: {child_text}"))?,
    )?;
    let parent_text = ws.get_symbol(parent)?;
    let object = catalog
        .get_mut(parent)
        .with_context(|| format!("concept directory not found: {parent_text}"))?;
    let ObjectPayload::ConceptDirectory(p) = &mut object.payload else {
        bail!("not a concept directory: {parent_text}");
    };
    p.children.push(child_hash);
    p.children.sort();
    p.children.dedup();
    Ok(())
}

fn detach_concept_child(
    ws: &Workspace,
    catalog: &mut Catalog,
    parent: SymbolHash,
    child: SymbolHash,
) -> Result<()> {
    let child_text = ws.get_symbol(child)?;
    let child_hash = format::object_hash(
        catalog
            .get(child)
            .with_context(|| format!("concept not found: {child_text}"))?,
    )?;
    let parent_text = ws.get_symbol(parent)?;
    let object = catalog
        .get_mut(parent)
        .with_context(|| format!("concept directory not found: {parent_text}"))?;
    let ObjectPayload::ConceptDirectory(p) = &mut object.payload else {
        bail!("not a concept directory: {parent_text}");
    };
    p.children.retain(|hash| *hash != child_hash);
    Ok(())
}

fn collect_concept_problems(
    ws: &Workspace,
    catalog: &Catalog,
    owner: &str,
    text: &str,
    problems: &mut Vec<String>,
) {
    let mut rest = text;
    while let Some(start) = rest.find("[[concept:") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            break;
        };
        let inner = &after[..end];
        let target = inner
            .split_once('|')
            .map(|(target, _)| target)
            .unwrap_or(inner)
            .trim()
            .strip_prefix("concept:")
            .unwrap_or("")
            .trim();
        if target.is_empty() {
            problems.push(format!("{owner}: empty concept reference"));
        } else if let Ok(hash) = ws.put_symbol(target) {
            match catalog.get(hash) {
                Some(object) if object.kind == Kind::Concept => {}
                Some(_) => problems.push(format!("{owner}: {target} is not a concept")),
                None => problems.push(format!("{owner}: concept not found: {target}")),
            }
        }
        rest = &after[end + 2..];
    }
}

fn structure(dir: Option<PathBuf>, command: StructureCommand) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    let root = ws.put_symbol("design")?;
    match command {
        StructureCommand::Init => {
            if catalog.get(root).is_some() {
                println!("design structure already exists");
                return Ok(());
            }
            let object = DesignObject::new(
                Kind::DesignStructure,
                ObjectPayload::DesignStructure(DesignStructurePayload {
                    qualified_name: root,
                    parent: ParentRef::View,
                    title: "设计结构".to_owned(),
                    document: SymbolHash::ZERO,
                    children: vec![],
                }),
            );
            format::validate_object(&object)?;
            catalog.insert(root, object)?;
            save_worktree(&ws, state, &catalog, "design")?;
        }
        StructureCommand::System { command } => match command {
            StructureSystemCommand::Create {
                name,
                title,
                document,
            } => {
                let parent = catalog
                    .get(root)
                    .context("design structure is missing; run `ds structure init`")?;
                if parent.kind != Kind::DesignStructure {
                    bail!("design root is not a design structure");
                }
                validate_structure_name(&name)?;
                let qname = child_qname(&ws, root, &name)?;
                if catalog.get(qname).is_some() {
                    bail!("system already exists: {name}");
                }
                let object = DesignObject::new(
                    Kind::System,
                    ObjectPayload::System(SystemPayload {
                        qualified_name: qname,
                        parent: root,
                        title: title.unwrap_or_else(|| name.clone()),
                        document: optional_ref(&ws, document.as_deref())?,
                        members: vec![],
                        children: vec![],
                    }),
                );
                format::validate_object(&object)?;
                catalog.insert(qname, object)?;
                save_worktree(&ws, state, &catalog, &format!("design/{name}"))?;
            }
        },
        StructureCommand::Module { command } => match command {
            StructureModuleCommand::Create {
                name,
                parent,
                title,
                document,
            } => {
                validate_structure_name(&name)?;
                let parent_hash = ws.put_symbol(&parent)?;
                let parent_object = catalog
                    .get(parent_hash)
                    .with_context(|| format!("parent not found: {parent}"))?;
                if !matches!(
                    parent_object.kind,
                    Kind::System | Kind::Module | Kind::Layer
                ) {
                    bail!("module parent must be a system, module or layer: {parent}");
                }
                let qname = child_qname(&ws, parent_hash, &name)?;
                if catalog.get(qname).is_some() {
                    bail!("module already exists: {name}");
                }
                let object = DesignObject::new(
                    Kind::Module,
                    ObjectPayload::Module(ModulePayload {
                        qualified_name: qname,
                        parent: parent_hash,
                        title: title.unwrap_or_else(|| name.clone()),
                        document: optional_ref(&ws, document.as_deref())?,
                        members: vec![],
                        children: vec![],
                    }),
                );
                format::validate_object(&object)?;
                catalog.insert(qname, object)?;
                save_worktree(&ws, state, &catalog, &format!("{parent}/{name}"))?;
            }
        },
        StructureCommand::Layer { command } => match command {
            StructureLayerCommand::Create {
                name,
                parent,
                title,
                order,
                document,
            } => {
                validate_structure_name(&name)?;
                let parent_hash = ws.put_symbol(&parent)?;
                let parent_object = catalog
                    .get(parent_hash)
                    .with_context(|| format!("parent not found: {parent}"))?;
                if !matches!(parent_object.kind, Kind::System | Kind::Module) {
                    bail!("layer parent must be a system or module: {parent}");
                }
                let qname = child_qname(&ws, parent_hash, &name)?;
                if catalog.get(qname).is_some() {
                    bail!("layer already exists: {name}");
                }
                let order = match order {
                    Some(order) => order,
                    None => next_layer_order(&catalog, parent_hash),
                };
                validate_layer_order(&catalog, parent_hash, order, None)?;
                let object = DesignObject::new(
                    Kind::Layer,
                    ObjectPayload::Layer(LayerPayload {
                        qualified_name: qname,
                        parent: parent_hash,
                        title: title.unwrap_or_else(|| name.clone()),
                        order,
                        document: optional_ref(&ws, document.as_deref())?,
                        members: vec![],
                        children: vec![],
                    }),
                );
                format::validate_object(&object)?;
                catalog.insert(qname, object)?;
                save_worktree(&ws, state, &catalog, &format!("{parent}/{name}"))?;
            }
            StructureLayerCommand::Set {
                qname,
                title,
                order,
                document,
            } => {
                let hash = ws.put_symbol(&qname)?;
                let layer = catalog
                    .get(hash)
                    .with_context(|| format!("layer not found: {qname}"))?;
                let ObjectPayload::Layer(existing) = &layer.payload else {
                    bail!("not a layer: {qname}");
                };
                let parent = existing.parent;
                if let Some(order) = order {
                    validate_layer_order(&catalog, parent, order, Some(hash))?;
                }
                let document = document
                    .as_deref()
                    .map(|document| optional_ref(&ws, Some(document)))
                    .transpose()?;
                let object = catalog
                    .get_mut(hash)
                    .with_context(|| format!("layer not found: {qname}"))?;
                let ObjectPayload::Layer(p) = &mut object.payload else {
                    bail!("not a layer: {qname}");
                };
                if let Some(order) = order {
                    p.order = order;
                }
                if let Some(title) = title {
                    p.title = title;
                }
                if let Some(document) = document {
                    p.document = document;
                }
                format::validate_object(object)?;
                save_worktree(&ws, state, &catalog, &qname)?;
            }
            StructureLayerCommand::Rm { qname } => {
                let hash = ws.put_symbol(&qname)?;
                let object = catalog
                    .get(hash)
                    .with_context(|| format!("layer not found: {qname}"))?;
                let ObjectPayload::Layer(p) = &object.payload else {
                    bail!("not a layer: {qname}");
                };
                if !p.members.is_empty() || !p.children.is_empty() {
                    bail!("layer is not empty: {qname}");
                }
                catalog.remove(hash);
                save_worktree(&ws, state, &catalog, &qname)?;
            }
        },
        StructureCommand::Assign { module, members } => {
            let unit_hash = ws.put_symbol(&module)?;
            let member_hashes = structure_member_hashes(&ws, &catalog, &members)?;
            for member in &member_hashes {
                let owner = catalog.iter().find(|(qname, object)| {
                    *qname != unit_hash && structure_members(object).contains(member)
                });
                if let Some((owner, _)) = owner {
                    bail!(
                        "{} already belongs to {}",
                        symbol_text(&ws, *member),
                        symbol_text(&ws, owner)
                    );
                }
            }
            {
                let object = catalog
                    .get_mut(unit_hash)
                    .with_context(|| format!("system or module not found: {module}"))?;
                let members = match &mut object.payload {
                    ObjectPayload::System(p) => &mut p.members,
                    ObjectPayload::Module(p) => &mut p.members,
                    ObjectPayload::Layer(p) => &mut p.members,
                    _ => bail!("not a system, module or layer: {module}"),
                };
                members.extend(member_hashes);
                members.sort();
                members.dedup();
                format::validate_object(object)?;
            }
            save_worktree(&ws, state, &catalog, &module)?;
        }
        StructureCommand::Unassign { module, members } => {
            let unit_hash = ws.put_symbol(&module)?;
            let member_hashes = structure_member_hashes(&ws, &catalog, &members)?;
            {
                let object = catalog
                    .get_mut(unit_hash)
                    .with_context(|| format!("system or module not found: {module}"))?;
                let members = match &mut object.payload {
                    ObjectPayload::System(p) => &mut p.members,
                    ObjectPayload::Module(p) => &mut p.members,
                    ObjectPayload::Layer(p) => &mut p.members,
                    _ => bail!("not a system, module or layer: {module}"),
                };
                members.retain(|member| !member_hashes.contains(member));
                format::validate_object(object)?;
            }
            save_worktree(&ws, state, &catalog, &module)?;
        }
        StructureCommand::List => {
            let mut rows = catalog
                .iter()
                .filter(|(_, object)| {
                    matches!(
                        object.kind,
                        Kind::DesignStructure | Kind::System | Kind::Module | Kind::Layer
                    )
                })
                .map(|(qname, object)| {
                    (
                        ws.get_symbol(qname).unwrap_or_default(),
                        structure_kind_label(object.kind),
                        structure_title(object).unwrap_or_default(),
                        structure_members(object).len(),
                    )
                })
                .collect::<Vec<_>>();
            rows.sort_by(|left, right| left.0.cmp(&right.0));
            for (qname, kind, title, members) in rows {
                println!("{kind}\t{qname}\t{title}\tmembers={members}");
            }
        }
        StructureCommand::Show { qname } => {
            let hash = ws.put_symbol(&qname)?;
            let object = catalog
                .get(hash)
                .with_context(|| format!("object not found: {qname}"))?;
            if !matches!(
                object.kind,
                Kind::DesignStructure | Kind::System | Kind::Module | Kind::Layer
            ) {
                bail!("not a system, module or layer: {qname}");
            }
            println!("{qname}");
            println!("kind: {}", structure_kind_label(object.kind));
            println!("title: {}", structure_title(object).unwrap_or_default());
            if let ObjectPayload::Layer(p) = &object.payload {
                println!("order: {}", p.order);
            }
            println!(
                "document: {}",
                structure_document(&ws, object).unwrap_or_default()
            );
            let children = catalog
                .children(hash)
                .iter()
                .map(|child| ws.get_symbol(*child).unwrap_or_default())
                .collect::<Vec<_>>();
            if !children.is_empty() {
                println!("children:");
                for child in children {
                    println!("  {child}");
                }
            }
            let members = structure_members(object);
            if !members.is_empty() {
                println!("members:");
                for member in members {
                    println!("  {}", ws.get_symbol(*member).unwrap_or_default());
                }
            }
        }
        StructureCommand::Check => {
            catalog
                .verify(ws.store())
                .context("design-space verification failed")?;
            let assigned = catalog
                .iter()
                .flat_map(|(_, object)| structure_members(object).iter().copied())
                .collect::<BTreeSet<_>>();
            let mut problems = Vec::new();
            let mut top_level = Vec::new();
            for (qname, object) in catalog.iter() {
                if matches!(
                    object.kind,
                    Kind::Class
                        | Kind::Struct
                        | Kind::Enum
                        | Kind::Interface
                        | Kind::Function
                        | Kind::Mixin
                ) {
                    top_level.push(qname);
                    if !assigned.contains(&qname) {
                        problems.push(format!(
                            "top-level element is not assigned to a system or module: {}",
                            ws.get_symbol(qname).unwrap_or_default()
                        ));
                    }
                }
            }
            for (qname, object) in catalog.iter() {
                match &object.payload {
                    ObjectPayload::DesignStructure(p) => {
                        if p.children.is_empty() {
                            problems.push("design structure has no systems".to_owned());
                        }
                    }
                    ObjectPayload::System(p) => {
                        if p.children.is_empty() && p.members.is_empty() {
                            problems.push(format!(
                                "system is empty: {}",
                                ws.get_symbol(qname).unwrap_or_default()
                            ));
                        }
                    }
                    ObjectPayload::Module(p) => {
                        if p.members.is_empty() && p.children.is_empty() {
                            problems.push(format!(
                                "module is empty: {}",
                                ws.get_symbol(qname).unwrap_or_default()
                            ));
                        }
                    }
                    _ => {}
                }
            }
            let mut owners = BTreeMap::new();
            for (qname, object) in catalog.iter() {
                for member in structure_members(object) {
                    if let Some(previous) = owners.insert(*member, qname) {
                        problems.push(format!(
                            "{} belongs to both {} and {}",
                            ws.get_symbol(*member).unwrap_or_default(),
                            ws.get_symbol(previous).unwrap_or_default(),
                            ws.get_symbol(qname).unwrap_or_default()
                        ));
                    }
                }
            }
            let mut layer_rows: BTreeMap<SymbolHash, (SymbolHash, u32, String)> = BTreeMap::new();
            for (qname, object) in catalog.iter() {
                let ObjectPayload::Layer(p) = &object.payload else {
                    continue;
                };
                let name = ws.get_symbol(qname).unwrap_or_default();
                if !catalog
                    .get(p.parent)
                    .is_some_and(|parent| matches!(parent.kind, Kind::System | Kind::Module))
                {
                    problems.push(format!("layer parent must be a system or module: {name}"));
                }
                if p.members.is_empty() && p.children.is_empty() {
                    problems.push(format!("layer is empty: {name}"));
                }
                if catalog.children(qname).iter().any(|child| {
                    catalog
                        .get(*child)
                        .is_some_and(|object| object.kind == Kind::Layer)
                }) {
                    problems.push(format!("layer cannot contain another layer: {name}"));
                }
                if p.order == 0 {
                    problems.push(format!("layer order must start at 1: {name}"));
                }
                layer_rows.insert(qname, (p.parent, p.order, name));
            }
            let layers = layer_rows.iter().collect::<Vec<_>>();
            for (index, (_, (parent, order, name))) in layers.iter().enumerate() {
                for (_, (other_parent, other_order, other_name)) in &layers[index + 1..] {
                    if parent == other_parent && order == other_order {
                        problems.push(format!(
                            "layers {} and {} use the same order {}",
                            name, other_name, order
                        ));
                    }
                }
            }
            for (qname, object) in catalog.iter() {
                if !matches!(object.kind, Kind::System | Kind::Module) {
                    continue;
                }
                let name = ws.get_symbol(qname).unwrap_or_default();
                let children = catalog.children(qname);
                let has_layers = children.iter().any(|child| {
                    catalog
                        .get(*child)
                        .is_some_and(|object| object.kind == Kind::Layer)
                });
                if !has_layers {
                    continue;
                }
                if !structure_members(object).is_empty() {
                    problems.push(format!(
                        "layered container has direct members; assign them to layers: {name}"
                    ));
                }
                if children.iter().any(|child| {
                    catalog
                        .get(*child)
                        .is_some_and(|object| object.kind == Kind::Module)
                }) {
                    problems.push(format!(
                        "layered container has direct modules; put them inside layers: {name}"
                    ));
                }
            }
            for (qname, object) in catalog.iter() {
                let Some(owner) = owners.get(&qname) else {
                    continue;
                };
                let Some(layer_a) = layer_for_owner(*owner, &catalog) else {
                    continue;
                };
                let Some((parent_a, order_a, name_a)) = layer_rows.get(&layer_a) else {
                    continue;
                };
                for target in object_dependency_targets(object) {
                    let Some(target_owner) = owners.get(&target) else {
                        continue;
                    };
                    let Some(layer_b) = layer_for_owner(*target_owner, &catalog) else {
                        continue;
                    };
                    if layer_a == layer_b {
                        continue;
                    }
                    let Some((parent_b, order_b, name_b)) = layer_rows.get(&layer_b) else {
                        continue;
                    };
                    if parent_a != parent_b {
                        continue;
                    }
                    if order_a.abs_diff(*order_b) > 1 {
                        problems.push(format!(
                            "layers {} and {} interact but are not adjacent",
                            name_a, name_b
                        ));
                    }
                }
            }
            if problems.is_empty() {
                println!(
                    "structure ok: {} top-level element(s), {} assigned",
                    top_level.len(),
                    assigned.len()
                );
            } else {
                for problem in &problems {
                    println!("problem: {problem}");
                }
                bail!("structure check found {} problem(s)", problems.len());
            }
        }
        StructureCommand::Unassigned => {
            let assigned = catalog
                .iter()
                .flat_map(|(_, object)| structure_members(object).iter().copied())
                .collect::<BTreeSet<_>>();
            let mut rows = catalog
                .iter()
                .filter(|(qname, object)| {
                    matches!(
                        object.kind,
                        Kind::Class
                            | Kind::Struct
                            | Kind::Enum
                            | Kind::Interface
                            | Kind::Function
                            | Kind::Mixin
                    ) && !assigned.contains(qname)
                })
                .map(|(qname, _)| ws.get_symbol(qname).unwrap_or_default())
                .collect::<Vec<_>>();
            rows.sort();
            for qname in rows {
                println!("{qname}");
            }
        }
    }
    Ok(())
}

fn validate_structure_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        bail!("invalid structure name: {name}");
    }
    Ok(())
}

fn sibling_layers(catalog: &Catalog, parent: SymbolHash) -> Vec<(SymbolHash, u32)> {
    catalog
        .children(parent)
        .into_iter()
        .filter_map(|child| {
            let object = catalog.get(child)?;
            let ObjectPayload::Layer(p) = &object.payload else {
                return None;
            };
            Some((child, p.order))
        })
        .collect()
}

fn next_layer_order(catalog: &Catalog, parent: SymbolHash) -> u32 {
    sibling_layers(catalog, parent)
        .into_iter()
        .map(|(_, order)| order)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn validate_layer_order(
    catalog: &Catalog,
    parent: SymbolHash,
    order: u32,
    exclude: Option<SymbolHash>,
) -> Result<()> {
    if order == 0 {
        bail!("layer order must start at 1");
    }
    for (layer, existing) in sibling_layers(catalog, parent) {
        if Some(layer) != exclude && existing == order {
            bail!("layer order {order} is already used by a sibling layer");
        }
    }
    Ok(())
}

fn structure_member_hashes(
    ws: &Workspace,
    catalog: &Catalog,
    members: &[String],
) -> Result<Vec<SymbolHash>> {
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        let hash = ws.put_symbol(member)?;
        let object = catalog
            .get(hash)
            .with_context(|| format!("member not found: {member}"))?;
        if !matches!(
            object.kind,
            Kind::Class
                | Kind::Struct
                | Kind::Enum
                | Kind::Interface
                | Kind::Function
                | Kind::Mixin
        ) {
            bail!("not a top-level code element: {member}");
        }
        out.push(hash);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn structure_kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::DesignStructure => "design-structure",
        Kind::System => "system",
        Kind::Module => "module",
        Kind::Layer => "layer",
        _ => "object",
    }
}

fn structure_title(object: &DesignObject) -> Option<String> {
    match &object.payload {
        ObjectPayload::DesignStructure(p) => Some(p.title.clone()),
        ObjectPayload::System(p) => Some(p.title.clone()),
        ObjectPayload::Module(p) => Some(p.title.clone()),
        ObjectPayload::Layer(p) => Some(p.title.clone()),
        _ => None,
    }
}

fn structure_document(ws: &Workspace, object: &DesignObject) -> Option<String> {
    let document = match &object.payload {
        ObjectPayload::DesignStructure(p) => p.document,
        ObjectPayload::System(p) => p.document,
        ObjectPayload::Module(p) => p.document,
        ObjectPayload::Layer(p) => p.document,
        _ => return None,
    };
    (!document.is_zero()).then(|| ws.get_symbol(document).unwrap_or_default())
}

fn structure_members(object: &DesignObject) -> &[SymbolHash] {
    match &object.payload {
        ObjectPayload::System(p) => &p.members,
        ObjectPayload::Module(p) => &p.members,
        ObjectPayload::Layer(p) => &p.members,
        _ => &[],
    }
}

fn object_dependency_targets(object: &DesignObject) -> Vec<SymbolHash> {
    let (explicit, implicit) = match &object.payload {
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            (&p.explicit_dependencies, &p.implicit_dependencies)
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            (&p.explicit_dependencies, &p.implicit_dependencies)
        }
        ObjectPayload::Method(p) => (&p.explicit_dependencies, &p.implicit_dependencies),
        ObjectPayload::Function(p) => (&p.explicit_dependencies, &p.implicit_dependencies),
        ObjectPayload::Part(p) => (&p.explicit_dependencies, &p.implicit_dependencies),
        ObjectPayload::Mixin(p) => (&p.explicit_dependencies, &p.implicit_dependencies),
        _ => return Vec::new(),
    };
    explicit
        .iter()
        .chain(implicit.iter())
        .map(|dependency| dependency.target)
        .collect()
}

fn layer_for_owner(owner: SymbolHash, catalog: &Catalog) -> Option<SymbolHash> {
    let mut current = owner;
    loop {
        let object = catalog.get(current)?;
        match &object.payload {
            ObjectPayload::Layer(_) => return Some(current),
            ObjectPayload::Module(p) => current = p.parent,
            ObjectPayload::System(_) => return None,
            _ => return None,
        }
    }
}

const TECHNICAL_DESIGN_DOC: &str = "documents/design/technical";
const DESIGN_PRINCIPLES_SECTION: &str = "设计原则";

#[derive(Debug, Clone)]
struct DocPlacement {
    object: SymbolHash,
    /// The owning System/Module/element, or `None` for a System whose parent is
    /// the technical document itself.
    parent: Option<SymbolHash>,
    title: String,
}

fn expected_doc_placements(ws: &Workspace, catalog: &Catalog) -> Result<Vec<DocPlacement>> {
    let technical = ws.put_symbol(TECHNICAL_DESIGN_DOC)?;
    if catalog.get(technical).is_none() {
        bail!("technical design document not found: {TECHNICAL_DESIGN_DOC}");
    }
    let mut out = Vec::new();
    let mut systems = catalog
        .iter()
        .filter(|(_, object)| object.kind == Kind::System)
        .map(|(qname, _)| qname)
        .collect::<Vec<_>>();
    systems.sort_by_key(|qname| ws.get_symbol(*qname).unwrap_or_default());
    for system in systems {
        push_container_placement(ws, catalog, system, None, &mut out)?;
    }
    Ok(out)
}

fn push_container_placement(
    ws: &Workspace,
    catalog: &Catalog,
    object: SymbolHash,
    parent: Option<SymbolHash>,
    out: &mut Vec<DocPlacement>,
) -> Result<()> {
    let container = catalog.get(object).with_context(|| {
        format!(
            "object not found: {}",
            ws.get_symbol(object).unwrap_or_default()
        )
    })?;
    let title = match &container.payload {
        ObjectPayload::System(p) => p.title.clone(),
        ObjectPayload::Module(p) => p.title.clone(),
        _ => bail!("{} is not a system or module", ws.get_symbol(object)?),
    };
    out.push(DocPlacement {
        object,
        parent,
        title,
    });
    let mut layers = catalog
        .children(object)
        .into_iter()
        .filter(|child| {
            catalog
                .get(*child)
                .is_some_and(|object| object.kind == Kind::Layer)
        })
        .collect::<Vec<_>>();
    layers.sort_by_key(|child| {
        catalog
            .get(*child)
            .and_then(|object| match &object.payload {
                ObjectPayload::Layer(layer) => Some(layer.order),
                _ => None,
            })
            .unwrap_or(u32::MAX)
    });
    for layer in &layers {
        push_layer_placement(ws, catalog, *layer, Some(object), out)?;
    }
    let modules = catalog
        .children(object)
        .into_iter()
        .filter(|child| {
            catalog
                .get(*child)
                .is_some_and(|object| object.kind == Kind::Module)
        })
        .collect::<Vec<_>>();
    let mut modules = modules;
    modules.sort_by_key(|child| ws.get_symbol(*child).unwrap_or_default());
    for module in modules {
        push_container_placement(ws, catalog, module, Some(object), out)?;
    }
    let members = structure_members(container);
    let mut members = members.to_vec();
    members.sort_by_key(|member| ws.get_symbol(*member).unwrap_or_default());
    for member in members {
        push_element_placement(ws, catalog, member, Some(object), out)?;
    }
    Ok(())
}

fn push_layer_placement(
    ws: &Workspace,
    catalog: &Catalog,
    layer: SymbolHash,
    parent: Option<SymbolHash>,
    out: &mut Vec<DocPlacement>,
) -> Result<()> {
    let object = catalog.get(layer).with_context(|| {
        format!(
            "object not found: {}",
            ws.get_symbol(layer).unwrap_or_default()
        )
    })?;
    let ObjectPayload::Layer(payload) = &object.payload else {
        bail!("{} is not a layer", ws.get_symbol(layer)?);
    };
    out.push(DocPlacement {
        object: layer,
        parent,
        title: payload.title.clone(),
    });
    let mut modules = catalog
        .children(layer)
        .into_iter()
        .filter(|child| {
            catalog
                .get(*child)
                .is_some_and(|object| object.kind == Kind::Module)
        })
        .collect::<Vec<_>>();
    modules.sort_by_key(|child| ws.get_symbol(*child).unwrap_or_default());
    for module in modules {
        push_container_placement(ws, catalog, module, Some(layer), out)?;
    }
    let mut members = structure_members(object).to_vec();
    members.sort_by_key(|member| ws.get_symbol(*member).unwrap_or_default());
    for member in members {
        push_element_placement(ws, catalog, member, Some(layer), out)?;
    }
    Ok(())
}

fn push_element_placement(
    ws: &Workspace,
    catalog: &Catalog,
    element: SymbolHash,
    parent: Option<SymbolHash>,
    out: &mut Vec<DocPlacement>,
) -> Result<()> {
    let name = ws.get_symbol(element)?;
    out.push(DocPlacement {
        object: element,
        parent,
        title: element_title(&name),
    });
    let mut nested = Vec::new();
    for (qname, object) in catalog.iter() {
        if let Some(child_parent) = nested_element_parent(object)
            && document_anchor(child_parent, catalog) == Some(element)
        {
            nested.push(qname);
        }
    }
    nested.sort_by_key(|qname| ws.get_symbol(*qname).unwrap_or_default());
    nested.dedup();
    for child in nested {
        push_element_placement(ws, catalog, child, Some(element), out)?;
    }
    Ok(())
}

fn nested_element_parent(object: &DesignObject) -> Option<SymbolHash> {
    match &object.payload {
        ObjectPayload::InnerClass(p) => match p.parent {
            ClassParent::Nested { parent } => Some(parent),
            ClassParent::TopLevel { .. } => None,
        },
        ObjectPayload::InnerInterface(p) => match p.parent {
            ClassParent::Nested { parent } => Some(parent),
            ClassParent::TopLevel { .. } => None,
        },
        ObjectPayload::InnerStruct(p) => match p.parent {
            ClassParent::Nested { parent } => Some(parent),
            ClassParent::TopLevel { .. } => None,
        },
        ObjectPayload::InnerEnum(p) => match p.parent {
            ClassParent::Nested { parent } => Some(parent),
            ClassParent::TopLevel { .. } => None,
        },
        ObjectPayload::Method(p) => Some(p.parent),
        ObjectPayload::Mixin(p) => match p.parent {
            ClassParent::Nested { parent } => Some(parent),
            ClassParent::TopLevel { .. } => None,
        },
        _ => None,
    }
}

/// A Method under a Part belongs to the Part's owning type for documentation.
fn document_anchor(mut hash: SymbolHash, catalog: &Catalog) -> Option<SymbolHash> {
    let mut seen = BTreeSet::new();
    while seen.insert(hash) {
        let object = catalog.get(hash)?;
        match &object.payload {
            ObjectPayload::Part(p) => hash = p.parent,
            _ => return Some(hash),
        }
    }
    None
}

fn doc_section_parent(catalog: &Catalog, section: SymbolHash) -> Option<SymbolHash> {
    match &catalog.get(section)?.payload {
        ObjectPayload::DocumentSection(p) => Some(p.parent),
        _ => None,
    }
}

fn placement_parent(
    ws: &Workspace,
    catalog: &Catalog,
    placement: &DocPlacement,
    technical: SymbolHash,
) -> Result<SymbolHash> {
    match placement.parent {
        None => Ok(technical),
        Some(parent) => {
            let parent_text = ws.get_symbol(parent)?;
            let parent_object = catalog
                .get(parent)
                .with_context(|| format!("parent object not found: {parent_text}"))?;
            document_of(parent_object)
                .filter(|section| !section.is_zero())
                .filter(|section| {
                    catalog
                        .get(*section)
                        .is_some_and(|object| object.kind == Kind::DocumentSection)
                })
                .ok_or_else(|| anyhow::anyhow!("parent {parent_text} has no document Section"))
        }
    }
}

fn add_doc_section(
    ws: &Workspace,
    catalog: &mut Catalog,
    parent: SymbolHash,
    title: &str,
) -> Result<SymbolHash> {
    let local = next_doc_id(ws, catalog, parent, "section")?;
    let qname = child_qname(ws, parent, &local)?;
    let object = DesignObject::new(
        Kind::DocumentSection,
        ObjectPayload::DocumentSection(DocumentSectionPayload {
            qualified_name: qname,
            parent,
            title: title.to_string(),
            body: vec![],
            sections: vec![],
            children: vec![],
            comments: vec![],
        }),
    );
    format::validate_object(&object)?;
    catalog.insert(qname, object)?;
    let index = catalog
        .get(parent)
        .and_then(|object| ordered_refs(object, DocRole::Sections))
        .map(Vec::len)
        .unwrap_or(0);
    insert_doc_ref(catalog, parent, DocRole::Sections, index, qname)?;
    Ok(qname)
}

#[derive(Debug, Deserialize)]
struct ImportPlan {
    root: ImportRoot,
    #[serde(default)]
    body: Vec<ImportBody>,
    #[serde(default)]
    sections: Vec<ImportSection>,
    #[serde(default)]
    documents: Vec<ImportPlan>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ImportRoot {
    Existing(String),
    Create(ImportRootCreate),
}

#[derive(Debug, Deserialize)]
struct ImportRootCreate {
    qname: String,
    #[serde(default)]
    kind: ImportRootKind,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ImportRootKind {
    #[default]
    Document,
    Directory,
    Section,
    Topic,
}

#[derive(Debug, Deserialize)]
struct ImportSection {
    #[serde(default)]
    qname: Option<String>,
    title: String,
    #[serde(default)]
    body: Vec<ImportBody>,
    #[serde(default)]
    document: Vec<String>,
    #[serde(default)]
    sections: Vec<ImportSection>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ImportBody {
    Text {
        #[serde(default)]
        qname: Option<String>,
        text: String,
    },
    Block {
        #[serde(default)]
        qname: Option<String>,
        #[serde(default)]
        language: String,
        text: String,
    },
    Mermaid {
        #[serde(default)]
        qname: Option<String>,
        text: String,
    },
    Html {
        #[serde(default)]
        qname: Option<String>,
        text: String,
    },
    Topic {
        #[serde(default)]
        qname: Option<String>,
        title: String,
        #[serde(default)]
        body: Vec<ImportBody>,
    },
    List {
        #[serde(default)]
        qname: Option<String>,
        items: Vec<ImportListItem>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ImportListItem {
    Text(String),
    Object {
        #[serde(default)]
        qname: Option<String>,
        text: String,
    },
}

impl ImportListItem {
    fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Object { text, .. } => text,
        }
    }

    fn qname(&self) -> Option<&str> {
        match self {
            Self::Text(_) => None,
            Self::Object { qname, .. } => qname.as_deref(),
        }
    }
}

fn import_doc_plan(ws: &Workspace, catalog: &mut Catalog, file: &Path) -> Result<SymbolHash> {
    let text = if file == Path::new("-") {
        std::io::read_to_string(std::io::stdin()).context("read import plan from stdin")?
    } else {
        fs::read_to_string(file).with_context(|| format!("read import plan: {}", file.display()))?
    };
    let plan: ImportPlan = serde_json::from_str(&text)
        .with_context(|| format!("parse import plan: {}", file.display()))?;
    let mut created = 0usize;
    let root = import_plan(ws, catalog, &plan, &mut created)?;
    println!("Imported {created} section(s)");
    Ok(root)
}

fn import_plan(
    ws: &Workspace,
    catalog: &mut Catalog,
    plan: &ImportPlan,
    created: &mut usize,
) -> Result<SymbolHash> {
    let root = import_root(ws, catalog, &plan.root)?;
    let kind = catalog.get(root).map(|object| object.kind);
    if !plan.body.is_empty() {
        match kind {
            Some(Kind::DocumentSection) | Some(Kind::DocumentTopic) => {}
            _ => bail!("root body requires a section or topic root"),
        }
        let in_topic = kind == Some(Kind::DocumentTopic);
        for body in &plan.body {
            import_body(ws, catalog, root, body, in_topic)?;
        }
    }
    if !plan.documents.is_empty() && kind != Some(Kind::DocumentDirectory) {
        bail!("documents requires a document-directory root");
    }
    for section in &plan.sections {
        import_section(ws, catalog, root, section, created)?;
    }
    for document in &plan.documents {
        import_plan(ws, catalog, document, created)?;
    }
    Ok(root)
}

fn import_root(ws: &Workspace, catalog: &mut Catalog, spec: &ImportRoot) -> Result<SymbolHash> {
    match spec {
        ImportRoot::Existing(qname) => {
            let root = ws.put_symbol(qname)?;
            let object = catalog
                .get(root)
                .with_context(|| format!("document root not found: {qname}"))?;
            if !matches!(
                object.kind,
                Kind::Document
                    | Kind::DocumentDirectory
                    | Kind::DocumentSection
                    | Kind::DocumentTopic
            ) {
                bail!("document root must be a document, directory, section or topic: {qname}");
            }
            Ok(root)
        }
        ImportRoot::Create(create) => {
            let root = ws.put_symbol(&create.qname)?;
            if let Some(object) = catalog.get(root) {
                let kind = match object.kind {
                    Kind::Document => ImportRootKind::Document,
                    Kind::DocumentDirectory => ImportRootKind::Directory,
                    Kind::DocumentSection => ImportRootKind::Section,
                    Kind::DocumentTopic => ImportRootKind::Topic,
                    _ => bail!(
                        "document root exists but is not a document node: {}",
                        create.qname
                    ),
                };
                if kind != create.kind {
                    bail!(
                        "document root {} exists with a different kind",
                        create.qname
                    );
                }
                return Ok(root);
            }
            let (parent, local) = doc_parent_and_local(ws, catalog, &create.qname)?;
            let parent_object = catalog
                .get(parent)
                .with_context(|| format!("parent not found: {}", create.qname))?;
            let parent_ok = match create.kind {
                ImportRootKind::Directory | ImportRootKind::Document => {
                    parent_object.kind == Kind::DocumentDirectory
                }
                ImportRootKind::Section => {
                    matches!(parent_object.kind, Kind::Document | Kind::DocumentSection)
                }
                ImportRootKind::Topic => parent_object.kind == Kind::DocumentSection,
            };
            if !parent_ok {
                bail!(
                    "cannot create {} under {}",
                    create.qname,
                    ws.get_symbol(parent)?
                );
            }
            let qname = child_qname(ws, parent, &local)?;
            if catalog.get(qname).is_some() {
                bail!("object already exists: {}", create.qname);
            }
            let title = create
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_owned)
                .unwrap_or(local);
            let object = match create.kind {
                ImportRootKind::Directory => DesignObject::new(
                    Kind::DocumentDirectory,
                    ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                        qualified_name: qname,
                        parent: ParentRef::Object(parent),
                        children: vec![],
                        comments: vec![],
                    }),
                ),
                ImportRootKind::Document => DesignObject::new(
                    Kind::Document,
                    ObjectPayload::Document(DocumentPayload {
                        qualified_name: qname,
                        parent,
                        title,
                        sections: vec![],
                        children: vec![],
                        comments: vec![],
                    }),
                ),
                ImportRootKind::Section => DesignObject::new(
                    Kind::DocumentSection,
                    ObjectPayload::DocumentSection(DocumentSectionPayload {
                        qualified_name: qname,
                        parent,
                        title,
                        body: vec![],
                        sections: vec![],
                        children: vec![],
                        comments: vec![],
                    }),
                ),
                ImportRootKind::Topic => DesignObject::new(
                    Kind::DocumentTopic,
                    ObjectPayload::DocumentTopic(DocumentTopicPayload {
                        qualified_name: qname,
                        parent,
                        title,
                        paragraphs: vec![],
                        children: vec![],
                        comments: vec![],
                    }),
                ),
            };
            format::validate_object(&object)?;
            catalog.insert(qname, object)?;
            Ok(qname)
        }
    }
}

fn import_section(
    ws: &Workspace,
    catalog: &mut Catalog,
    parent: SymbolHash,
    node: &ImportSection,
    created: &mut usize,
) -> Result<SymbolHash> {
    let title = node.title.trim();
    if title.is_empty() {
        bail!("section title must not be empty");
    }
    let section = match resolve_import_object(
        ws,
        catalog,
        node.qname.as_deref(),
        parent,
        &[Kind::DocumentSection],
    )? {
        Some(section) => section,
        None => match find_child_section(catalog, parent, title) {
            Some(section) => section,
            None => {
                *created += 1;
                add_doc_section(ws, catalog, parent, title)?
            }
        },
    };
    if let Some(object) = catalog.get_mut(section)
        && let ObjectPayload::DocumentSection(p) = &mut object.payload
        && p.title != title
    {
        p.title = title.to_owned();
    }
    for body in &node.body {
        import_body(ws, catalog, section, body, false)?;
    }
    for target in &node.document {
        link_document(ws, catalog, target, section)?;
    }
    for child in &node.sections {
        import_section(ws, catalog, section, child, created)?;
    }
    Ok(section)
}

/// Resolve an exported qname to an existing object under `parent`. A qname
/// that is present but missing, of the wrong kind, or under another parent is
/// an error: the caller asked to update a specific object, not to create one.
fn resolve_import_object(
    ws: &Workspace,
    catalog: &Catalog,
    qname: Option<&str>,
    parent: SymbolHash,
    kinds: &[Kind],
) -> Result<Option<SymbolHash>> {
    let Some(qname) = qname else {
        return Ok(None);
    };
    let hash = ws.put_symbol(qname)?;
    let object = catalog
        .get(hash)
        .with_context(|| format!("object not found: {qname}"))?;
    if !kinds.contains(&object.kind) {
        bail!(
            "object {qname} has kind {:?}, expected {:?}",
            object.kind,
            kinds
        );
    }
    if doc_parent_of(object) != Some(parent) {
        bail!(
            "object {qname} is not a child of {}",
            ws.get_symbol(parent)?
        );
    }
    Ok(Some(hash))
}

fn doc_parent_of(object: &DesignObject) -> Option<SymbolHash> {
    match &object.payload {
        ObjectPayload::DocumentSection(p) => Some(p.parent),
        ObjectPayload::DocumentTopic(p) => Some(p.parent),
        ObjectPayload::DocumentList(p) => Some(p.parent),
        ObjectPayload::DocumentText(p) => Some(p.parent),
        ObjectPayload::Block(p) => Some(p.parent),
        ObjectPayload::MermaidBlock(p) => Some(p.parent),
        ObjectPayload::HtmlBlock(p) => Some(p.parent),
        _ => None,
    }
}

fn import_body(
    ws: &Workspace,
    catalog: &mut Catalog,
    parent: SymbolHash,
    body: &ImportBody,
    in_topic: bool,
) -> Result<()> {
    match body {
        ImportBody::Text { qname, text } => {
            if let Some(existing) =
                resolve_import_object(ws, catalog, qname.as_deref(), parent, &[Kind::DocumentText])?
            {
                if let Some(object) = catalog.get_mut(existing)
                    && let ObjectPayload::DocumentText(p) = &mut object.payload
                {
                    p.text = text.clone();
                }
                return Ok(());
            }
            if body_text_exists(catalog, parent, text) {
                return Ok(());
            }
            append_body(ws, catalog, parent, |qname, parent| {
                (
                    Kind::DocumentText,
                    ObjectPayload::DocumentText(DocumentTextPayload {
                        qualified_name: qname,
                        parent,
                        text: text.clone(),
                        comments: vec![],
                    }),
                )
            })?;
        }
        ImportBody::Block {
            qname,
            language,
            text,
        } => {
            if let Some(existing) =
                resolve_import_object(ws, catalog, qname.as_deref(), parent, &[Kind::Block])?
            {
                if let Some(object) = catalog.get_mut(existing)
                    && let ObjectPayload::Block(p) = &mut object.payload
                {
                    p.language = language.clone();
                    p.text = text.clone();
                }
                return Ok(());
            }
            if body_block_exists(catalog, parent, language, text) {
                return Ok(());
            }
            append_body(ws, catalog, parent, |qname, parent| {
                (
                    Kind::Block,
                    ObjectPayload::Block(BlockPayload {
                        qualified_name: qname,
                        parent,
                        language: language.clone(),
                        text: text.clone(),
                        comments: vec![],
                    }),
                )
            })?;
        }
        ImportBody::Mermaid { qname, text } => {
            if let Some(existing) =
                resolve_import_object(ws, catalog, qname.as_deref(), parent, &[Kind::MermaidBlock])?
            {
                if let Some(object) = catalog.get_mut(existing)
                    && let ObjectPayload::MermaidBlock(p) = &mut object.payload
                {
                    p.text = text.clone();
                }
                return Ok(());
            }
            if body_text_block_exists(catalog, parent, text, BodyKind::Mermaid) {
                return Ok(());
            }
            append_body(ws, catalog, parent, |qname, parent| {
                (
                    Kind::MermaidBlock,
                    ObjectPayload::MermaidBlock(MermaidBlockPayload {
                        qualified_name: qname,
                        parent,
                        text: text.clone(),
                        comments: vec![],
                    }),
                )
            })?;
        }
        ImportBody::Html { qname, text } => {
            if let Some(existing) =
                resolve_import_object(ws, catalog, qname.as_deref(), parent, &[Kind::HtmlBlock])?
            {
                if let Some(object) = catalog.get_mut(existing)
                    && let ObjectPayload::HtmlBlock(p) = &mut object.payload
                {
                    p.text = text.clone();
                }
                return Ok(());
            }
            if body_text_block_exists(catalog, parent, text, BodyKind::Html) {
                return Ok(());
            }
            append_body(ws, catalog, parent, |qname, parent| {
                (
                    Kind::HtmlBlock,
                    ObjectPayload::HtmlBlock(HtmlBlockPayload {
                        qualified_name: qname,
                        parent,
                        text: text.clone(),
                        comments: vec![],
                    }),
                )
            })?;
        }
        ImportBody::Topic {
            qname,
            title,
            body: paragraphs,
        } => {
            if in_topic {
                bail!("a topic cannot contain another topic");
            }
            let topic = match resolve_import_object(
                ws,
                catalog,
                qname.as_deref(),
                parent,
                &[Kind::DocumentTopic],
            )? {
                Some(topic) => topic,
                None => match find_child_topic(catalog, parent, title) {
                    Some(topic) => topic,
                    None => append_body(ws, catalog, parent, |qname, parent| {
                        (
                            Kind::DocumentTopic,
                            ObjectPayload::DocumentTopic(DocumentTopicPayload {
                                qualified_name: qname,
                                parent,
                                title: title.clone(),
                                paragraphs: vec![],
                                children: vec![],
                                comments: vec![],
                            }),
                        )
                    })?,
                },
            };
            if let Some(object) = catalog.get_mut(topic)
                && let ObjectPayload::DocumentTopic(p) = &mut object.payload
            {
                p.title = title.clone();
            }
            for paragraph in paragraphs {
                import_body(ws, catalog, topic, paragraph, true)?;
            }
        }
        ImportBody::List { qname, items } => {
            let existing = resolve_import_object(
                ws,
                catalog,
                qname.as_deref(),
                parent,
                &[Kind::DocumentList],
            )?;
            let list = match existing {
                Some(list) => list,
                None => {
                    if body_list_exists(catalog, parent, items) {
                        return Ok(());
                    }
                    append_body(ws, catalog, parent, |qname, parent| {
                        (
                            Kind::DocumentList,
                            ObjectPayload::DocumentList(DocumentListPayload {
                                qualified_name: qname,
                                parent,
                                items: vec![],
                                children: vec![],
                                comments: vec![],
                            }),
                        )
                    })?
                }
            };
            import_list_items(ws, catalog, list, items)?;
        }
    }
    Ok(())
}

fn import_list_items(
    ws: &Workspace,
    catalog: &mut Catalog,
    list: SymbolHash,
    items: &[ImportListItem],
) -> Result<()> {
    let existing = match catalog.get(list).map(|object| &object.payload) {
        Some(ObjectPayload::DocumentList(p)) => p.items.clone(),
        _ => bail!("not a document list: {}", ws.get_symbol(list)?),
    };
    for (index, item) in items.iter().enumerate() {
        let target = match item.qname() {
            Some(qname) => Some(
                resolve_import_object(ws, catalog, Some(qname), list, &[Kind::DocumentText])?
                    .with_context(|| format!("object not found: {qname}"))?,
            ),
            None => existing.get(index).copied(),
        };
        if let Some(target) = target {
            if let Some(object) = catalog.get_mut(target)
                && let ObjectPayload::DocumentText(p) = &mut object.payload
            {
                p.text = item.text().to_owned();
            }
        } else {
            append_list_item(ws, catalog, list, item.text())?;
        }
    }
    Ok(())
}

fn append_body(
    ws: &Workspace,
    catalog: &mut Catalog,
    parent: SymbolHash,
    build: impl FnOnce(SymbolHash, SymbolHash) -> (Kind, ObjectPayload),
) -> Result<SymbolHash> {
    let local = next_doc_id(ws, catalog, parent, "body")?;
    let qname = child_qname(ws, parent, &local)?;
    let (kind, payload) = build(qname, parent);
    let object = DesignObject::new(kind, payload);
    format::validate_object(&object)?;
    catalog.insert(qname, object)?;
    let index = catalog
        .get(parent)
        .and_then(|object| ordered_refs(object, DocRole::Body))
        .map(Vec::len)
        .unwrap_or(0);
    insert_doc_ref(catalog, parent, DocRole::Body, index, qname)?;
    Ok(qname)
}

fn append_list_item(
    ws: &Workspace,
    catalog: &mut Catalog,
    list: SymbolHash,
    text: &str,
) -> Result<()> {
    let local = next_doc_id(ws, catalog, list, "item")?;
    let qname = child_qname(ws, list, &local)?;
    let object = DesignObject::new(
        Kind::DocumentText,
        ObjectPayload::DocumentText(DocumentTextPayload {
            qualified_name: qname,
            parent: list,
            text: text.to_owned(),
            comments: vec![],
        }),
    );
    format::validate_object(&object)?;
    catalog.insert(qname, object)?;
    let index = catalog
        .get(list)
        .and_then(|object| ordered_refs(object, DocRole::Items))
        .map(Vec::len)
        .unwrap_or(0);
    insert_doc_ref(catalog, list, DocRole::Items, index, qname)?;
    Ok(())
}

fn link_document(
    ws: &Workspace,
    catalog: &mut Catalog,
    target: &str,
    section: SymbolHash,
) -> Result<()> {
    let hash = ws.put_symbol(target)?;
    let object = catalog
        .get_mut(hash)
        .with_context(|| format!("object not found: {target}"))?;
    set_document(object, section)?;
    format::validate_object(object)?;
    Ok(())
}

fn find_child_section(catalog: &Catalog, parent: SymbolHash, title: &str) -> Option<SymbolHash> {
    catalog.get(parent).and_then(|object| {
        ordered_refs(object, DocRole::Sections)?
            .iter()
            .find_map(|hash| {
                let child = catalog.get(*hash)?;
                match &child.payload {
                    ObjectPayload::DocumentSection(payload) if payload.title == title => {
                        Some(*hash)
                    }
                    _ => None,
                }
            })
    })
}

fn find_child_topic(catalog: &Catalog, parent: SymbolHash, title: &str) -> Option<SymbolHash> {
    doc_body_refs(catalog, parent).into_iter().find(|hash| {
        catalog.get(*hash).is_some_and(
            |object| matches!(&object.payload, ObjectPayload::DocumentTopic(p) if p.title == title),
        )
    })
}

fn doc_body_refs(catalog: &Catalog, parent: SymbolHash) -> Vec<SymbolHash> {
    catalog
        .get(parent)
        .and_then(|object| ordered_refs(object, DocRole::Body))
        .cloned()
        .unwrap_or_default()
}

fn body_text_exists(catalog: &Catalog, parent: SymbolHash, text: &str) -> bool {
    doc_body_refs(catalog, parent).into_iter().any(|hash| {
        catalog.get(hash).is_some_and(
            |object| matches!(&object.payload, ObjectPayload::DocumentText(p) if p.text == text),
        )
    })
}

fn body_block_exists(catalog: &Catalog, parent: SymbolHash, language: &str, text: &str) -> bool {
    doc_body_refs(catalog, parent).into_iter().any(|hash| {
        catalog.get(hash).is_some_and(|object| {
            matches!(
                &object.payload,
                ObjectPayload::Block(p) if p.language == language && p.text == text
            )
        })
    })
}

#[derive(Clone, Copy)]
enum BodyKind {
    Mermaid,
    Html,
}

fn body_text_block_exists(
    catalog: &Catalog,
    parent: SymbolHash,
    text: &str,
    kind: BodyKind,
) -> bool {
    doc_body_refs(catalog, parent).into_iter().any(|hash| {
        let Some(object) = catalog.get(hash) else {
            return false;
        };
        match (kind, &object.payload) {
            (BodyKind::Mermaid, ObjectPayload::MermaidBlock(p)) => p.text == text,
            (BodyKind::Html, ObjectPayload::HtmlBlock(p)) => p.text == text,
            _ => false,
        }
    })
}

fn body_list_exists(catalog: &Catalog, parent: SymbolHash, items: &[ImportListItem]) -> bool {
    doc_body_refs(catalog, parent).into_iter().any(|hash| {
        let Some(object) = catalog.get(hash) else {
            return false;
        };
        let ObjectPayload::DocumentList(list) = &object.payload else {
            return false;
        };
        list.items.len() == items.len()
            && list.items.iter().zip(items).all(|(item, text)| {
                catalog.get(*item).is_some_and(|object| {
                    matches!(&object.payload, ObjectPayload::DocumentText(p) if p.text == text.text())
                })
            })
    })
}

fn move_doc_section(
    ws: &Workspace,
    catalog: &mut Catalog,
    section: SymbolHash,
    new_parent: SymbolHash,
) -> Result<SymbolHash> {
    let old_parent =
        doc_section_parent(catalog, section).context("document section has no parent")?;
    if old_parent == new_parent {
        return Ok(section);
    }
    let local = next_doc_id(ws, catalog, new_parent, "section")?;
    let new_qname = child_qname(ws, new_parent, &local)?;
    let mapping = rename_mapping(ws, catalog, section, new_qname)?;

    if let Some(parent) = catalog.get_mut(old_parent)
        && let Some(refs) = ordered_refs_mut(parent, DocRole::Sections)
    {
        refs.retain(|child| *child != section);
    }
    {
        let section_text = ws.get_symbol(section)?;
        let object = catalog
            .get_mut(section)
            .with_context(|| format!("section not found: {section_text}"))?;
        let ObjectPayload::DocumentSection(p) = &mut object.payload else {
            bail!("not a document section: {section_text}");
        };
        p.parent = new_parent;
    }
    let index = catalog
        .get(new_parent)
        .and_then(|object| ordered_refs(object, DocRole::Sections))
        .map(Vec::len)
        .unwrap_or(0);
    insert_doc_ref(catalog, new_parent, DocRole::Sections, index, new_qname)?;
    catalog.rename_symbols(&mapping)?;
    Ok(new_qname)
}

fn doc_layout(ws: &Workspace, catalog: &mut Catalog) -> Result<()> {
    let placements = expected_doc_placements(ws, catalog)?;
    let technical = ws.put_symbol(TECHNICAL_DESIGN_DOC)?;
    let mut changes = 0usize;
    for placement in &placements {
        let parent = placement_parent(ws, catalog, placement, technical)?;
        let object_text = ws.get_symbol(placement.object)?;
        let object = catalog
            .get(placement.object)
            .with_context(|| format!("object not found: {object_text}"))?;
        let current = document_of(object).filter(|document| !document.is_zero());
        let section = match current {
            Some(document)
                if catalog
                    .get(document)
                    .is_some_and(|object| object.kind == Kind::DocumentSection) =>
            {
                if doc_section_parent(catalog, document) == Some(parent) {
                    document
                } else {
                    let moved = move_doc_section(ws, catalog, document, parent)?;
                    changes += 1;
                    moved
                }
            }
            _ => {
                let created = add_doc_section(ws, catalog, parent, &placement.title)?;
                changes += 1;
                created
            }
        };
        if set_section_title(catalog, section, &placement.title)? {
            changes += 1;
        }
        set_document(
            catalog
                .get_mut(placement.object)
                .with_context(|| format!("object not found: {object_text}"))?,
            section,
        )?;
    }

    let mut namespaces = catalog
        .iter()
        .filter(|(_, object)| object.kind == Kind::Namespace)
        .map(|(qname, _)| qname)
        .collect::<Vec<_>>();
    namespaces.sort_by_key(|qname| ws.get_symbol(*qname).unwrap_or_default());
    for namespace in namespaces {
        let namespace_text = ws.get_symbol(namespace)?;
        let Some(object) = catalog.get(namespace) else {
            continue;
        };
        let Some(section) = document_of(object).filter(|section| !section.is_zero()) else {
            continue;
        };
        if !catalog
            .get(section)
            .is_some_and(|object| object.kind == Kind::DocumentSection)
        {
            continue;
        }
        if set_section_title(catalog, section, &local_name(&namespace_text))? {
            changes += 1;
        }
    }

    println!("document hierarchy layout: {changes} change(s)");
    Ok(())
}

fn set_section_title(catalog: &mut Catalog, section: SymbolHash, title: &str) -> Result<bool> {
    let object = catalog
        .get_mut(section)
        .with_context(|| format!("section not found: {}", section.to_hex()))?;
    let ObjectPayload::DocumentSection(payload) = &mut object.payload else {
        bail!("not a document section: {}", section.to_hex());
    };
    if payload.title == title {
        return Ok(false);
    }
    payload.title = title.to_owned();
    Ok(true)
}

fn doc_check(ws: &Workspace, catalog: &Catalog) -> Result<()> {
    let placements = expected_doc_placements(ws, catalog)?;
    let technical = ws.put_symbol(TECHNICAL_DESIGN_DOC)?;
    let mut problems = Vec::new();
    for placement in &placements {
        let expected = match placement_parent(ws, catalog, placement, technical) {
            Ok(parent) => parent,
            Err(error) => {
                problems.push(format!("{}: {error}", ws.get_symbol(placement.object)?));
                continue;
            }
        };
        let object_text = ws.get_symbol(placement.object)?;
        let object = catalog
            .get(placement.object)
            .with_context(|| format!("object not found: {object_text}"))?;
        let Some(document) = document_of(object).filter(|document| !document.is_zero()) else {
            problems.push(format!(
                "{}: no document Section",
                ws.get_symbol(placement.object)?
            ));
            continue;
        };
        if !catalog
            .get(document)
            .is_some_and(|object| object.kind == Kind::DocumentSection)
        {
            problems.push(format!(
                "{}: document {} is not a Section",
                ws.get_symbol(placement.object)?,
                ws.get_symbol(document)?
            ));
            continue;
        }
        let actual = doc_section_parent(catalog, document);
        if actual != Some(expected) {
            problems.push(format!(
                "{}: Section {} is under {}, expected {}",
                ws.get_symbol(placement.object)?,
                ws.get_symbol(document)?,
                actual
                    .map(|parent| ws.get_symbol(parent).unwrap_or_default())
                    .unwrap_or_else(|| "<none>".to_string()),
                ws.get_symbol(expected)?
            ));
        }
        if let Some(section) = catalog.get(document)
            && let ObjectPayload::DocumentSection(payload) = &section.payload
            && payload.title != placement.title
        {
            problems.push(format!(
                "{}: Section title is {:?}, expected {:?}",
                ws.get_symbol(placement.object)?,
                payload.title,
                placement.title
            ));
        }
    }
    let mut namespaces = catalog
        .iter()
        .filter(|(_, object)| object.kind == Kind::Namespace)
        .map(|(qname, _)| qname)
        .collect::<Vec<_>>();
    namespaces.sort_by_key(|qname| ws.get_symbol(*qname).unwrap_or_default());
    for namespace in namespaces {
        let namespace_text = ws.get_symbol(namespace)?;
        let Some(object) = catalog.get(namespace) else {
            continue;
        };
        let Some(document) = document_of(object).filter(|document| !document.is_zero()) else {
            continue;
        };
        let Some(section) = catalog.get(document) else {
            problems.push(format!(
                "{namespace_text}: document {} is not a Section",
                ws.get_symbol(document).unwrap_or_default()
            ));
            continue;
        };
        let ObjectPayload::DocumentSection(payload) = &section.payload else {
            problems.push(format!(
                "{namespace_text}: document {} is not a Section",
                ws.get_symbol(document).unwrap_or_default()
            ));
            continue;
        };
        let expected = local_name(&namespace_text);
        if payload.title != expected {
            problems.push(format!(
                "{namespace_text}: Section title is {:?}, expected short name {:?}",
                payload.title, expected
            ));
        }
    }
    if let Some(technical_object) = catalog.get(technical)
        && let ObjectPayload::Document(document) = &technical_object.payload
    {
        let placement_sections = placements
            .iter()
            .filter_map(|placement| catalog.get(placement.object).and_then(document_of))
            .collect::<BTreeSet<_>>();
        let mut principles = Vec::new();
        for (index, section) in document.sections.iter().enumerate() {
            if placement_sections.contains(section) {
                continue;
            }
            if let Some(section_object) = catalog.get(*section)
                && let ObjectPayload::DocumentSection(payload) = &section_object.payload
                && payload.title == DESIGN_PRINCIPLES_SECTION
            {
                principles.push(index);
                if !payload.sections.is_empty() {
                    problems.push(format!(
                        "{TECHNICAL_DESIGN_DOC}: {DESIGN_PRINCIPLES_SECTION} Section must not contain child Sections"
                    ));
                }
                if payload.body.is_empty()
                    || payload.body.iter().any(|item| {
                        catalog
                            .get(*item)
                            .is_none_or(|object| object.kind != Kind::DocumentTopic)
                    })
                {
                    problems.push(format!(
                        "{TECHNICAL_DESIGN_DOC}: {DESIGN_PRINCIPLES_SECTION} Section must contain principle topics only"
                    ));
                }
            }
        }
        if principles.len() > 1 {
            problems.push(format!(
                "{TECHNICAL_DESIGN_DOC}: {DESIGN_PRINCIPLES_SECTION} Section appears {} times",
                principles.len()
            ));
        } else if let Some(index) = principles.first()
            && *index != 0
        {
            problems.push(format!(
                "{TECHNICAL_DESIGN_DOC}: {DESIGN_PRINCIPLES_SECTION} Section must be the first Section"
            ));
        }
    }
    if problems.is_empty() {
        println!("document hierarchy ok: {} object(s)", placements.len());
        return Ok(());
    }
    for problem in &problems {
        println!("problem: {problem}");
    }
    bail!(
        "document hierarchy check found {} problem(s)",
        problems.len()
    )
}

#[derive(Clone, Copy, Debug)]
enum DocRole {
    Sections,
    Body,
    Items,
}

struct DocPosition {
    parent: SymbolHash,
    index: usize,
}

fn doc_parent_and_local(
    ws: &Workspace,
    catalog: &Catalog,
    qname: &str,
) -> Result<(SymbolHash, String)> {
    if qname == "documents" {
        bail!("the documents root already exists");
    }
    let (parent, local) = qname
        .rsplit_once('/')
        .with_context(|| format!("qname must include a parent: {qname}"))?;
    if local.is_empty() {
        bail!("empty document name: {qname}");
    }
    if !local
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        bail!("document names use lowercase letters, digits, '-' and '_': {local:?}");
    }
    let parent = ws.put_symbol(parent)?;
    if catalog.get(parent).is_none() {
        bail!("parent not found: {parent:?}");
    }
    Ok((parent, local.to_owned()))
}

fn resolve_doc_position(
    ws: &Workspace,
    catalog: &Catalog,
    target: &str,
    role: DocRole,
    anchor: &str,
    element_kinds: &[Kind],
    owner_kinds: &[Kind],
) -> Result<DocPosition> {
    let target_hash = symbol_hash(target);
    if let Some(object) = catalog.get(target_hash) {
        if !element_kinds.contains(&object.kind) {
            bail!(
                "target has kind {:?}; expected one of {:?}",
                object.kind,
                element_kinds
            );
        }
        let parents = catalog.parents(target_hash);
        if parents.len() != 1 {
            bail!("target must have exactly one parent: {target}");
        }
        let parent = parents[0];
        let parent_object = catalog
            .get(parent)
            .with_context(|| format!("parent not found for target: {target}"))?;
        if !owner_kinds.contains(&parent_object.kind) {
            bail!(
                "target parent has kind {:?}; expected one of {:?}",
                parent_object.kind,
                owner_kinds
            );
        }
        let refs = ordered_refs(parent_object, role)
            .with_context(|| format!("target is not in a valid {role:?} collection: {target}"))?;
        let index = refs
            .iter()
            .position(|candidate| *candidate == target_hash)
            .with_context(|| format!("target is not in the {role:?} collection: {target}"))?;
        return Ok(DocPosition {
            parent,
            index: index + 1,
        });
    }

    let (owner_text, local) = target
        .rsplit_once('/')
        .with_context(|| format!("object not found: {target}"))?;
    if local != anchor {
        bail!("object not found: {target}");
    }
    let owner = ws.put_symbol(owner_text)?;
    let owner_object = catalog
        .get(owner)
        .with_context(|| format!("parent not found: {owner_text}"))?;
    if !owner_kinds.contains(&owner_object.kind) {
        bail!(
            "anchor owner has kind {:?}; expected one of {:?}",
            owner_object.kind,
            owner_kinds
        );
    }
    ordered_refs(owner_object, role)
        .with_context(|| format!("{owner_text} does not have a {role:?} collection"))?;
    Ok(DocPosition {
        parent: owner,
        index: 0,
    })
}

fn ordered_refs(object: &DesignObject, role: DocRole) -> Option<&Vec<SymbolHash>> {
    match (&object.payload, role) {
        (ObjectPayload::Document(p), DocRole::Sections) => Some(&p.sections),
        (ObjectPayload::DocumentSection(p), DocRole::Sections) => Some(&p.sections),
        (ObjectPayload::DocumentSection(p), DocRole::Body) => Some(&p.body),
        (ObjectPayload::DocumentTopic(p), DocRole::Body) => Some(&p.paragraphs),
        (ObjectPayload::DocumentList(p), DocRole::Items) => Some(&p.items),
        _ => None,
    }
}

fn ordered_refs_mut(object: &mut DesignObject, role: DocRole) -> Option<&mut Vec<SymbolHash>> {
    match (&mut object.payload, role) {
        (ObjectPayload::Document(p), DocRole::Sections) => Some(&mut p.sections),
        (ObjectPayload::DocumentSection(p), DocRole::Sections) => Some(&mut p.sections),
        (ObjectPayload::DocumentSection(p), DocRole::Body) => Some(&mut p.body),
        (ObjectPayload::DocumentTopic(p), DocRole::Body) => Some(&mut p.paragraphs),
        (ObjectPayload::DocumentList(p), DocRole::Items) => Some(&mut p.items),
        _ => None,
    }
}

fn insert_doc_ref(
    catalog: &mut Catalog,
    parent: SymbolHash,
    role: DocRole,
    index: usize,
    child: SymbolHash,
) -> Result<()> {
    let object = catalog
        .get_mut(parent)
        .with_context(|| format!("parent not found: {}", parent.to_hex()))?;
    let refs = ordered_refs_mut(object, role)
        .with_context(|| format!("parent does not have a {role:?} collection"))?;
    if index > refs.len() {
        bail!("insert position is out of range");
    }
    refs.insert(index, child);
    Ok(())
}

fn next_doc_id(
    ws: &Workspace,
    catalog: &Catalog,
    parent: SymbolHash,
    prefix: &str,
) -> Result<String> {
    let mut next = 1u32;
    for (qname, _) in catalog.iter() {
        if !catalog.parents(qname).contains(&parent) {
            continue;
        }
        let Some(text) = ws.get_symbol(qname).ok() else {
            continue;
        };
        let Some(local) = text.rsplit_once('/').map(|(_, local)| local) else {
            continue;
        };
        let Some(number) = local.strip_prefix(prefix) else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        if let Ok(value) = number.parse::<u32>() {
            next = next.max(value + 1);
        }
    }
    Ok(format!("{prefix}{next}"))
}

fn save_worktree(ws: &Workspace, state: State, catalog: &Catalog, name: &str) -> Result<()> {
    let view = catalog.rebuild(ws.store())?;
    ws.write_worktree(State {
        base_commit: state.base_commit,
        root_view: view,
    })?;
    println!("Wrote {name}");
    println!("Worktree view {}", view.to_hex());
    Ok(())
}

fn doc_comments(object: &DesignObject) -> Option<&[DocComment]> {
    match &object.payload {
        ObjectPayload::DocumentDirectory(p) => Some(&p.comments),
        ObjectPayload::Document(p) => Some(&p.comments),
        ObjectPayload::DocumentSection(p) => Some(&p.comments),
        ObjectPayload::DocumentList(p) => Some(&p.comments),
        ObjectPayload::DocumentText(p) => Some(&p.comments),
        ObjectPayload::DocumentTopic(p) => Some(&p.comments),
        ObjectPayload::Block(p) => Some(&p.comments),
        ObjectPayload::MermaidBlock(p) => Some(&p.comments),
        ObjectPayload::HtmlBlock(p) => Some(&p.comments),
        _ => None,
    }
}

fn doc_comments_mut(object: &mut DesignObject) -> Option<&mut Vec<DocComment>> {
    match &mut object.payload {
        ObjectPayload::DocumentDirectory(p) => Some(&mut p.comments),
        ObjectPayload::Document(p) => Some(&mut p.comments),
        ObjectPayload::DocumentSection(p) => Some(&mut p.comments),
        ObjectPayload::DocumentList(p) => Some(&mut p.comments),
        ObjectPayload::DocumentText(p) => Some(&mut p.comments),
        ObjectPayload::DocumentTopic(p) => Some(&mut p.comments),
        ObjectPayload::Block(p) => Some(&mut p.comments),
        ObjectPayload::MermaidBlock(p) => Some(&mut p.comments),
        ObjectPayload::HtmlBlock(p) => Some(&mut p.comments),
        _ => None,
    }
}

fn parse_comment_id(text: &str) -> Result<u32> {
    let digits = text.strip_prefix("comment").unwrap_or(text);
    let id = digits
        .parse::<u32>()
        .with_context(|| format!("invalid comment id: {text}"))?;
    if id == 0 {
        bail!("comment id must be greater than zero");
    }
    Ok(id)
}

fn comment_status(comment: &DocComment) -> &'static str {
    if comment.resolved { "resolved" } else { "open" }
}

fn escape_comment_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn comment_json(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
    comment: &DocComment,
) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "qname": ws.get_symbol(qname)?,
        "kind": kind_name(object.kind),
        "current_text": doc_preview(catalog, object),
        "comment_id": comment.id,
        "resolved": comment.resolved,
        "comment": comment.text,
    }))
}

fn doc_preview(catalog: &Catalog, object: &DesignObject) -> String {
    match &object.payload {
        ObjectPayload::DocumentText(p) => p.text.clone(),
        ObjectPayload::DocumentTopic(p) => p
            .paragraphs
            .iter()
            .find_map(|paragraph| catalog.get(*paragraph))
            .map(|child| doc_preview(catalog, child))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| p.title.clone()),
        ObjectPayload::DocumentList(p) => p
            .items
            .iter()
            .filter_map(|item| catalog.get(*item))
            .map(|item| doc_preview(catalog, item))
            .collect::<Vec<_>>()
            .join("；"),
        ObjectPayload::Block(p) => p.text.clone(),
        ObjectPayload::MermaidBlock(p) => p.text.clone(),
        ObjectPayload::HtmlBlock(p) => p.text.clone(),
        ObjectPayload::DocumentSection(p) => p.title.clone(),
        ObjectPayload::Document(p) => p.title.clone(),
        ObjectPayload::DocumentDirectory(_) => String::new(),
        _ => String::new(),
    }
}

fn print_comment(qname: Option<&str>, comment: &DocComment) {
    let status = comment_status(comment);
    let text = escape_comment_text(&comment.text);
    match qname {
        Some(qname) => println!("{qname}\tcomment{}\t{status}\t{text}", comment.id),
        None => println!("comment{}\t{status}\t{text}", comment.id),
    }
}

fn comments_text(comments: &[DocComment]) -> String {
    if comments.is_empty() {
        return "<none>".to_owned();
    }
    comments
        .iter()
        .map(|comment| {
            format!(
                "comment{} [{}] {}",
                comment.id,
                comment_status(comment),
                escape_comment_text(&comment.text)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn print_comments(comments: &[DocComment], indent: &str) {
    for comment in comments {
        println!(
            "{indent}comment{} [{}] {}",
            comment.id,
            comment_status(comment),
            escape_comment_text(&comment.text)
        );
    }
}

fn doc_grep(
    ws: &Workspace,
    catalog: &Catalog,
    pattern: &str,
    ignore_case: bool,
    format: OutputFormat,
) -> Result<()> {
    if pattern.is_empty() {
        bail!("grep pattern must not be empty");
    }
    let needle = if ignore_case {
        pattern.to_lowercase()
    } else {
        pattern.to_owned()
    };
    let mut rows = Vec::new();
    for (qname, object) in catalog.iter() {
        let mut text = doc_preview(catalog, object);
        match &object.payload {
            ObjectPayload::DocumentTopic(p) => {
                text = format!("{}\n{text}", p.title);
            }
            ObjectPayload::DocumentSection(p) => {
                text = format!("{}\n{text}", p.title);
            }
            ObjectPayload::Document(p) => {
                text = format!("{}\n{text}", p.title);
            }
            _ => {}
        }
        if text.is_empty() {
            continue;
        }
        let haystack = if ignore_case {
            text.to_lowercase()
        } else {
            text.clone()
        };
        if !haystack.contains(&needle) {
            continue;
        }
        rows.push((
            ws.get_symbol(qname)?,
            kind_name(object.kind).to_owned(),
            text,
        ));
    }
    if format == OutputFormat::Json {
        let values = rows
            .into_iter()
            .map(|(qname, kind, text)| {
                serde_json::json!({"qname": qname, "kind": kind, "text": text})
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&values)?);
    } else {
        for (qname, kind, text) in rows {
            println!("{qname}\t{kind}\t{}", text.replace('\n', " "));
        }
    }
    Ok(())
}

fn doc_lint(ws: &Workspace, catalog: &Catalog, rules: &str, format: OutputFormat) -> Result<()> {
    let enabled = rules
        .split(',')
        .map(str::trim)
        .filter(|rule| !rule.is_empty())
        .collect::<BTreeSet<_>>();
    let mut issues = Vec::new();
    for (topic_qname, object) in catalog.iter() {
        let ObjectPayload::DocumentTopic(topic) = &object.payload else {
            continue;
        };
        let rule = match topic.title.as_str() {
            "参数" => "parameter",
            "返回值" => "return",
            _ => continue,
        };
        if !enabled.contains(rule) {
            continue;
        }
        let topic_name = ws.get_symbol(topic_qname)?;
        let mut has_text = false;
        for paragraph in &topic.paragraphs {
            let Some(child) = catalog.get(*paragraph) else {
                continue;
            };
            let ObjectPayload::DocumentText(text) = &child.payload else {
                continue;
            };
            has_text = true;
            let child_name = ws.get_symbol(*paragraph)?;
            for line in text.text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with("<!--") {
                    continue;
                }
                let valid = match rule {
                    "parameter" => valid_parameter_line(line),
                    "return" => valid_return_line(line),
                    _ => true,
                };
                if !valid {
                    issues.push(serde_json::json!({
                        "qname": child_name,
                        "topic": topic_name,
                        "rule": rule,
                        "text": line,
                    }));
                }
            }
        }
        if !has_text {
            issues.push(serde_json::json!({
                "qname": topic_name,
                "topic": topic_name,
                "rule": rule,
                "text": "",
                "message": "topic has no text paragraph",
            }));
        }
    }
    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&issues)?);
    } else {
        for issue in &issues {
            println!(
                "{}\t{}\t{}",
                issue["qname"].as_str().unwrap_or_default(),
                issue["rule"].as_str().unwrap_or_default(),
                issue["text"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn valid_parameter_line(line: &str) -> bool {
    if line == "无参数。" {
        return true;
    }
    let Some(rest) = line.strip_prefix("参数 ") else {
        return false;
    };
    let Some((name, value)) = rest.split_once('：') else {
        return false;
    };
    !name.trim().is_empty() && !value.trim().is_empty() && line.ends_with('。')
}

fn valid_return_line(line: &str) -> bool {
    if line == "无返回值。" {
        return true;
    }
    line.starts_with("返回：") && line.ends_with('。') && line.len() > "返回：。".len()
}

fn doc_show(ws: &Workspace, catalog: &Catalog, qname: &str, format: OutputFormat) -> Result<()> {
    let hash = symbol_hash(qname);
    let object = catalog
        .get(hash)
        .with_context(|| format!("object not found: {qname}"))?;
    match format {
        OutputFormat::Text => render_doc(ws, catalog, hash, object, 0),
        OutputFormat::Json => {
            let value = doc_export_json(ws, catalog, hash, object)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        OutputFormat::Md => {
            let links = document_links(ws, catalog);
            let mut out = String::new();
            render_doc_md(ws, catalog, hash, object, 0, &links, &mut out)?;
            print!("{out}");
            Ok(())
        }
    }
}

/// Objects linked to each document section, as qname strings.
fn document_links(ws: &Workspace, catalog: &Catalog) -> BTreeMap<SymbolHash, Vec<String>> {
    let mut links: BTreeMap<SymbolHash, Vec<String>> = BTreeMap::new();
    for (qname, object) in catalog.iter() {
        let Some(section) = document_of(object).filter(|section| !section.is_zero()) else {
            continue;
        };
        if let Ok(name) = ws.get_symbol(qname) {
            links.entry(section).or_default().push(name);
        }
    }
    for names in links.values_mut() {
        names.sort();
    }
    links
}

fn doc_export_json(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
) -> Result<serde_json::Value> {
    let links = document_links(ws, catalog);
    doc_export_node(ws, catalog, qname, object, &links)
}

fn doc_export_node(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
    links: &BTreeMap<SymbolHash, Vec<String>>,
) -> Result<serde_json::Value> {
    let name = ws.get_symbol(qname)?;
    match &object.payload {
        ObjectPayload::DocumentDirectory(_) => {
            let mut documents = Vec::new();
            for child in catalog.children(qname) {
                let Some(child_object) = catalog.get(child) else {
                    continue;
                };
                if child_object.kind == Kind::Document {
                    documents.push(doc_export_node(ws, catalog, child, child_object, links)?);
                }
            }
            Ok(serde_json::json!({
                "root": {"qname": name, "kind": "directory"},
                "documents": documents,
            }))
        }
        ObjectPayload::Document(p) => {
            let mut sections = Vec::new();
            for section in &p.sections {
                let Some(child) = catalog.get(*section) else {
                    continue;
                };
                sections.push(export_section(ws, catalog, *section, child, links)?);
            }
            Ok(serde_json::json!({
                "root": {"qname": name, "kind": "document", "title": p.title},
                "sections": sections,
            }))
        }
        ObjectPayload::DocumentSection(p) => {
            let mut sections = Vec::new();
            for section in &p.sections {
                let Some(child) = catalog.get(*section) else {
                    continue;
                };
                sections.push(export_section(ws, catalog, *section, child, links)?);
            }
            Ok(serde_json::json!({
                "root": {"qname": name, "kind": "section", "title": p.title},
                "body": export_body(ws, catalog, &p.body)?,
                "sections": sections,
            }))
        }
        ObjectPayload::DocumentTopic(p) => Ok(serde_json::json!({
            "root": {"qname": name, "kind": "topic", "title": p.title},
            "body": export_body(ws, catalog, &p.paragraphs)?,
        })),
        ObjectPayload::DocumentList(_) => Ok(serde_json::json!({
            "root": {"qname": name, "kind": "list"},
            "body": export_body(ws, catalog, &[qname])?,
        })),
        ObjectPayload::DocumentText(_) => Ok(serde_json::json!({
            "root": {"qname": name, "kind": "text"},
            "body": export_body(ws, catalog, &[qname])?,
        })),
        ObjectPayload::Block(_) => Ok(serde_json::json!({
            "root": {"qname": name, "kind": "block"},
            "body": export_body(ws, catalog, &[qname])?,
        })),
        ObjectPayload::MermaidBlock(_) => Ok(serde_json::json!({
            "root": {"qname": name, "kind": "mermaid"},
            "body": export_body(ws, catalog, &[qname])?,
        })),
        ObjectPayload::HtmlBlock(_) => Ok(serde_json::json!({
            "root": {"qname": name, "kind": "html"},
            "body": export_body(ws, catalog, &[qname])?,
        })),
        _ => bail!("not a document object: {name}"),
    }
}

fn export_section(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
    links: &BTreeMap<SymbolHash, Vec<String>>,
) -> Result<serde_json::Value> {
    let name = ws.get_symbol(qname)?;
    let ObjectPayload::DocumentSection(p) = &object.payload else {
        bail!("not a document section: {name}");
    };
    let mut sections = Vec::new();
    for section in &p.sections {
        let Some(child) = catalog.get(*section) else {
            continue;
        };
        sections.push(export_section(ws, catalog, *section, child, links)?);
    }
    Ok(serde_json::json!({
        "qname": name,
        "kind": "section",
        "title": p.title,
        "document": links.get(&qname).cloned().unwrap_or_default(),
        "body": export_body(ws, catalog, &p.body)?,
        "sections": sections,
    }))
}

fn export_body(
    ws: &Workspace,
    catalog: &Catalog,
    refs: &[SymbolHash],
) -> Result<Vec<serde_json::Value>> {
    let mut out = Vec::new();
    for reference in refs {
        let Some(object) = catalog.get(*reference) else {
            continue;
        };
        let name = ws.get_symbol(*reference)?;
        let value = match &object.payload {
            ObjectPayload::DocumentText(p) => serde_json::json!({
                "type": "text",
                "qname": name,
                "text": p.text,
            }),
            ObjectPayload::DocumentList(p) => {
                let mut items = Vec::new();
                for item in &p.items {
                    if let Some(ObjectPayload::DocumentText(text)) =
                        catalog.get(*item).map(|object| &object.payload)
                    {
                        items.push(serde_json::json!({
                            "qname": ws.get_symbol(*item)?,
                            "text": text.text,
                        }));
                    }
                }
                serde_json::json!({"type": "list", "qname": name, "items": items})
            }
            ObjectPayload::Block(p) => serde_json::json!({
                "type": "block",
                "qname": name,
                "language": p.language,
                "text": p.text,
            }),
            ObjectPayload::MermaidBlock(p) => serde_json::json!({
                "type": "mermaid",
                "qname": name,
                "text": p.text,
            }),
            ObjectPayload::HtmlBlock(p) => serde_json::json!({
                "type": "html",
                "qname": name,
                "text": p.text,
            }),
            ObjectPayload::DocumentTopic(p) => serde_json::json!({
                "type": "topic",
                "qname": name,
                "title": p.title,
                "body": export_body(ws, catalog, &p.paragraphs)?,
            }),
            _ => continue,
        };
        out.push(value);
    }
    Ok(out)
}

fn render_doc_md(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
    depth: usize,
    links: &BTreeMap<SymbolHash, Vec<String>>,
    out: &mut String,
) -> Result<()> {
    match &object.payload {
        ObjectPayload::DocumentDirectory(_) => {
            if depth == 0 {
                out.push_str(&format!("# {}\n\n", ws.get_symbol(qname)?));
            }
            for child in catalog.children(qname) {
                let Some(child_object) = catalog.get(child) else {
                    continue;
                };
                if child_object.kind == Kind::Document {
                    render_doc_md(ws, catalog, child, child_object, depth + 1, links, out)?;
                }
            }
        }
        ObjectPayload::Document(p) => {
            push_heading(out, depth + 1, &p.title);
            for section in &p.sections {
                let Some(child) = catalog.get(*section) else {
                    continue;
                };
                render_section_md(ws, catalog, *section, child, depth + 2, links, out)?;
            }
        }
        ObjectPayload::DocumentSection(p) => {
            push_heading(out, depth + 1, &p.title);
            render_body_md(catalog, &p.body, depth + 2, out)?;
            render_doc_links(links, qname, out);
            for section in &p.sections {
                let Some(child) = catalog.get(*section) else {
                    continue;
                };
                render_section_md(ws, catalog, *section, child, depth + 2, links, out)?;
            }
        }
        ObjectPayload::DocumentTopic(p) => {
            push_heading(out, depth + 1, &p.title);
            render_body_md(catalog, &p.paragraphs, depth + 2, out)?;
        }
        ObjectPayload::DocumentList(p) => {
            let items = list_items(catalog, p);
            out.push_str(&format!("{}\n\n", items.join("\n")));
        }
        ObjectPayload::DocumentText(p) => out.push_str(&format!("{}\n\n", p.text)),
        ObjectPayload::Block(p) => push_fence(out, &p.language, &p.text),
        ObjectPayload::MermaidBlock(p) => push_fence(out, "mermaid", &p.text),
        ObjectPayload::HtmlBlock(p) => push_fence(out, "html", &p.text),
        _ => bail!("not a document object: {}", ws.get_symbol(qname)?),
    }
    Ok(())
}

fn render_section_md(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
    level: usize,
    links: &BTreeMap<SymbolHash, Vec<String>>,
    out: &mut String,
) -> Result<()> {
    let ObjectPayload::DocumentSection(p) = &object.payload else {
        bail!("not a document section: {}", ws.get_symbol(qname)?);
    };
    push_heading(out, level, &p.title);
    render_body_md(catalog, &p.body, level + 1, out)?;
    render_doc_links(links, qname, out);
    for section in &p.sections {
        let Some(child) = catalog.get(*section) else {
            continue;
        };
        render_section_md(ws, catalog, *section, child, level + 1, links, out)?;
    }
    Ok(())
}

fn render_body_md(
    catalog: &Catalog,
    refs: &[SymbolHash],
    level: usize,
    out: &mut String,
) -> Result<()> {
    for reference in refs {
        let Some(object) = catalog.get(*reference) else {
            continue;
        };
        match &object.payload {
            ObjectPayload::DocumentText(p) => out.push_str(&format!("{}\n\n", p.text)),
            ObjectPayload::DocumentList(p) => {
                let items = list_items(catalog, p);
                out.push_str(&items.join("\n"));
                out.push_str("\n\n");
            }
            ObjectPayload::Block(p) => push_fence(out, &p.language, &p.text),
            ObjectPayload::MermaidBlock(p) => push_fence(out, "mermaid", &p.text),
            ObjectPayload::HtmlBlock(p) => push_fence(out, "html", &p.text),
            ObjectPayload::DocumentTopic(p) => {
                push_heading(out, level, &p.title);
                render_body_md(catalog, &p.paragraphs, level + 1, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn list_items(catalog: &Catalog, list: &DocumentListPayload) -> Vec<String> {
    list.items
        .iter()
        .filter_map(|item| catalog.get(*item))
        .filter_map(|object| match &object.payload {
            ObjectPayload::DocumentText(text) => Some(format!("- {}", text.text)),
            _ => None,
        })
        .collect()
}

fn render_doc_links(
    links: &BTreeMap<SymbolHash, Vec<String>>,
    qname: SymbolHash,
    out: &mut String,
) {
    if let Some(names) = links.get(&qname)
        && !names.is_empty()
    {
        out.push_str(&format!("<!-- document: {} -->\n\n", names.join(", ")));
    }
}

fn push_heading(out: &mut String, level: usize, title: &str) {
    out.push_str(&"#".repeat(level.clamp(1, 6)));
    out.push(' ');
    out.push_str(title);
    out.push_str("\n\n");
}

fn push_fence(out: &mut String, language: &str, text: &str) {
    out.push_str("```");
    out.push_str(language);
    out.push('\n');
    out.push_str(text.trim_end_matches('\n'));
    out.push_str("\n```\n\n");
}

/// Reading order for the preset design documents: directory, namespace,
/// technical, then drafts last.
fn design_document_rank(name: &str) -> u8 {
    match name.rsplit('/').next() {
        Some("directory") => 0,
        Some("namespace") => 1,
        Some("technical") => 2,
        Some("drafts" | "proposals") => 3,
        _ => 2,
    }
}

fn render_doc(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
    depth: usize,
) -> Result<()> {
    let indent = "  ".repeat(depth);
    let name = ws.get_symbol(qname)?;
    match &object.payload {
        ObjectPayload::DocumentDirectory(_p) => {
            println!("{indent}{name}  document-directory");
            let mut children = Vec::new();
            for child in catalog.children(qname) {
                children.push((ws.get_symbol(child)?, child));
            }
            if name == "documents/design" {
                children.sort_by(|left, right| {
                    design_document_rank(&left.0)
                        .cmp(&design_document_rank(&right.0))
                        .then_with(|| left.0.cmp(&right.0))
                });
            } else {
                children.sort_by(|left, right| left.0.cmp(&right.0));
            }
            for (_, child) in children {
                let child_object = catalog.get(child).context("missing child")?;
                render_doc(ws, catalog, child, child_object, depth + 1)?;
            }
        }
        ObjectPayload::Document(p) => {
            println!("{indent}{name}  document {:?}", p.title);
            for section in &p.sections {
                let child = catalog.get(*section).context("missing section")?;
                render_doc(ws, catalog, *section, child, depth + 1)?;
            }
        }
        ObjectPayload::DocumentSection(p) => {
            println!("{indent}{name}  section {:?}", p.title);
            for paragraph in &p.body {
                let child = catalog.get(*paragraph).context("missing paragraph")?;
                render_doc(ws, catalog, *paragraph, child, depth + 1)?;
            }
            for section in &p.sections {
                let child = catalog.get(*section).context("missing subsection")?;
                render_doc(ws, catalog, *section, child, depth + 1)?;
            }
        }
        ObjectPayload::DocumentList(p) => {
            println!("{indent}{name}  list");
            for item in &p.items {
                let child = catalog.get(*item).context("missing item")?;
                render_doc(ws, catalog, *item, child, depth + 1)?;
            }
        }
        ObjectPayload::DocumentText(p) => {
            println!("{indent}{name}  text {:?}", p.text);
        }
        ObjectPayload::Block(p) => {
            println!("{indent}{name}  block {:?}", p.language);
            for line in p.text.lines() {
                println!("{indent}  {line}");
            }
        }
        ObjectPayload::MermaidBlock(p) => {
            println!("{indent}{name}  mermaid");
            for line in p.text.lines() {
                println!("{indent}  {line}");
            }
        }
        ObjectPayload::HtmlBlock(p) => {
            println!("{indent}{name}  html");
            for line in p.text.lines() {
                println!("{indent}  {line}");
            }
        }
        ObjectPayload::DocumentTopic(p) => {
            println!("{indent}{name}  topic {:?}", p.title);
            for paragraph in &p.paragraphs {
                let child = catalog.get(*paragraph).context("missing topic paragraph")?;
                render_doc(ws, catalog, *paragraph, child, depth + 1)?;
            }
        }
        _ => bail!("not a document object: {name}"),
    }
    if let Some(comments) = doc_comments(object) {
        print_comments(comments, &format!("{indent}  "));
    }
    Ok(())
}

fn remote(dir: Option<PathBuf>, command: RemoteCommand) -> Result<()> {
    let mut ws = open_workspace(dir)?;
    match command {
        RemoteCommand::Add { url } => {
            if ws.remote_url().is_some() {
                bail!("remote already configured; use `ds remote set-url`");
            }
            ws.set_remote_url(url.clone())?;
            println!("remote: {url}");
        }
        RemoteCommand::Show => match ws.remote_url() {
            Some(url) => println!("{url}"),
            None => println!("no remote configured"),
        },
        RemoteCommand::SetUrl { url } => {
            ws.set_remote_url(url.clone())?;
            println!("remote: {url}");
        }
    }
    Ok(())
}

fn remote_for(ws: &Workspace) -> Result<Remote> {
    let url = ws
        .remote_url()
        .context("no remote configured; run `ds remote add <path>`")?;
    Remote::open(url)
}

fn push(dir: Option<PathBuf>, branch: Option<String>, force: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let remote = remote_for(&ws)?;
    let branch = branch.unwrap_or(current_branch(&ws)?);
    let commit = remote.push(&ws, &branch, force)?;
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            p.source.as_ref().map(|source| source.path)
        }
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            p.source.as_ref().map(|source| source.path)
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            p.source.as_ref().map(|source| source.path)
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            p.source.as_ref().map(|source| source.path)
        }
        ObjectPayload::Method(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Function(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::Part(p) => Some(p.source.path),
        ObjectPayload::Mixin(p) => p.source.as_ref().map(|source| source.path),
        ObjectPayload::View(_)
        | ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_)
        | ObjectPayload::DesignStructure(_)
        | ObjectPayload::System(_)
        | ObjectPayload::Module(_)
        | ObjectPayload::ConceptDirectory(_)
        | ObjectPayload::Concept(_)
        | ObjectPayload::Layer(_) => None,
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            if let Some(source) = p.source.as_mut() {
                source.path = symbol;
            }
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
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
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_) => bail!("document objects have no source"),
        ObjectPayload::ConceptDirectory(_) | ObjectPayload::Concept(_) => {
            bail!("concept objects have no source")
        }
        ObjectPayload::Layer(_) => bail!("layer objects have no source"),
        ObjectPayload::DesignStructure(_) | ObjectPayload::System(_) | ObjectPayload::Module(_) => {
            bail!("structure objects have no source")
        }
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
                document: SymbolHash::ZERO,
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

fn retarget_file_associations(
    ws: &Workspace,
    catalog: &mut Catalog,
    old_path: &str,
    new_path: &str,
) -> Result<()> {
    let old_file = ws.put_symbol(&object_qname_text(old_path))?;
    let new_file = ws.put_symbol(&object_qname_text(new_path))?;
    let mut targets = Vec::new();
    for (qname, object) in catalog.iter() {
        if object_file_association(ws, object)? == Some(old_file) {
            targets.push(qname);
        }
    }
    for qname in targets {
        if let Some(object) = catalog.get_mut(qname) {
            match &mut object.payload {
                ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
                    if let ClassParent::TopLevel { file, .. } = &mut p.parent
                        && *file == old_file
                    {
                        *file = new_file;
                    }
                }
                ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
                    if let ClassParent::TopLevel { file, .. } = &mut p.parent
                        && *file == old_file
                    {
                        *file = new_file;
                    }
                }
                ObjectPayload::Mixin(p) => {
                    if let ClassParent::TopLevel { file, .. } = &mut p.parent
                        && *file == old_file
                    {
                        *file = new_file;
                    }
                }
                ObjectPayload::Function(p) => {
                    if p.file == old_file {
                        p.file = new_file;
                    }
                }
                _ => {}
            }
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
    retarget_file_associations(ws, catalog, old_path, new_path)?;
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
                            document: SymbolHash::ZERO,
                            children: vec![],
                        }),
                    ),
                )?;
                created_files += 1;
            }
        }
    }

    // Source-backed concepts whose file no longer exists are stale too.
    let mut stale = BTreeSet::new();
    for (qname, object) in catalog.iter() {
        if matches!(object.kind, Kind::File | Kind::Directory | Kind::View) {
            continue;
        }
        if let Some(path) = object_source_path(ws, object)? {
            if !file_paths.contains(&path) && !dir_paths.contains(&path) {
                stale.insert(qname);
            }
        } else if let Some(file) = object_file_association(ws, object)?
            && catalog.get(file).is_none()
        {
            stale.insert(qname);
        }
    }
    for qname in stale {
        removed += remove_source_subtree(&mut catalog, qname);
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
            repo.sync_worktree_to_head()?;
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
        SourceCommand::Worktree => {
            repo.ensure_worktree()?;
            println!("{}", repo.worktree_path().display());
        }
        SourceCommand::Status => {
            if !repo.exists() {
                println!("source: not fetched");
                return Ok(());
            }
            println!("ref:    {}", repo.current_ref()?);
            println!("commit: {}", repo.resolve_current()?);
            println!("worktree: {}", repo.worktree_path().display());
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

    if args.clear_document {
        set_document(&mut object, SymbolHash::ZERO)?;
    } else if let Some(text) = args.document.as_deref() {
        set_document(&mut object, relation_symbol(&ws, Some(text))?)?;
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
                Some(true),
            )?;
        }
        ObjectPayload::InnerClass(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
                Some(false),
            )?;
        }
        ObjectPayload::Struct(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
                Some(true),
            )?;
        }
        ObjectPayload::InnerStruct(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
                Some(false),
            )?;
        }
        ObjectPayload::Enum(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
                Some(true),
            )?;
        }
        ObjectPayload::InnerEnum(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
                Some(false),
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
                Some(true),
            )?;
        }
        ObjectPayload::InnerInterface(p) => {
            edit_class_parent(
                ws,
                catalog,
                &mut p.parent,
                &mut p.qualified_name,
                &local,
                args,
                Some(false),
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
                None,
            )?;
        }
        ObjectPayload::Method(p) => {
            if let Some(parent) = args.parent.as_deref() {
                let parent = parent_symbol(
                    ws,
                    catalog,
                    parent,
                    &[
                        Kind::Class,
                        Kind::InnerClass,
                        Kind::Interface,
                        Kind::InnerInterface,
                        Kind::Part,
                    ],
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
                let parent = parent_symbol(
                    ws,
                    catalog,
                    parent,
                    &[
                        Kind::Class,
                        Kind::InnerClass,
                        Kind::Interface,
                        Kind::InnerInterface,
                    ],
                )?;
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
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_) => {
            bail!("document objects are edited with `ds doc`")
        }
        ObjectPayload::DesignStructure(_) | ObjectPayload::System(_) | ObjectPayload::Module(_) => {
            bail!("structure objects are edited with `ds structure`")
        }
        ObjectPayload::ConceptDirectory(_) | ObjectPayload::Concept(_) => {
            bail!("concepts are edited with `ds concept`")
        }
        ObjectPayload::Layer(_) => bail!("layers are edited with `ds structure`"),
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            rename_type(ws, &p.parent, &mut p.qualified_name, name)?;
        }
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            rename_type(ws, &p.parent, &mut p.qualified_name, name)?;
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            rename_type(ws, &p.parent, &mut p.qualified_name, name)?;
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
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
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_) => {
            bail!("document fragments have fixed local names; create a new fragment")
        }
        ObjectPayload::DesignStructure(_) | ObjectPayload::System(_) | ObjectPayload::Module(_) => {
            bail!("structure objects have fixed local names; create a new object")
        }
        ObjectPayload::ConceptDirectory(_) | ObjectPayload::Concept(_) => {
            bail!("concepts are renamed with `ds concept`")
        }
        ObjectPayload::Layer(_) => bail!("layers are renamed with `ds structure`"),
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
    top_level: Option<bool>,
) -> Result<()> {
    if top_level == Some(true) && args.parent.is_some() {
        bail!("a top-level type cannot be moved under a parent; create an inner type");
    }
    if top_level == Some(false)
        && (args.namespace.is_some() || args.file.is_some() || args.module.is_some())
    {
        bail!("an inner type cannot be moved to a namespace/file/module");
    }
    if let Some(new_parent) = args.parent.as_deref() {
        let new_parent = parent_symbol(
            ws,
            catalog,
            new_parent,
            &[
                Kind::Class,
                Kind::InnerClass,
                Kind::Struct,
                Kind::InnerStruct,
                Kind::Enum,
                Kind::InnerEnum,
                Kind::Interface,
                Kind::InnerInterface,
            ],
        )?;
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

/// Human-readable Section title for a code element. Method qnames end in
/// `#<signature-hash>`; the hash is identity, not a title.
fn element_title(qname: &str) -> String {
    let without_hash = qname.split_once('#').map(|(name, _)| name).unwrap_or(qname);
    local_name(without_hash)
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

fn set_document(object: &mut DesignObject, document: SymbolHash) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::View(_) => bail!("view has no document reference"),
        ObjectPayload::Directory(p) => p.document = document,
        ObjectPayload::File(p) => p.document = document,
        ObjectPayload::Namespace(p) => p.document = document,
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => p.document = document,
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => p.document = document,
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => p.document = document,
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => p.document = document,
        ObjectPayload::Method(p) => p.document = document,
        ObjectPayload::Function(p) => p.document = document,
        ObjectPayload::Part(_) => bail!("part has no document reference"),
        ObjectPayload::Mixin(p) => p.document = document,
        ObjectPayload::Layer(p) => p.document = document,
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_)
        | ObjectPayload::ConceptDirectory(_)
        | ObjectPayload::Concept(_) => {
            bail!("document objects have no design document field")
        }
        ObjectPayload::DesignStructure(p) => p.document = document,
        ObjectPayload::System(p) => p.document = document,
        ObjectPayload::Module(p) => p.document = document,
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            let source = read_source(ws, Some(spec), true)?.context("source missing")?;
            p.source = Some(source.span_source());
            p.content_hash = source.content_hash;
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
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
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_) => bail!("document objects have no source"),
        ObjectPayload::ConceptDirectory(_) | ObjectPayload::Concept(_) => {
            bail!("concept objects have no source")
        }
        ObjectPayload::Layer(_) => bail!("layer objects have no source"),
        ObjectPayload::DesignStructure(_) | ObjectPayload::System(_) | ObjectPayload::Module(_) => {
            bail!("structure objects have no source")
        }
    }
    Ok(())
}

fn set_superclass(object: &mut DesignObject, symbol: SymbolHash) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => p.superclass = symbol,
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => select_dependency_list(
            p.explicit_dependencies.as_mut(),
            p.implicit_dependencies.as_mut(),
            explicit,
        )?,
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => select_dependency_list(
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
            if p.superclass == target {
                p.superclass = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_symbol(&mut p.interfaces, target);
            changed |= remove_symbol(&mut p.mixins, target);
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
            if p.extends == target {
                p.extends = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Method(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
            if p.overrides == target {
                p.overrides = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Function(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Part(p) => {
            changed |= remove_dependencies(&mut p.explicit_dependencies, target);
            changed |= remove_dependencies(&mut p.implicit_dependencies, target);
        }
        ObjectPayload::Mixin(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
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
        ObjectPayload::Module(p) => {
            changed |= remove_symbol(&mut p.members, target);
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
        }
        ObjectPayload::DesignStructure(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
        }
        ObjectPayload::System(p) => {
            changed |= remove_symbol(&mut p.members, target);
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
        }
        ObjectPayload::Directory(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
        }
        ObjectPayload::File(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
        }
        ObjectPayload::Namespace(p) => {
            if p.document == target {
                p.document = SymbolHash::ZERO;
                changed = true;
            }
        }
        ObjectPayload::Document(p) => {
            changed |= remove_symbol(&mut p.sections, target);
        }
        ObjectPayload::DocumentSection(p) => {
            changed |= remove_symbol(&mut p.body, target);
            changed |= remove_symbol(&mut p.sections, target);
        }
        ObjectPayload::DocumentList(p) => {
            changed |= remove_symbol(&mut p.items, target);
        }
        ObjectPayload::DocumentTopic(p) => {
            changed |= remove_symbol(&mut p.paragraphs, target);
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => remove_from(
            if explicit {
                &mut p.explicit_dependencies
            } else {
                &mut p.implicit_dependencies
            },
            target,
        ),
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => remove_from(
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            Ok((&p.explicit_dependencies, &p.implicit_dependencies))
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            Ok((&p.explicit_dependencies, &p.implicit_dependencies))
        }
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

fn add(
    dir: Option<PathBuf>,
    names: Vec<String>,
    all: bool,
    file: Option<String>,
    dir_path: Option<String>,
) -> Result<()> {
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
        if file.is_some() || dir_path.is_some() {
            bail!("use either --all, --file, or --dir");
        }
        index_catalog = work_catalog.clone();
    } else if let Some(path) = file {
        if !names.is_empty() {
            bail!("--file cannot be combined with qnames");
        }
        let targets = file_scope_qnames(&ws, &work_catalog, &index_catalog, &path)?;
        for qname in &targets {
            if work_catalog.get(*qname).is_some() {
                stage_object_with_parents(&work_catalog, &mut index_catalog, *qname)?;
            } else {
                index_catalog.remove(*qname);
            }
        }
        index_catalog.set_source_commit(work_catalog.source_commit());
        println!("Staged {} objects for file {path}", targets.len());
    } else if let Some(path) = dir_path {
        if !names.is_empty() {
            bail!("--dir cannot be combined with qnames");
        }
        let qname = ws.put_symbol(&object_qname_text(&path))?;
        if let Some(object) = work_catalog.get(qname) {
            let document = document_of(object);
            stage_object_with_parents(&work_catalog, &mut index_catalog, qname)?;
            if let Some(document) = document {
                stage_object_with_parents(&work_catalog, &mut index_catalog, document)?;
            }
        } else {
            index_catalog.remove(qname);
        }
        index_catalog.set_source_commit(work_catalog.source_commit());
        println!("Staged directory {path}");
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

fn file_scope_qnames(
    ws: &Workspace,
    left: &Catalog,
    right: &Catalog,
    path: &str,
) -> Result<BTreeSet<SymbolHash>> {
    let file_qname = ws.put_symbol(&object_qname_text(path))?;
    let prefix = format!("{}/", ws.get_symbol(file_qname)?);
    let mut qnames = BTreeSet::new();
    for catalog in [left, right] {
        for (qname, object) in catalog.iter() {
            let qname_text = ws.get_symbol(qname)?;
            let belongs = qname == file_qname
                || qname_text.starts_with(&prefix)
                || object_source_path(ws, object)?.as_deref() == Some(path)
                || object_file_association(ws, object)? == Some(file_qname);
            if belongs {
                qnames.insert(qname);
                if let Some(document) = document_of(object) {
                    qnames.insert(document);
                }
            }
        }
    }
    Ok(qnames)
}

fn directory_scope_qnames(
    ws: &Workspace,
    left: &Catalog,
    right: &Catalog,
    path: &str,
) -> Result<BTreeSet<SymbolHash>> {
    let dir_qname = ws.put_symbol(&object_qname_text(path))?;
    let dir_text = ws.get_symbol(dir_qname)?;
    let prefix = format!("{dir_text}/");
    let mut qnames = BTreeSet::new();
    for catalog in [left, right] {
        for (qname, object) in catalog.iter() {
            let qname_text = ws.get_symbol(qname)?;
            let source = object_source_path(ws, object)?;
            let source_belongs = source.as_deref().is_some_and(|source| {
                source == path
                    || source
                        .strip_prefix(path)
                        .is_some_and(|rest| rest.starts_with('/'))
            });
            let associated_file = object_file_association(ws, object)?
                .map(|file| ws.get_symbol(file))
                .transpose()?
                .is_some_and(|file| {
                    file.as_str() == dir_text.as_str() || file.starts_with(&prefix)
                });
            let belongs = qname == dir_qname
                || qname_text.starts_with(&prefix)
                || source_belongs
                || associated_file;
            if belongs {
                qnames.insert(qname);
                if let Some(document) = document_of(object) {
                    qnames.insert(document);
                }
            }
        }
    }
    Ok(qnames)
}

fn object_file_association(_ws: &Workspace, object: &DesignObject) -> Result<Option<SymbolHash>> {
    let file = match &object.payload {
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => match p.parent {
            ClassParent::TopLevel { file, .. } => Some(file),
            ClassParent::Nested { .. } => None,
        },
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => match p.parent {
            ClassParent::TopLevel { file, .. } => Some(file),
            ClassParent::Nested { .. } => None,
        },
        ObjectPayload::Mixin(p) => match p.parent {
            ClassParent::TopLevel { file, .. } => Some(file),
            ClassParent::Nested { .. } => None,
        },
        ObjectPayload::Function(p) => Some(p.file),
        _ => None,
    };
    match file {
        Some(file) if !file.is_zero() => Ok(Some(file)),
        _ => Ok(None),
    }
}

fn unstage(dir: Option<PathBuf>, file: Option<String>, dir_path: Option<String>) -> Result<()> {
    let ws = open_workspace(dir)?;
    let branch = current_branch(&ws)?;
    let head = ws.read_ref(&branch)?;
    let index = ws
        .read_index()?
        .context("index is not initialized; run `ds init`")?;
    let head_catalog = load_catalog(
        ws.store(),
        head.map(|hash| ws.get_commit(hash))
            .transpose()?
            .map(|commit| commit.view),
    )?;
    let mut index_catalog = Catalog::load(ws.store(), index.root_view)?;
    let targets = match (file, dir_path) {
        (Some(path), None) => file_scope_qnames(&ws, &index_catalog, &head_catalog, &path)?,
        (None, Some(path)) => directory_scope_qnames(&ws, &index_catalog, &head_catalog, &path)?,
        _ => bail!("use exactly one of --file or --dir"),
    };
    let mut targets = targets;
    expand_subtree_qnames(&ws, &[&index_catalog, &head_catalog], &mut targets)?;
    for qname in &targets {
        if let Some(object) = head_catalog.get(*qname) {
            if index_catalog.get(*qname).is_some() {
                index_catalog.replace(*qname, object.clone())?;
            } else {
                index_catalog.insert(*qname, object.clone())?;
            }
        } else {
            index_catalog.remove(*qname);
        }
    }
    let view = index_catalog.rebuild(ws.store())?;
    ws.write_index(State {
        base_commit: index.base_commit,
        root_view: view,
    })?;
    println!("Unstaged {} objects", targets.len());
    Ok(())
}

fn expand_subtree_qnames(
    ws: &Workspace,
    catalogs: &[&Catalog],
    targets: &mut BTreeSet<SymbolHash>,
) -> Result<()> {
    let mut prefixes = Vec::new();
    for qname in targets.iter() {
        prefixes.push(ws.get_symbol(*qname)?);
    }
    for catalog in catalogs {
        for (qname, _) in catalog.iter() {
            let text = ws.get_symbol(qname)?;
            if prefixes.iter().any(|prefix| {
                text.strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
            }) {
                targets.insert(qname);
            }
        }
    }
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
        if let Some(object) = worktree.get(current)
            && let Some(document) = document_of(object)
        {
            queue.push(document);
        }
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

fn branch_delete(dir: Option<PathBuf>, branch: &str) -> Result<()> {
    let ws = open_workspace(dir)?;
    let current = current_branch(&ws)?;
    if current == branch {
        bail!("cannot delete the current branch: {branch}");
    }
    let path = ws.meta_root().join("refs/heads").join(branch);
    if !path.is_file() {
        bail!("branch not found: {branch}");
    }
    std::fs::remove_file(&path)?;
    println!("Deleted branch {branch}");
    Ok(())
}

fn status(dir: Option<PathBuf>, porcelain: bool, detail: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let branch = current_branch(&ws)?;
    let head = ws.read_ref(&branch)?;
    let index = ws.read_index()?;
    let worktree = ws.read_worktree()?;
    if !porcelain {
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

    if porcelain {
        for (status, name) in &unstaged {
            if !detail {
                println!("{status}\t{name}");
                continue;
            }
            let qname = ws.put_symbol(name)?;
            let object = worktree_catalog
                .as_ref()
                .and_then(|catalog| catalog.get(qname))
                .or_else(|| {
                    index_catalog
                        .as_ref()
                        .and_then(|catalog| catalog.get(qname))
                });
            let (kind, source) = match object {
                Some(object) => (
                    kind_name(object.kind),
                    object_source_path(&ws, object)?.unwrap_or_default(),
                ),
                None => ("unknown", String::new()),
            };
            println!("{status}\t{kind}\t{source}\t{name}");
        }
        return Ok(());
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

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::View => "view",
        Kind::Directory => "directory",
        Kind::File => "file",
        Kind::Namespace => "namespace",
        Kind::Class => "class",
        Kind::InnerClass => "inner-class",
        Kind::Struct => "struct",
        Kind::InnerStruct => "inner-struct",
        Kind::Enum => "enum",
        Kind::InnerEnum => "inner-enum",
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
        Kind::Block => "document-block",
        Kind::MermaidBlock => "document-mermaid",
        Kind::HtmlBlock => "document-html",
        Kind::DesignStructure => "design-structure",
        Kind::System => "system",
        Kind::Module => "module",
        Kind::ConceptDirectory => "concept-directory",
        Kind::Concept => "concept",
        Kind::Layer => "layer",
    }
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
            fields.insert("documents", p.documents.to_hex());
            fields.insert("design", format_hash(p.design.0));
            fields.insert("concepts", format_hash(p.concepts.0));
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
            fields.insert("document", symbol_text(ws, p.document));
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
            fields.insert("document", symbol_text(ws, p.document));
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
            fields.insert("document", symbol_text(ws, p.document));
        }
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
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
            fields.insert("document", symbol_text(ws, p.document));
        }
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            fields.insert("parent", class_parent_text(ws, &p.parent));
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
            fields.insert("document", symbol_text(ws, p.document));
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            fields.insert("parent", class_parent_text(ws, &p.parent));
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
            fields.insert("document", symbol_text(ws, p.document));
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
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
            fields.insert("document", symbol_text(ws, p.document));
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
            fields.insert("document", symbol_text(ws, p.document));
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
            fields.insert("document", symbol_text(ws, p.document));
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
            fields.insert("document", symbol_text(ws, p.document));
        }
        ObjectPayload::DocumentDirectory(p) => {
            fields.insert("parent", parent_ref_text(ws, p.parent));
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::Document(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("title", p.title.clone());
            fields.insert("sections", symbol_list_text(ws, &p.sections));
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::DocumentSection(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("title", p.title.clone());
            fields.insert("body", symbol_list_text(ws, &p.body));
            fields.insert("sections", symbol_list_text(ws, &p.sections));
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::DocumentList(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("items", symbol_list_text(ws, &p.items));
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::DocumentText(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("text", p.text.clone());
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::DocumentTopic(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("title", p.title.clone());
            fields.insert("paragraphs", symbol_list_text(ws, &p.paragraphs));
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::Block(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("language", p.language.clone());
            fields.insert("text", p.text.clone());
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::MermaidBlock(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("text", p.text.clone());
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::HtmlBlock(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("text", p.text.clone());
            fields.insert("comments", comments_text(&p.comments));
        }
        ObjectPayload::DesignStructure(p) => {
            fields.insert("parent", parent_ref_text(ws, p.parent));
            fields.insert("title", p.title.clone());
            fields.insert("document", symbol_text(ws, p.document));
        }
        ObjectPayload::System(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("title", p.title.clone());
            fields.insert("document", symbol_text(ws, p.document));
            fields.insert("members", symbol_list_text(ws, &p.members));
        }
        ObjectPayload::Module(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("title", p.title.clone());
            fields.insert("document", symbol_text(ws, p.document));
            fields.insert("members", symbol_list_text(ws, &p.members));
        }
        ObjectPayload::ConceptDirectory(p) => {
            fields.insert("parent", parent_ref_text(ws, p.parent));
        }
        ObjectPayload::Concept(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("name", p.name.clone());
            fields.insert("summary", p.summary.clone());
            fields.insert("explanation", p.explanation.clone());
            fields.insert("characteristics", p.characteristics.join(" | "));
            fields.insert("relations", p.relations.join(" | "));
            fields.insert("aliases", p.aliases.join(", "));
        }
        ObjectPayload::Layer(p) => {
            fields.insert("parent", symbol_text(ws, p.parent));
            fields.insert("title", p.title.clone());
            fields.insert("order", p.order.to_string());
            fields.insert("document", symbol_text(ws, p.document));
            fields.insert("members", symbol_list_text(ws, &p.members));
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

fn rm(dir: Option<PathBuf>, name: &str, force: bool, recursive: bool) -> Result<()> {
    let ws = open_workspace(dir)?;
    let state = ws
        .read_worktree()?
        .context("worktree is not initialized; run `ds init`")?;
    let mut catalog = Catalog::load(ws.store(), state.root_view)?;
    let qname = ws.put_symbol(name)?;
    if qname == symbol_hash("files")
        || qname == symbol_hash("namespaces")
        || qname == symbol_hash("documents")
    {
        bail!("cannot remove a root object");
    }
    if catalog.get(qname).is_none() {
        bail!("object not found: {name}");
    }

    if recursive {
        let mut order = Vec::new();
        collect_subtree(&catalog, qname, &mut order);
        let subtree = order.iter().copied().collect::<BTreeSet<_>>();
        for object in &order {
            remove_object(&ws, &mut catalog, *object, force, &subtree)?;
        }
        println!("Removed {} object(s) under {name}", order.len());
    } else {
        remove_object(&ws, &mut catalog, qname, force, &BTreeSet::new())?;
    }

    let view = catalog.rebuild(ws.store())?;
    ws.write_worktree(State {
        base_commit: state.base_commit,
        root_view: view,
    })?;
    Ok(())
}

fn collect_subtree(catalog: &Catalog, qname: SymbolHash, out: &mut Vec<SymbolHash>) {
    for child in catalog.children(qname) {
        collect_subtree(catalog, child, out);
    }
    out.push(qname);
}

fn remove_object(
    ws: &Workspace,
    catalog: &mut Catalog,
    qname: SymbolHash,
    force: bool,
    subtree: &BTreeSet<SymbolHash>,
) -> Result<()> {
    let name = ws.get_symbol(qname)?;
    let children = catalog.children(qname);
    if !children.is_empty() {
        let names = children
            .iter()
            .map(|child| symbol_text(ws, *child))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("object has children; remove them first: {names}");
    }
    for module in catalog.references_to(qname) {
        let Some(object) = catalog.get_mut(module) else {
            continue;
        };
        if structure_members(object).contains(&qname) {
            remove_references_to(object, qname);
            println!("Unassigned from {}", symbol_text(ws, module));
        }
    }
    let references = catalog
        .references_to(qname)
        .into_iter()
        .filter(|reference| !subtree.contains(reference))
        .collect::<Vec<_>>();
    if !references.is_empty() {
        if !force {
            let names = references
                .iter()
                .map(|reference| symbol_text(ws, *reference))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("object is referenced by: {names}");
        }
        let mut changed = Vec::new();
        for reference in references {
            if let Some(object) = catalog.get_mut(reference)
                && remove_references_to(object, qname)
            {
                changed.push(symbol_text(ws, reference));
            }
        }
        changed.sort();
        for name in changed {
            println!("Unreferenced from {name}");
        }
    }
    // Document fragments are referenced by their parent's ordered collection.
    // Remove that containment reference before deleting the child.
    for parent in catalog.parents(qname) {
        if let Some(object) = catalog.get_mut(parent)
            && remove_references_to(object, qname)
        {
            println!("Removed from {}", symbol_text(ws, parent));
        }
    }
    catalog.remove(qname);
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
    documents: bool,
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
            !is_document_kind(object.kind) && !matches!(object.kind, Kind::Directory | Kind::File)
        } else if documents {
            is_document_kind(object.kind)
        } else if design {
            document_of(object).is_some()
        } else {
            true
        };
        if selected {
            rows.push((symbol_text(&ws, qname), document_of(object)));
        }
    }
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, document) in rows {
        if design {
            let document = document
                .map(|target| symbol_text(&ws, target))
                .unwrap_or_else(|| "<none>".to_owned());
            println!("{name}\t{document}");
        } else {
            println!("{name}");
        }
    }
    Ok(())
}

fn is_document_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::DocumentDirectory
            | Kind::Document
            | Kind::DocumentSection
            | Kind::DocumentList
            | Kind::DocumentText
            | Kind::DocumentTopic
            | Kind::Block
            | Kind::MermaidBlock
            | Kind::HtmlBlock
    )
}

fn document_of(object: &DesignObject) -> Option<SymbolHash> {
    let target = match &object.payload {
        ObjectPayload::View(_) => return None,
        ObjectPayload::Directory(p) => p.document,
        ObjectPayload::File(p) => p.document,
        ObjectPayload::Namespace(p) => p.document,
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => p.document,
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => p.document,
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => p.document,
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => p.document,
        ObjectPayload::Method(p) => p.document,
        ObjectPayload::Function(p) => p.document,
        ObjectPayload::Mixin(p) => p.document,
        ObjectPayload::DesignStructure(p) => p.document,
        ObjectPayload::System(p) => p.document,
        ObjectPayload::Module(p) => p.document,
        ObjectPayload::Layer(p) => p.document,
        ObjectPayload::Part(_)
        | ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_)
        | ObjectPayload::ConceptDirectory(_)
        | ObjectPayload::Concept(_) => return None,
    };
    (!target.is_zero()).then_some(target)
}

fn show(
    dir: Option<PathBuf>,
    name: &str,
    source: bool,
    staged: bool,
    format: OutputFormat,
) -> Result<()> {
    if format == OutputFormat::Text {
        return show_text(dir, name, source, staged);
    }
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
    if is_document_object(object) {
        match format {
            OutputFormat::Json => {
                let value = doc_export_json(&ws, &catalog, qname, object)?;
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            OutputFormat::Md => {
                let links = document_links(&ws, &catalog);
                let mut out = String::new();
                render_doc_md(&ws, &catalog, qname, object, 0, &links, &mut out)?;
                print!("{out}");
            }
            OutputFormat::Text => unreachable!(),
        }
        return Ok(());
    }
    match format {
        OutputFormat::Json => {
            let value = object_json(&ws, &catalog, qname, object)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        OutputFormat::Md => {
            let mut out = String::new();
            render_object_md(&ws, &catalog, qname, object, &mut out)?;
            print!("{out}");
        }
        OutputFormat::Text => unreachable!(),
    }
    Ok(())
}

fn is_document_object(object: &DesignObject) -> bool {
    matches!(
        object.kind,
        Kind::DocumentDirectory
            | Kind::Document
            | Kind::DocumentSection
            | Kind::DocumentList
            | Kind::DocumentText
            | Kind::DocumentTopic
            | Kind::Block
            | Kind::MermaidBlock
            | Kind::HtmlBlock
    )
}

fn object_json(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
) -> Result<serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert("qname".into(), serde_json::json!(ws.get_symbol(qname)?));
    map.insert("kind".into(), serde_json::json!(kind_name(object.kind)));
    match &object.payload {
        ObjectPayload::View(_) => {}
        ObjectPayload::Directory(p) => {
            insert_path_source(&mut map, "source", ws, p.source)?;
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::File(p) => {
            insert_path_source(&mut map, "source", ws, p.source)?;
            insert_hash(&mut map, "content_hash", p.content_hash);
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::Namespace(p) => {
            insert_span_source(&mut map, "source", ws, p.source)?;
            insert_hash(&mut map, "content_hash", p.content_hash);
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            map.insert(
                "parent".into(),
                serde_json::json!(class_parent_text(ws, &p.parent)),
            );
            insert_span_source(&mut map, "source", ws, p.source)?;
            insert_hash(&mut map, "content_hash", p.content_hash);
            insert_symbol(&mut map, "superclass", ws, p.superclass);
            insert_symbols(&mut map, "interfaces", ws, &p.interfaces);
            insert_symbols(&mut map, "mixins", ws, &p.mixins);
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            map.insert(
                "parent".into(),
                serde_json::json!(class_parent_text(ws, &p.parent)),
            );
            insert_span_source(&mut map, "source", ws, p.source)?;
            insert_hash(&mut map, "content_hash", p.content_hash);
            insert_symbol(&mut map, "extends", ws, p.extends);
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::Method(p) => {
            insert_symbol(&mut map, "parent", ws, p.parent);
            insert_span_source(&mut map, "source", ws, p.source)?;
            insert_hash(&mut map, "content_hash", p.content_hash);
            insert_symbol(&mut map, "overrides", ws, p.overrides);
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::Function(p) => {
            insert_symbol(&mut map, "file", ws, p.file);
            insert_symbol(&mut map, "namespace", ws, p.namespace);
            insert_symbol(&mut map, "module", ws, p.module);
            insert_span_source(&mut map, "source", ws, p.source)?;
            insert_hash(&mut map, "content_hash", p.content_hash);
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::Part(p) => {
            insert_symbol(&mut map, "parent", ws, p.parent);
            insert_span_source(&mut map, "source", ws, Some(p.source))?;
            insert_hash(&mut map, "content_hash", p.content_hash);
        }
        ObjectPayload::Mixin(p) => {
            map.insert(
                "parent".into(),
                serde_json::json!(class_parent_text(ws, &p.parent)),
            );
            insert_span_source(&mut map, "source", ws, p.source)?;
            insert_hash(&mut map, "content_hash", p.content_hash);
            insert_symbols(&mut map, "on", ws, &p.on);
            insert_symbol(&mut map, "superclass", ws, p.superclass);
            insert_symbols(&mut map, "interfaces", ws, &p.interfaces);
            insert_symbols(&mut map, "mixins", ws, &p.mixins);
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::DesignStructure(p) => {
            map.insert(
                "parent".into(),
                serde_json::json!(parent_ref_text(ws, p.parent)),
            );
            map.insert("title".into(), serde_json::json!(p.title));
            insert_symbol(&mut map, "document", ws, p.document);
        }
        ObjectPayload::System(p) => {
            insert_symbol(&mut map, "parent", ws, p.parent);
            map.insert("title".into(), serde_json::json!(p.title));
            insert_symbol(&mut map, "document", ws, p.document);
            insert_symbols(&mut map, "members", ws, &p.members);
        }
        ObjectPayload::Module(p) => {
            insert_symbol(&mut map, "parent", ws, p.parent);
            map.insert("title".into(), serde_json::json!(p.title));
            insert_symbol(&mut map, "document", ws, p.document);
            insert_symbols(&mut map, "members", ws, &p.members);
        }
        ObjectPayload::Layer(p) => {
            insert_symbol(&mut map, "parent", ws, p.parent);
            map.insert("title".into(), serde_json::json!(p.title));
            map.insert("order".into(), serde_json::json!(p.order));
            insert_symbol(&mut map, "document", ws, p.document);
            insert_symbols(&mut map, "members", ws, &p.members);
        }
        ObjectPayload::ConceptDirectory(p) => {
            map.insert(
                "parent".into(),
                serde_json::json!(parent_ref_text(ws, p.parent)),
            );
        }
        ObjectPayload::Concept(p) => {
            insert_symbol(&mut map, "parent", ws, p.parent);
            map.insert("name".into(), serde_json::json!(p.name));
            map.insert("summary".into(), serde_json::json!(p.summary));
            map.insert("explanation".into(), serde_json::json!(p.explanation));
            map.insert(
                "characteristics".into(),
                serde_json::json!(p.characteristics),
            );
            map.insert("relations".into(), serde_json::json!(p.relations));
            map.insert("aliases".into(), serde_json::json!(p.aliases));
        }
        _ => {}
    }
    let children = catalog.children(qname);
    if !children.is_empty() {
        let names = children
            .iter()
            .map(|child| symbol_text(ws, *child))
            .collect::<Vec<_>>();
        map.insert("children".into(), serde_json::json!(names));
    }
    Ok(serde_json::Value::Object(map))
}

fn insert_symbol(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    ws: &Workspace,
    symbol: SymbolHash,
) {
    if !symbol.is_zero() {
        map.insert(key.into(), serde_json::json!(symbol_text(ws, symbol)));
    }
}

fn insert_symbols(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    ws: &Workspace,
    symbols: &[SymbolHash],
) {
    if !symbols.is_empty() {
        let names = symbols
            .iter()
            .map(|symbol| symbol_text(ws, *symbol))
            .collect::<Vec<_>>();
        map.insert(key.into(), serde_json::json!(names));
    }
}

fn insert_hash(map: &mut serde_json::Map<String, serde_json::Value>, key: &str, hash: Hash32) {
    if !hash.is_zero() {
        map.insert(key.into(), serde_json::json!(hash.to_hex()));
    }
}

fn insert_path_source(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    ws: &Workspace,
    source: Option<PathSource>,
) -> Result<()> {
    if let Some(source) = source {
        map.insert(key.into(), serde_json::json!(ws.get_symbol(source.path)?));
    }
    Ok(())
}

fn insert_span_source(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    ws: &Workspace,
    source: Option<SpanSource>,
) -> Result<()> {
    if let Some(source) = source {
        map.insert(
            key.into(),
            serde_json::json!(format!(
                "{}:{}-{} bytes {}-{}",
                ws.get_symbol(source.path)?,
                source.span.start_line,
                source.span.end_line,
                source.span.start_byte,
                source.span.end_byte
            )),
        );
    }
    Ok(())
}

fn render_object_md(
    ws: &Workspace,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
    out: &mut String,
) -> Result<()> {
    let value = object_json(ws, catalog, qname, object)?;
    out.push_str(&format!("# {}\n\n", ws.get_symbol(qname)?));
    if let Some(map) = value.as_object() {
        for (key, value) in map {
            if key == "qname" {
                continue;
            }
            match value {
                serde_json::Value::String(text) => out.push_str(&format!("- {key}: {text}\n")),
                serde_json::Value::Number(number) => out.push_str(&format!("- {key}: {number}\n")),
                serde_json::Value::Array(items) => {
                    let text = items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!("- {key}: {text}\n"));
                }
                _ => {}
            }
        }
    }
    out.push('\n');
    Ok(())
}

fn show_text(dir: Option<PathBuf>, name: &str, source: bool, staged: bool) -> Result<()> {
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
            print_document(&ws, p.document);
        }
        ObjectPayload::File(p) => {
            print_source_path(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_document(&ws, p.document);
        }
        ObjectPayload::Namespace(p) => {
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_document(&ws, p.document);
        }
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_optional_symbol("superclass", p.superclass, &ws)?;
            print_symbol_list("interfaces", &p.interfaces, &ws)?;
            print_symbol_list("mixins", &p.mixins, &ws)?;
            print_document(&ws, p.document);
        }
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_document(&ws, p.document);
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_document(&ws, p.document);
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_optional_symbol("extends", p.extends, &ws)?;
            print_document(&ws, p.document);
        }
        ObjectPayload::Method(p) => {
            print_optional_symbol("parent", p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_optional_symbol("overrides", p.overrides, &ws)?;
            print_document(&ws, p.document);
        }
        ObjectPayload::Function(p) => {
            print_optional_symbol("file", p.file, &ws)?;
            print_optional_symbol("namespace", p.namespace, &ws)?;
            print_optional_symbol("module", p.module, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_document(&ws, p.document);
        }
        ObjectPayload::Part(p) => {
            print_optional_symbol("parent", p.parent, &ws)?;
            print_span_source(Some(p.source), &ws)?;
            print_content_hash(p.content_hash);
        }
        ObjectPayload::Mixin(p) => {
            print_class_parent(p.parent, &ws)?;
            print_span_source(p.source, &ws)?;
            print_content_hash(p.content_hash);
            print_symbol_list("on", &p.on, &ws)?;
            print_optional_symbol("superclass", p.superclass, &ws)?;
            print_symbol_list("interfaces", &p.interfaces, &ws)?;
            print_symbol_list("mixins", &p.mixins, &ws)?;
            print_document(&ws, p.document);
        }
        ObjectPayload::DocumentDirectory(p) => {
            println!("parent: {}", parent_ref_text(&ws, p.parent));
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::Document(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("title: {}", p.title);
            print_symbol_list("sections", &p.sections, &ws)?;
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::DocumentSection(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("title: {}", p.title);
            print_symbol_list("body", &p.body, &ws)?;
            print_symbol_list("sections", &p.sections, &ws)?;
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::DocumentList(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            print_symbol_list("items", &p.items, &ws)?;
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::DocumentText(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("text: {}", p.text);
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::Block(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("language: {}", p.language);
            println!("text: {}", p.text);
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::MermaidBlock(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("text: {}", p.text);
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::HtmlBlock(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("text: {}", p.text);
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::DocumentTopic(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("title: {}", p.title);
            print_symbol_list("paragraphs", &p.paragraphs, &ws)?;
            println!("comments: {}", comments_text(&p.comments));
        }
        ObjectPayload::DesignStructure(p) => {
            println!("parent: {}", parent_ref_text(&ws, p.parent));
            println!("title: {}", p.title);
            print_document(&ws, p.document);
        }
        ObjectPayload::System(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("title: {}", p.title);
            print_document(&ws, p.document);
            print_symbol_list("members", &p.members, &ws)?;
        }
        ObjectPayload::Module(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("title: {}", p.title);
            print_document(&ws, p.document);
            print_symbol_list("members", &p.members, &ws)?;
        }
        ObjectPayload::Layer(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("title: {}", p.title);
            println!("order: {}", p.order);
            print_document(&ws, p.document);
            print_symbol_list("members", &p.members, &ws)?;
        }
        ObjectPayload::ConceptDirectory(p) => {
            println!("parent: {}", parent_ref_text(&ws, p.parent));
        }
        ObjectPayload::Concept(p) => {
            println!("parent: {}", symbol_text(&ws, p.parent));
            println!("name: {}", p.name);
            println!("summary: {}", p.summary);
            println!("explanation: {}", p.explanation);
            print_string_list("characteristics", &p.characteristics);
            print_string_list("relations", &p.relations);
            print_string_list("aliases", &p.aliases);
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => span_anchor(p.source)?,
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => span_anchor(p.source)?,
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => span_anchor(p.source)?,
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => span_anchor(p.source)?,
        ObjectPayload::Method(p) => span_anchor(p.source)?,
        ObjectPayload::Function(p) => span_anchor(p.source)?,
        ObjectPayload::Part(p) => (p.source.path, Some(p.source.span)),
        ObjectPayload::Mixin(p) => span_anchor(p.source)?,
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_)
        | ObjectPayload::DesignStructure(_)
        | ObjectPayload::System(_)
        | ObjectPayload::Module(_)
        | ObjectPayload::ConceptDirectory(_)
        | ObjectPayload::Concept(_)
        | ObjectPayload::Layer(_) => bail!("object has no source anchor"),
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
    let _lock = RepoLock::acquire(store.root())?;
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
    let _lock = RepoLock::acquire(ws.store().root())?;
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
    for root in [p.files, p.namespaces, p.documents, p.design, p.concepts] {
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
            out.push(p.document);
            push_parent_ref(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
        }
        ObjectPayload::File(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            out.push(p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
        }
        ObjectPayload::Namespace(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            push_parent_ref(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
        }
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
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
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            push_class_parent(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            push_class_parent(&mut out, p.parent);
            if let Some(source) = p.source {
                out.push(source.path);
            }
            push_deps(&mut out, &p.explicit_dependencies);
            push_deps(&mut out, &p.implicit_dependencies);
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
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
            out.push(p.document);
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
            out.push(p.document);
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
            out.push(p.document);
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
        ObjectPayload::DocumentDirectory(p) => {
            out.push(p.qualified_name);
            push_parent_ref(&mut out, p.parent);
        }
        ObjectPayload::Document(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
            out.extend(p.sections.iter().copied());
        }
        ObjectPayload::DocumentSection(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
            out.extend(p.body.iter().copied());
            out.extend(p.sections.iter().copied());
        }
        ObjectPayload::DocumentList(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
            out.extend(p.items.iter().copied());
        }
        ObjectPayload::DocumentText(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
        }
        ObjectPayload::Block(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
        }
        ObjectPayload::MermaidBlock(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
        }
        ObjectPayload::HtmlBlock(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
        }
        ObjectPayload::DocumentTopic(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
            out.extend(p.paragraphs.iter().copied());
        }
        ObjectPayload::DesignStructure(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            push_parent_ref(&mut out, p.parent);
        }
        ObjectPayload::System(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            out.push(p.parent);
            out.extend(p.members.iter().copied());
        }
        ObjectPayload::Module(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            out.push(p.parent);
            out.extend(p.members.iter().copied());
        }
        ObjectPayload::Layer(p) => {
            out.push(p.qualified_name);
            out.push(p.document);
            out.push(p.parent);
            out.extend(p.members.iter().copied());
        }
        ObjectPayload::ConceptDirectory(p) => {
            out.push(p.qualified_name);
            push_parent_ref(&mut out, p.parent);
        }
        ObjectPayload::Concept(p) => {
            out.push(p.qualified_name);
            out.push(p.parent);
        }
    }
    out.retain(|hash| !hash.is_zero());
    out
}

fn object_docs(object: &DesignObject) -> Vec<DocHash> {
    let mut out = Vec::new();
    match &object.payload {
        ObjectPayload::View(_) => {}
        ObjectPayload::Directory(_) | ObjectPayload::File(_) | ObjectPayload::Namespace(_) => {}
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Method(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Function(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Part(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::Mixin(p) => {
            out.extend(p.explicit_dependencies.iter().map(|d| d.rationale));
            out.extend(p.implicit_dependencies.iter().map(|d| d.rationale));
        }
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::Block(_)
        | ObjectPayload::MermaidBlock(_)
        | ObjectPayload::HtmlBlock(_)
        | ObjectPayload::DocumentTopic(_)
        | ObjectPayload::DesignStructure(_)
        | ObjectPayload::System(_)
        | ObjectPayload::Module(_)
        | ObjectPayload::ConceptDirectory(_)
        | ObjectPayload::Concept(_)
        | ObjectPayload::Layer(_) => {}
    }
    out.retain(|hash| !hash.is_zero());
    out
}

fn object_children(object: &DesignObject) -> &[ObjectHash] {
    match &object.payload {
        ObjectPayload::Directory(p) => &p.children,
        ObjectPayload::File(p) => &p.children,
        ObjectPayload::Namespace(p) => &p.children,
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => &p.children,
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => &p.children,
        ObjectPayload::Part(p) => &p.children,
        ObjectPayload::Mixin(p) => &p.children,
        ObjectPayload::DocumentDirectory(p) => &p.children,
        ObjectPayload::Document(p) => &p.children,
        ObjectPayload::DocumentSection(p) => &p.children,
        ObjectPayload::DocumentList(p) => &p.children,
        ObjectPayload::DocumentTopic(p) => &p.children,
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
    let document = optional_ref(ws, args.document.as_deref())?;
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
                    document,
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
                    document,
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
                    document,
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
                    document,
                    superclass: relation_symbol(ws, args.superclass.as_deref())?,
                    interfaces: relation_symbols(ws, &args.interfaces)?,
                    mixins: relation_symbols(ws, &args.mixins)?,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::InnerClass => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let parent = match parent {
                ClassParent::Nested { parent } => parent,
                ClassParent::TopLevel { .. } => {
                    bail!("inner class requires a nested parent")
                }
            };
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::InnerClass,
                ObjectPayload::InnerClass(ClassPayload {
                    qualified_name: qname,
                    parent: ClassParent::Nested { parent },
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    document,
                    superclass: relation_symbol(ws, args.superclass.as_deref())?,
                    interfaces: relation_symbols(ws, &args.interfaces)?,
                    mixins: relation_symbols(ws, &args.mixins)?,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::Struct => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Struct,
                ObjectPayload::Struct(StructPayload {
                    qualified_name: qname,
                    parent,
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    document,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::InnerStruct => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let parent = match parent {
                ClassParent::Nested { parent } => parent,
                ClassParent::TopLevel { .. } => {
                    bail!("inner struct requires a nested parent")
                }
            };
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::InnerStruct,
                ObjectPayload::InnerStruct(StructPayload {
                    qualified_name: qname,
                    parent: ClassParent::Nested { parent },
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    document,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::Enum => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::Enum,
                ObjectPayload::Enum(EnumPayload {
                    qualified_name: qname,
                    parent,
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    document,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::InnerEnum => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let parent = match parent {
                ClassParent::Nested { parent } => parent,
                ClassParent::TopLevel { .. } => {
                    bail!("inner enum requires a nested parent")
                }
            };
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::InnerEnum,
                ObjectPayload::InnerEnum(EnumPayload {
                    qualified_name: qname,
                    parent: ClassParent::Nested { parent },
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    document,
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
                    document,
                    extends: relation_symbol(ws, args.extends.as_deref())?,
                    explicit_dependencies: vec![],
                    implicit_dependencies: vec![],
                    children: vec![],
                }),
            )
        }
        KindArg::InnerInterface => {
            let (qname, parent) = code_qname_and_parent(ws, catalog, args)?;
            let parent = match parent {
                ClassParent::Nested { parent } => parent,
                ClassParent::TopLevel { .. } => {
                    bail!("inner interface requires a nested parent")
                }
            };
            let source = read_source(ws, args.source.as_deref(), true)?;
            let content = source
                .as_ref()
                .map(|s| s.content_hash)
                .unwrap_or(Hash32::ZERO);
            DesignObject::new(
                Kind::InnerInterface,
                ObjectPayload::InnerInterface(InterfacePayload {
                    qualified_name: qname,
                    parent: ClassParent::Nested { parent },
                    source: source.map(|s| s.span_source()),
                    content_hash: content,
                    document,
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
                &[
                    Kind::Class,
                    Kind::InnerClass,
                    Kind::Interface,
                    Kind::InnerInterface,
                    Kind::Mixin,
                ],
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
                    document,
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
                    document,
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
                    document,
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
        KindArg::Class | KindArg::Struct | KindArg::Enum | KindArg::Interface => {
            if args.parent.is_some() {
                bail!(
                    "a top-level type has no parent; use an inner-* kind \
                     for a nested type"
                );
            }
            if args.namespace.is_none() {
                bail!("a top-level type requires --namespace");
            }
        }
        KindArg::InnerClass
        | KindArg::InnerStruct
        | KindArg::InnerEnum
        | KindArg::InnerInterface => {
            if args.parent.is_none() {
                bail!("an inner type requires --parent");
            }
            if args.namespace.is_some() || args.file.is_some() || args.module.is_some() {
                bail!(
                    "an inner type uses --parent; do not also pass \
                     --namespace/--file/--module"
                );
            }
        }
        KindArg::Mixin => {
            if args.parent.is_some() {
                if args.namespace.is_some() || args.file.is_some() || args.module.is_some() {
                    bail!(
                        "a nested mixin uses --parent; do not also pass \
                         --namespace/--file/--module"
                    );
                }
            } else if args.namespace.is_none() {
                bail!("a top-level mixin requires --namespace");
            }
        }
        KindArg::Function => {
            if args.parent.is_some() {
                bail!("function does not take --parent; use --namespace");
            }
        }
        KindArg::Method | KindArg::Part => {
            if args.namespace.is_some() || args.file.is_some() || args.module.is_some() {
                bail!(
                    "{} does not take --namespace/--file/--module",
                    match args.kind {
                        KindArg::Method => "method",
                        _ => "part",
                    }
                );
            }
            if matches!(args.kind, KindArg::Part) && args.document.is_some() {
                bail!("part does not take --document");
            }
        }
        KindArg::Directory | KindArg::File | KindArg::Namespace => {
            if args.namespace.is_some() || args.file.is_some() || args.module.is_some() {
                bail!(
                    "{} does not take --namespace/--file/--module",
                    match args.kind {
                        KindArg::Directory => "directory",
                        KindArg::File => "file",
                        _ => "namespace",
                    }
                );
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
            Kind::Class
                | Kind::InnerClass
                | Kind::Struct
                | Kind::InnerStruct
                | Kind::Enum
                | Kind::InnerEnum
                | Kind::Interface
                | Kind::InnerInterface
                | Kind::Mixin
        ) {
            bail!("a nested type parent must be a type: {parent:?}");
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => Some(p.qualified_name),
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => Some(p.qualified_name),
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => Some(p.qualified_name),
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => Some(p.qualified_name),
        ObjectPayload::Method(p) => Some(p.qualified_name),
        ObjectPayload::Function(p) => Some(p.qualified_name),
        ObjectPayload::Part(p) => Some(p.qualified_name),
        ObjectPayload::Mixin(p) => Some(p.qualified_name),
        ObjectPayload::DocumentDirectory(p) => Some(p.qualified_name),
        ObjectPayload::Document(p) => Some(p.qualified_name),
        ObjectPayload::DocumentSection(p) => Some(p.qualified_name),
        ObjectPayload::DocumentList(p) => Some(p.qualified_name),
        ObjectPayload::DocumentText(p) => Some(p.qualified_name),
        ObjectPayload::DocumentTopic(p) => Some(p.qualified_name),
        ObjectPayload::Block(p) => Some(p.qualified_name),
        ObjectPayload::MermaidBlock(p) => Some(p.qualified_name),
        ObjectPayload::HtmlBlock(p) => Some(p.qualified_name),
        ObjectPayload::DesignStructure(p) => Some(p.qualified_name),
        ObjectPayload::System(p) => Some(p.qualified_name),
        ObjectPayload::Module(p) => Some(p.qualified_name),
        ObjectPayload::ConceptDirectory(p) => Some(p.qualified_name),
        ObjectPayload::Concept(p) => Some(p.qualified_name),
        ObjectPayload::Layer(p) => Some(p.qualified_name),
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

fn print_document(ws: &Workspace, target: SymbolHash) {
    println!("document: {}", symbol_text(ws, target));
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

fn print_string_list(label: &str, items: &[String]) {
    for item in items {
        println!("{label}: {item}");
    }
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
