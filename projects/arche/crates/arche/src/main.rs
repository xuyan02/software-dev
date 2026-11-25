use std::path::PathBuf;

use std::collections::BTreeSet;
use std::io::Read;

use anyhow::{Context, Result, bail};
use arche_core::snapshot::Snapshot;
use arche_core::{
    Object, ObjectHash, Repository, SymbolHash, parse_priority, priority_display, priority_slug,
};
use arche_server::{ServerOptions, serve, serve_stdio};
use clap::{Parser, Subcommand};

mod help;

#[derive(Debug, Parser)]
#[command(
    name = "arche",
    version,
    about = "A versioned design repository",
    long_about = "Arche is a versioned design repository for technical, requirement and concept \
design objects. Documents are object fields; Arche does not parse source ASTs.",
    after_help = "Run `arche help` for full built-in documentation: objects, fields, design, \
writing, writing-detail, kinds, naming, technical, requirement, concept, document, staging, \
source, remote, examples.",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Field arguments shared by `create` and `update`.
///
/// `arche update` accepts the same field flags as `arche create`, plus generic
/// `--field NAME=VALUE` and `--ref FIELD=TARGET[,TARGET...]` escape hatches.
#[derive(Debug, clap::Args)]
struct ObjectFieldArgs {
    /// Parent qname for nested objects.
    #[arg(long)]
    parent: Option<String>,
    /// Namespace qname for a code type or function.
    #[arg(long)]
    namespace: Option<String>,
    /// Canonical prototype text; required for technical-method.
    #[arg(long)]
    prototype: Option<String>,
    /// Source span (`path:start-end`) for a code element or Part.
    #[arg(long)]
    source: Option<String>,
    /// Source file qname for a code type or function.
    #[arg(long)]
    file: Option<String>,
    /// Module qname for a code type or function.
    #[arg(long)]
    module: Option<String>,
    /// Fictional requirement persona name; required for requirement-user.
    #[arg(long)]
    display_name: Option<String>,
    /// Requirement user age.
    #[arg(long)]
    age: Option<String>,
    /// Requirement user occupation or role.
    #[arg(long, visible_alias = "role-name")]
    occupation: Option<String>,
    /// Requirement user priority: core, primary, secondary, or peripheral.
    #[arg(long)]
    priority: Option<String>,
    /// Requirement user situation; repeatable.
    #[arg(long = "situation")]
    situations: Vec<String>,
    /// Requirement user hobby; repeatable.
    #[arg(long = "hobby")]
    hobbies: Vec<String>,
    /// Requirement user need; repeatable.
    #[arg(long = "need")]
    needs: Vec<String>,
    /// Requirement story qname for the user's stories; repeatable.
    #[arg(long = "story")]
    stories: Vec<String>,
    /// Set one scalar field: NAME=VALUE. Repeatable.
    #[arg(long = "field", value_name = "NAME=VALUE")]
    fields: Vec<String>,
    /// Replace one reference field: FIELD=TARGET[,TARGET...]. Repeatable.
    #[arg(long = "ref", value_name = "FIELD=TARGET")]
    refs: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show built-in documentation for objects, fields and workflows.
    Help {
        /// Topic: objects, fields, technical, requirement, concept, document,
        /// staging, source, remote, examples.
        topic: Option<String>,
    },
    /// Create a new Arche repository.
    Init {
        /// Directory to initialize. Defaults to the current directory.
        #[arg(value_name = "DIR")]
        dir: Option<PathBuf>,
        /// Source Git repository URL.
        #[arg(long)]
        source: Option<String>,
    },
    /// Clone a local Arche repository.
    Clone {
        /// Local remote repository path.
        remote: String,
        /// Destination directory.
        dir: Option<PathBuf>,
        /// Branch to check out.
        #[arg(long)]
        branch: Option<String>,
        /// Override the source Git URL inherited from the remote repository.
        #[arg(long)]
        source: Option<String>,
    },
    /// Serve repositories over HTTP or stdio.
    Serve {
        /// Directory containing Arche repositories.
        #[arg(long)]
        root: PathBuf,
        /// HTTP listen address.
        #[arg(long, default_value = "127.0.0.1:7700")]
        listen: String,
        /// Optional bearer token.
        #[arg(long)]
        token: Option<String>,
        /// Token allowed to move protected branches.
        #[arg(long)]
        admin_token: Option<String>,
        /// Protected branch name; repeat for multiple branches.
        #[arg(long = "protect", default_value = "main")]
        protect: Vec<String>,
        /// Serve one repository over stdin/stdout for SSH.
        #[arg(long)]
        stdio: bool,
    },
    /// Remote operations.
    Remote {
        #[command(subcommand)]
        command: RemoteCommand,
    },
    /// Branch operations.
    Branch {
        #[command(subcommand)]
        command: BranchCommand,
    },
    /// Switch branches.
    Checkout {
        /// Branch name.
        branch: String,
        /// Create the branch at HEAD before switching.
        #[arg(short = 'b')]
        create: bool,
    },
    /// Push to the configured remote.
    Push {
        /// Allow non-fast-forward ref updates.
        #[arg(long)]
        force: bool,
    },
    /// Fetch from the configured remote.
    Fetch,
    /// Fast-forward the local branch from the configured remote.
    Pull,
    /// Source Git worktree operations.
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    /// Create a design object.
    #[command(
        after_help = "Kinds: technical-system, technical-module, technical-layer, \
technical-namespace, technical-class, technical-struct, technical-enum, technical-interface, \
technical-mixin, technical-function, technical-method, technical-part, requirement-user, \
requirement-story, requirement-system, requirement-module, requirement-function, \
requirement-interaction. Run `arche help technical` or `arche help requirement` for qnames \
and fields."
    )]
    Create {
        /// Object kind, for example technical-system or requirement-user.
        kind: String,
        /// Local object name / requirement role id. Not used for technical-method.
        name: Option<String>,
        #[command(flatten)]
        fields: ObjectFieldArgs,
    },
    /// Update fields of an existing design object.
    #[command(
        after_help = "Accepts the same field flags as `arche create`. Scalars are \
replaced; --situation/--hobby/--need add list items; --story replaces the stories list. \
Use --field NAME=VALUE for any scalar and --ref FIELD=TARGET[,TARGET...] for any reference \
list. Parent and prototype are part of object identity and cannot be updated."
    )]
    Update {
        /// Existing object qname.
        qname: String,
        #[command(flatten)]
        fields: ObjectFieldArgs,
    },
    /// Structure membership operations.
    Structure {
        #[command(subcommand)]
        command: StructureCommand,
    },
    /// Document content operations.
    #[command(
        after_help = "Fields are DocumentText, DocumentList, DocumentParagraphList or \
DocumentMermaid depending on the object. Run `arche help document` for field rules and import \
JSON format."
    )]
    Doc {
        #[command(subcommand)]
        command: DocCommand,
    },
    /// Comment operations.
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
    /// Reference field operations (legacy; prefer `arche update --ref`).
    #[command(hide = true)]
    Ref {
        #[command(subcommand)]
        command: RefCommand,
    },
    /// Scalar field operations (legacy; prefer `arche update --field`).
    #[command(hide = true)]
    Field {
        #[command(subcommand)]
        command: FieldCommand,
    },
    /// Concept operations.
    Concept {
        #[command(subcommand)]
        command: ConceptCommand,
    },
    /// Requirement operations.
    Requirement {
        #[command(subcommand)]
        command: RequirementCommand,
    },
    /// Show one object by qname.
    Show {
        /// Qualified name, for example technical/app.
        qname: String,
        /// Output format: text, json, or md.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List all object qnames.
    Ls,
    /// Search objects by qname, name, kind, or design presence.
    Search {
        /// Text matched against qname and name.
        query: Option<String>,
        /// Object kinds, for example technical-system or requirement-function.
        #[arg(long)]
        kind: Vec<String>,
        /// Only objects with document fields.
        #[arg(long = "has-design")]
        has_design: bool,
    },
    /// Validate the worktree object graph.
    Check,
    /// Stage worktree objects.
    Add {
        /// Stage every worktree change.
        #[arg(short = 'A', long = "all")]
        all: bool,
        /// Stage objects for one source file path.
        #[arg(long)]
        file: Option<String>,
        /// Stage objects for one source directory path.
        #[arg(long)]
        directory: Option<String>,
        /// Object qnames to stage.
        qnames: Vec<String>,
    },
    /// Restore index entries from HEAD.
    Unstage {
        /// Restore objects for one source file path.
        #[arg(long)]
        file: Option<String>,
        /// Restore objects for one source directory path.
        #[arg(long)]
        directory: Option<String>,
        /// Object qnames to unstage.
        qnames: Vec<String>,
    },
    /// Show repository status.
    Status {
        /// Print only unstaged changes, one per line.
        #[arg(long)]
        porcelain: bool,
        /// Include kind and source in porcelain output.
        #[arg(long)]
        detail: bool,
    },
    /// Compare snapshots.
    Diff {
        /// Compare index against HEAD instead of worktree against index.
        #[arg(long)]
        staged: bool,
    },
    /// Commit the index.
    Commit {
        #[arg(short, long)]
        message: String,
        #[arg(long)]
        allow_empty: bool,
        /// Commit author recorded in the commit record.
        #[arg(long, default_value = "arche")]
        author: String,
    },
    /// Reset index and optionally worktree to HEAD.
    Reset {
        #[arg(long)]
        hard: bool,
        /// Only reset these qnames in the index.
        qnames: Vec<String>,
    },
    /// Apply a batch JSON plan.
    Batch {
        /// JSON plan file, or "-" for stdin.
        file: String,
    },
    /// Remove design objects from the worktree.
    Rm {
        /// Object qnames to remove.
        qnames: Vec<String>,
        /// Remove objects even when still referenced.
        #[arg(long)]
        force: bool,
        /// Also remove all descendant objects.
        #[arg(long)]
        recursive: bool,
    },
    /// Remove unreachable loose objects.
    Gc,
    /// Pack all loose records into one pack file.
    Repack,
    /// Show commit history.
    Log {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
enum DocCommand {
    /// Work with text fields.
    Text {
        #[command(subcommand)]
        command: DocTextCommand,
    },
    /// Work with list fields.
    List {
        #[command(subcommand)]
        command: DocListCommand,
    },
    /// Work with paragraph-list fields.
    Topic {
        #[command(subcommand)]
        command: DocTopicCommand,
    },
    /// Set a Mermaid field.
    Mermaid {
        #[command(subcommand)]
        command: DocMermaidCommand,
    },
    /// Add HTML content.
    Html {
        #[command(subcommand)]
        command: DocHtmlCommand,
    },
    /// Add a code block.
    Code {
        #[command(subcommand)]
        command: DocCodeCommand,
    },
    /// Search document text.
    Grep {
        /// Text to search for.
        text: String,
    },
    /// Check document text rules.
    Lint,
    /// Print the projected field order of one object.
    Layout {
        /// Object qname.
        qname: String,
    },
    /// Import document fields from a JSON projection.
    Import {
        /// Object qname.
        qname: String,
        /// JSON file, or "-" for stdin.
        file: String,
    },
    /// Validate that design objects carry document fields.
    Check,
}

#[derive(Debug, Subcommand)]
enum StructureCommand {
    /// Assign a top-level element to a system, module, or layer.
    Assign {
        /// Element qname.
        qname: String,
        /// Owning system, module, or layer qname.
        #[arg(long)]
        parent: String,
    },
    /// List systems, modules, layers and their members.
    List,
    /// List top-level elements without a system or module.
    Unassigned,
    /// Validate membership.
    Check,
}

#[derive(Debug, Subcommand)]
enum DocTextCommand {
    /// Set a text field on an object.
    Set {
        /// Object qname.
        qname: String,
        /// Field name, currently position.
        field: String,
        /// Text content.
        text: String,
    },
    /// Alias for set.
    Add {
        /// Object qname.
        qname: String,
        /// Field name, currently position.
        field: String,
        /// Text content.
        text: String,
    },
}

#[derive(Debug, Subcommand)]
enum DocListCommand {
    /// Create or ensure a list field.
    Add {
        /// Object qname.
        owner: String,
        /// Field name.
        field: String,
    },
    /// Append a text item to a list field.
    Item {
        #[command(subcommand)]
        command: DocListItemCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DocListItemCommand {
    Add {
        /// Object qname.
        owner: String,
        /// Field name.
        field: String,
        /// Item text.
        text: String,
    },
}

#[derive(Debug, Subcommand)]
enum DocTopicCommand {
    /// Append a topic to a paragraph-list field.
    Add {
        /// Object qname.
        owner: String,
        /// Field name.
        field: String,
        /// Topic title.
        title: String,
    },
}

#[derive(Debug, Subcommand)]
enum DocMermaidCommand {
    /// Set a Mermaid field.
    Set {
        /// Object qname.
        owner: String,
        /// Field name.
        field: String,
        /// Mermaid source.
        source: String,
    },
}

#[derive(Debug, Subcommand)]
enum DocHtmlCommand {
    /// Append an HTML preview to a paragraph-list field.
    Add {
        /// Object qname.
        owner: String,
        /// Field name.
        field: String,
        /// HTML/SVG source.
        source: String,
    },
}

#[derive(Debug, Subcommand)]
enum DocCodeCommand {
    /// Append a code block to a paragraph-list field.
    Add {
        /// Object qname.
        owner: String,
        /// Field name.
        field: String,
        /// Code language.
        #[arg(long)]
        language: String,
        /// Code source.
        source: String,
    },
}

#[derive(Debug, Subcommand)]
enum CommentCommand {
    /// Add a comment to an object.
    Add {
        /// Target object qname.
        target: String,
        /// Comment text.
        text: String,
    },
    /// List comments.
    List {
        /// Target qname. With --subtree, include every qname below it.
        target: Option<String>,
        /// Include the target's descendants, fields and anchored positions.
        #[arg(long)]
        subtree: bool,
        /// Only open comments.
        #[arg(long)]
        open: bool,
    },
    /// Resolve a comment.
    Resolve {
        /// Comment id.
        id: u64,
        /// Conclusion written when resolving.
        #[arg(long)]
        conclusion: String,
        /// Agent that wrote the conclusion.
        #[arg(long)]
        agent: String,
    },
    /// Clear comments for one target, or all comments when no target is given.
    Clear {
        /// Target object qname.
        target: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum FieldCommand {
    /// Set one scalar field: String, bool or integer.
    Set {
        /// Object qname.
        qname: String,
        /// Field name, for example display_name, age or priority.
        field: String,
        /// Field value.
        value: String,
    },
}

#[derive(Debug, Subcommand)]
enum RefCommand {
    /// Replace a reference field with one or more qnames.
    Set {
        /// Object qname.
        qname: String,
        /// Reference field, for example users, protagonist or functions.
        field: String,
        /// Target qnames. Omit to clear the field.
        targets: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ConceptCommand {
    /// Create a concept directory.
    Mkdir {
        /// Directory qname under concept/domain, concept/requirement, or concept/technical.
        qname: String,
    },
    /// Create a concept.
    Create {
        /// Concept qname.
        qname: String,
        /// Display name. Defaults to the last path segment.
        #[arg(long)]
        name: Option<String>,
        /// Concept explanation text.
        #[arg(long)]
        explanation: String,
    },
    /// List concepts.
    List,
}

#[derive(Debug, Subcommand)]
enum RequirementCommand {
    /// Legacy requirement creation; use `arche create requirement-*`.
    #[command(hide = true)]
    Create {
        #[command(subcommand)]
        command: RequirementCreateCommand,
    },
    /// List requirement objects.
    List {
        /// Filter by object kind, for example requirement-function.
        #[arg(long)]
        kind: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum RequirementCreateCommand {
    User {
        /// Role-representative local id, for example project-sponsor.
        name: String,
        /// The fictional persona's name for display.
        #[arg(long)]
        display_name: String,
    },
    Story {
        name: String,
    },
    System {
        name: String,
    },
    Module {
        #[arg(long)]
        parent: String,
        name: String,
    },
    Function {
        #[arg(long)]
        parent: String,
        name: String,
    },
    Interaction {
        #[arg(long)]
        parent: String,
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum BranchCommand {
    /// List branches.
    List,
    /// Create a branch at HEAD.
    Create { name: String },
    /// Delete a branch.
    Delete { name: String },
}

#[derive(Debug, Subcommand)]
enum RemoteCommand {
    /// Set the local remote path.
    Add { path: String },
    /// Show the configured remote.
    Show,
}

#[derive(Debug, Subcommand)]
enum SourceCommand {
    /// Clone or fetch the source Git repository and ensure the worktree.
    Fetch,
    /// Check out a source branch or commit.
    Checkout {
        /// Git ref, branch, tag, or commit.
        git_ref: String,
    },
    /// Ensure the workspace-root source worktree exists.
    Worktree,
    /// Show source worktree status.
    Status,
    /// List tracked source files.
    Ls,
    /// Print one tracked source file.
    Cat {
        /// Source path.
        path: String,
    },
    /// Align TechnicalFile objects with tracked source files.
    Sync,
    /// Show source changes since the design baseline.
    Changes {
        /// Base Git ref; defaults to the design baseline commit.
        #[arg(long)]
        base: Option<String>,
    },
    /// Record the read-only source Git URL.
    SetUrl {
        /// Source Git URL.
        url: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Help { topic } => {
            help::print(topic.as_deref())?;
        }
        Command::Init { dir, source } => {
            let repo = Repository::init_with_source(
                dir.unwrap_or_else(|| PathBuf::from(".")),
                source.as_deref(),
            )?;
            println!("Initialized Arche repository at {}", repo.dir().display());
        }
        Command::Clone {
            remote,
            dir,
            branch,
            source,
        } => {
            let repo = arche_client::clone_from(
                &remote,
                dir.unwrap_or_else(|| PathBuf::from(".")),
                branch.as_deref(),
            )?;
            if let Some(source) = source {
                repo.set_source_url(&source)?;
            }
            println!("Cloned Arche repository into {}", repo.root().display());
        }
        Command::Serve {
            root,
            listen,
            token,
            admin_token,
            protect,
            stdio,
        } => {
            if stdio {
                serve_stdio(&root)?;
            } else {
                serve(
                    &ServerOptions::new(root)
                        .listen(listen)
                        .token(token)
                        .admin_token(admin_token)
                        .protected(protect),
                )?;
            }
        }
        Command::Remote { command } => {
            let repo = Repository::open(".")?;
            match command {
                RemoteCommand::Add { path } => repo.set_remote(&path)?,
                RemoteCommand::Show => {
                    println!("{}", repo.remote_url().unwrap_or("-"));
                }
            }
        }
        Command::Branch { command } => {
            let repo = Repository::open(".")?;
            match command {
                BranchCommand::List => {
                    let current = repo.current_branch()?;
                    for branch in repo.branch_list()? {
                        if branch == current {
                            println!("* {branch}");
                        } else {
                            println!("  {branch}");
                        }
                    }
                }
                BranchCommand::Create { name } => repo.branch_create(&name)?,
                BranchCommand::Delete { name } => repo.branch_delete(&name)?,
            }
        }
        Command::Checkout { branch, create } => {
            Repository::open(".")?.checkout(&branch, create)?;
        }
        Command::Push { force } => {
            arche_client::push(&Repository::open(".")?, force)?;
        }
        Command::Fetch => {
            arche_client::fetch(&Repository::open(".")?)?;
        }
        Command::Pull => {
            arche_client::pull(&Repository::open(".")?)?;
        }
        Command::Source { command } => {
            let repo = Repository::open(".")?;
            match command {
                SourceCommand::Fetch => {
                    repo.source_fetch()?;
                    println!("{}", repo.root().display());
                }
                SourceCommand::Checkout { git_ref } => {
                    repo.source_checkout(&git_ref)?;
                }
                SourceCommand::Worktree => {
                    println!("{}", repo.source_worktree()?.display());
                }
                SourceCommand::Status => {
                    print!("{}", repo.source_status()?);
                }
                SourceCommand::Ls => {
                    for path in repo.source_ls()? {
                        println!("{path}");
                    }
                }
                SourceCommand::Cat { path } => {
                    print!("{}", repo.source_cat(&path)?);
                }
                SourceCommand::Sync => {
                    let count = repo.source_sync()?;
                    println!("synced {count} source files");
                }
                SourceCommand::Changes { base } => {
                    print!("{}", repo.source_changes(base.as_deref())?);
                }
                SourceCommand::SetUrl { url } => {
                    repo.set_source_url(&url)?;
                }
            }
        }
        Command::Create { kind, name, fields } => {
            let repo = Repository::open(".")?;
            let qname = if kind == "requirement-user" {
                let name = name
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("requirement-user requires <role-id>"))?;
                let display_name = fields.display_name.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("requirement-user requires --display-name <假想角色姓名>")
                })?;
                let (qname, _) = repo.create_requirement_user(name, display_name)?;
                let qname_text = repo.resolve(qname)?;
                apply_user_fields(&repo, &qname_text, &fields)?;
                qname
            } else {
                create_object(
                    &repo,
                    &kind,
                    name.as_deref(),
                    fields.parent.as_deref(),
                    fields.namespace.as_deref(),
                    fields.prototype.as_deref(),
                    fields.source.as_deref(),
                    fields.file.as_deref(),
                    fields.module.as_deref(),
                )?
                .0
            };
            let qname_text = repo.resolve(qname)?;
            apply_generic_assignments(&repo, &qname_text, &fields)?;
            let hash = repo.find_by_qname(&qname_text)?;
            println!("{qname_text}");
            println!("object {}", hash.to_hex());
        }
        Command::Update { qname, fields } => {
            let repo = Repository::open(".")?;
            update_object(&repo, &qname, &fields)?;
            let (hash, _) = repo.show_qname(&qname)?;
            println!("{qname}");
            println!("object {}", hash.to_hex());
        }
        Command::Structure { command } => {
            let repo = Repository::open(".")?;
            match command {
                StructureCommand::Assign { qname, parent } => {
                    repo.assign_member(&qname, &parent)?;
                }
                StructureCommand::List => {
                    for (owner, members) in repo.structure_list()? {
                        println!("{owner}");
                        for member in members {
                            println!("\t{member}");
                        }
                    }
                }
                StructureCommand::Unassigned => {
                    for qname in repo.unassigned_members()? {
                        println!("{qname}");
                    }
                }
                StructureCommand::Check => {
                    let errors = repo.membership_check()?;
                    if errors.is_empty() {
                        println!("ok");
                    } else {
                        for error in errors {
                            println!("{error}");
                        }
                        bail!("structure check failed");
                    }
                }
            }
        }
        Command::Doc { command } => {
            let repo = Repository::open(".")?;
            match command {
                DocCommand::Text { command } => match command {
                    DocTextCommand::Set { qname, field, text }
                    | DocTextCommand::Add { qname, field, text } => {
                        let (field_qname, hash) = repo.set_document_text(&qname, &field, &text)?;
                        println!("{}", repo.resolve(field_qname)?);
                        println!("object {}", hash.to_hex());
                    }
                },
                DocCommand::List { command } => match command {
                    DocListCommand::Add { owner, field } => {
                        let (field_qname, hash) = repo.doc_list_add(&owner, &field)?;
                        println!("{}", repo.resolve(field_qname)?);
                        println!("object {}", hash.to_hex());
                    }
                    DocListCommand::Item {
                        command: DocListItemCommand::Add { owner, field, text },
                    } => {
                        let (item_qname, hash) = repo.doc_list_item_add(&owner, &field, &text)?;
                        println!("{}", repo.resolve(item_qname)?);
                        println!("object {}", hash.to_hex());
                    }
                },
                DocCommand::Topic { command } => match command {
                    DocTopicCommand::Add {
                        owner,
                        field,
                        title,
                    } => {
                        let (topic_qname, hash) = repo.doc_topic_add(&owner, &field, &title)?;
                        println!("{}", repo.resolve(topic_qname)?);
                        println!("object {}", hash.to_hex());
                    }
                },
                DocCommand::Mermaid { command } => match command {
                    DocMermaidCommand::Set {
                        owner,
                        field,
                        source,
                    } => {
                        let (field_qname, hash) = repo.doc_mermaid_set(&owner, &field, &source)?;
                        println!("{}", repo.resolve(field_qname)?);
                        println!("object {}", hash.to_hex());
                    }
                },
                DocCommand::Html { command } => match command {
                    DocHtmlCommand::Add {
                        owner,
                        field,
                        source,
                    } => {
                        let (html_qname, hash) = repo.doc_html_add(&owner, &field, &source)?;
                        println!("{}", repo.resolve(html_qname)?);
                        println!("object {}", hash.to_hex());
                    }
                },
                DocCommand::Code { command } => match command {
                    DocCodeCommand::Add {
                        owner,
                        field,
                        language,
                        source,
                    } => {
                        let (code_qname, hash) =
                            repo.doc_code_add(&owner, &field, &language, &source)?;
                        println!("{}", repo.resolve(code_qname)?);
                        println!("object {}", hash.to_hex());
                    }
                },
                DocCommand::Grep { text } => {
                    doc_grep(&repo, &text)?;
                }
                DocCommand::Lint => {
                    let errors = repo.doc_lint()?;
                    if errors.is_empty() {
                        println!("ok");
                    } else {
                        for error in errors {
                            println!("{error}");
                        }
                        bail!("doc lint failed");
                    }
                }
                DocCommand::Layout { qname } => {
                    doc_layout(&repo, &qname)?;
                }
                DocCommand::Import { qname, file } => {
                    let text = if file == "-" {
                        let mut text = String::new();
                        std::io::stdin().read_to_string(&mut text)?;
                        text
                    } else {
                        std::fs::read_to_string(&file)?
                    };
                    let count = repo.import_doc_json(&qname, &text)?;
                    println!("imported {count} document fields");
                }
                DocCommand::Check => {
                    let errors = repo.doc_check()?;
                    if errors.is_empty() {
                        println!("ok");
                    } else {
                        for error in errors {
                            println!("{error}");
                        }
                        bail!("doc check failed");
                    }
                }
            }
        }
        Command::Field { command } => {
            let repo = Repository::open(".")?;
            match command {
                FieldCommand::Set {
                    qname,
                    field,
                    value,
                } => {
                    let value = normalize_scalar_value(&field, &value)?;
                    repo.set_scalar(&qname, &field, &value)?;
                    println!("updated {qname}.{field}");
                }
            }
        }
        Command::Ref { command } => {
            let repo = Repository::open(".")?;
            match command {
                RefCommand::Set {
                    qname,
                    field,
                    targets,
                } => {
                    repo.set_refs(&qname, &field, &targets)?;
                    println!("updated {qname}.{field}");
                }
            }
        }
        Command::Comment { command } => {
            let repo = Repository::open(".")?;
            match command {
                CommentCommand::Add { target, text } => {
                    let comment = repo.comment_add(&target, &text)?;
                    println!("comment {}", comment.id);
                }
                CommentCommand::List {
                    target,
                    subtree,
                    open,
                } => {
                    let comments = match (subtree, target.as_deref()) {
                        (true, Some(target)) => repo.comment_list_subtree(target, open)?,
                        (true, None) => repo.comment_list(None, open)?,
                        (false, target) => repo.comment_list(target, open)?,
                    };
                    for comment in comments {
                        let state = if comment.resolved { "resolved" } else { "open" };
                        println!(
                            "{}\t{}\t{}\t{}",
                            comment.id, state, comment.target, comment.body
                        );
                        if !comment.conclusion.is_empty() {
                            println!(
                                "  conclusion (by {}): {}",
                                comment.resolved_by, comment.conclusion
                            );
                        }
                    }
                }
                CommentCommand::Resolve {
                    id,
                    conclusion,
                    agent,
                } => {
                    repo.comment_resolve(id, &conclusion, &agent)?;
                }
                CommentCommand::Clear { target } => {
                    let count = repo.comment_clear(target.as_deref())?;
                    println!("cleared {count} comment(s)");
                }
            }
        }
        Command::Concept { command } => {
            let repo = Repository::open(".")?;
            match command {
                ConceptCommand::Mkdir { qname } => {
                    let (qname, hash) = repo.concept_mkdir(&qname)?;
                    println!("{}", repo.resolve(qname)?);
                    println!("object {}", hash.to_hex());
                }
                ConceptCommand::Create {
                    qname,
                    name,
                    explanation,
                } => {
                    let name = name.unwrap_or_else(|| {
                        qname
                            .rsplit_once('/')
                            .map(|(_, name)| name.to_owned())
                            .unwrap_or_else(|| qname.clone())
                    });
                    let (qname, hash) = repo.concept_create(&qname, &name, &explanation)?;
                    println!("{}", repo.resolve(qname)?);
                    println!("object {}", hash.to_hex());
                }
                ConceptCommand::List => {
                    for (qname, concept) in repo.concept_list()? {
                        println!("{}\t{}", repo.resolve(qname)?, concept.display_name());
                    }
                }
            }
        }
        Command::Requirement { command } => {
            let repo = Repository::open(".")?;
            match command {
                RequirementCommand::Create { command } => {
                    let (qname, hash) = match command {
                        RequirementCreateCommand::User { name, display_name } => {
                            repo.create_requirement_user(&name, &display_name)?
                        }
                        RequirementCreateCommand::Story { name } => {
                            repo.create_requirement_story(&name)?
                        }
                        RequirementCreateCommand::System { name } => {
                            repo.create_requirement_system(&name)?
                        }
                        RequirementCreateCommand::Module { parent, name } => {
                            repo.create_requirement_module(&parent, &name)?
                        }
                        RequirementCreateCommand::Function { parent, name } => {
                            repo.create_requirement_function(&parent, &name)?
                        }
                        RequirementCreateCommand::Interaction { parent, name } => {
                            repo.create_requirement_interaction(&parent, &name)?
                        }
                    };
                    println!("{}", repo.resolve(qname)?);
                    println!("object {}", hash.to_hex());
                }
                RequirementCommand::List { kind } => {
                    for (_, object) in repo.list_objects()? {
                        let object_kind = object.kind().name();
                        if !object_kind.starts_with("Requirement") {
                            continue;
                        }
                        if let Some(kind) = &kind {
                            let expected = match kind.as_str() {
                                "user" => "RequirementUser",
                                "story" => "RequirementStory",
                                "system" => "RequirementSystem",
                                "module" => "RequirementModule",
                                "function" => "RequirementFunction",
                                "interaction" => "RequirementInteraction",
                                other => bail!("unknown requirement kind {other}"),
                            };
                            if object_kind != expected {
                                continue;
                            }
                        }
                        println!("{}\t{}", object_kind, repo.resolve(object.qname())?);
                    }
                }
            }
        }
        Command::Show { qname, format } => {
            let repo = Repository::open(".")?;
            let (hash, object) = repo.show_qname(&qname)?;
            match format.as_str() {
                "text" => show_object(&repo, hash, &object)?,
                "json" => show_json(&repo, hash, &object)?,
                "md" => show_markdown(&repo, hash, &object)?,
                other => bail!("unknown show format {other}"),
            }
        }
        Command::Ls => {
            let repo = Repository::open(".")?;
            let mut rows = Vec::new();
            for (_, object) in repo.list_objects()? {
                let qname = repo.resolve(object.qname())?;
                rows.push((qname, object.kind().name()));
            }
            rows.sort();
            for (qname, kind) in rows {
                println!("{kind}\t{qname}");
            }
        }
        Command::Search {
            query,
            kind,
            has_design,
        } => {
            let repo = Repository::open(".")?;
            for (_, object) in repo.search(query.as_deref(), &kind, has_design)? {
                println!(
                    "{}\t{}",
                    object.kind().name(),
                    repo.resolve(object.qname())?
                );
            }
        }
        Command::Check => {
            let repo = Repository::open(".")?;
            let errors = repo.check()?;
            if errors.is_empty() {
                println!("ok");
            } else {
                for error in errors {
                    println!("{error}");
                }
                bail!("check failed");
            }
        }
        Command::Add {
            all,
            file,
            directory,
            qnames,
        } => {
            let repo = Repository::open(".")?;
            if let Some(file) = file {
                repo.add_file(&file)?;
            } else if let Some(directory) = directory {
                repo.add_directory(&directory)?;
            } else if all || qnames.is_empty() {
                repo.add_all()?;
            } else {
                repo.add(&qnames)?;
            }
        }
        Command::Unstage {
            file,
            directory,
            qnames,
        } => {
            let repo = Repository::open(".")?;
            if let Some(file) = file {
                repo.unstage_file(&file)?;
            } else if let Some(directory) = directory {
                repo.unstage_directory(&directory)?;
            } else {
                repo.unstage(&qnames)?;
            }
        }
        Command::Status { porcelain, detail } => {
            let repo = Repository::open(".")?;
            status(&repo, porcelain, detail)?;
        }
        Command::Diff { staged } => {
            let repo = Repository::open(".")?;
            diff(&repo, staged)?;
        }
        Command::Commit {
            message,
            allow_empty,
            author,
        } => {
            let repo = Repository::open(".")?;
            let hash = repo.commit_index_as(&message, allow_empty, &author)?;
            println!("{}", hash.to_hex());
        }
        Command::Reset { hard, qnames } => {
            let repo = Repository::open(".")?;
            if qnames.is_empty() {
                repo.reset(hard)?;
            } else {
                repo.reset_paths(&qnames)?;
            }
        }
        Command::Batch { file } => {
            let repo = Repository::open(".")?;
            batch(&repo, &file)?;
        }
        Command::Rm {
            qnames,
            force,
            recursive,
        } => {
            let repo = Repository::open(".")?;
            repo.remove_objects(&qnames, force, recursive)?;
        }
        Command::Gc => {
            let repo = Repository::open(".")?;
            let removed = repo.gc()?;
            println!("removed {removed} unreachable objects");
        }
        Command::Repack => {
            let repo = Repository::open(".")?;
            let count = repo.repack()?;
            println!("packed {count} records");
        }
        Command::Log { limit } => {
            let repo = Repository::open(".")?;
            for (hash, commit) in repo.log()?.into_iter().take(limit) {
                println!("commit {}", hash.to_hex());
                println!("Author: {}", commit.author);
                println!("Date:   {}", commit.timestamp);
                println!();
                for line in commit.message.lines() {
                    println!("    {line}");
                }
                println!();
            }
        }
    }
    Ok(())
}

fn show_object(repo: &Repository, hash: arche_core::ObjectHash, object: &Object) -> Result<()> {
    println!("qname: {}", repo.resolve(object.qname())?);
    let name = object.name();
    if !name.is_zero() {
        println!("name:  {}", repo.resolve(name)?);
    }
    println!("kind:  {}", object.kind().name());
    println!("hash:  {}", hash.to_hex());

    match object {
        Object::TechnicalRoot(value) => {
            println!("file: {}", optional_symbol(repo, value.file())?);
            println!("directory: {}", optional_symbol(repo, value.directory())?);
            println!("namespace: {}", optional_symbol(repo, value.namespace())?);
            println!("system: {}", optional_symbol(repo, value.system())?);
        }
        Object::RequirementRoot(value) => {
            println!("user: {}", optional_symbol(repo, value.user())?);
            println!("story: {}", optional_symbol(repo, value.story())?);
            println!("system: {}", optional_symbol(repo, value.system())?);
        }
        Object::TechnicalSystem(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("members: {}", optional_symbol(repo, value.members())?);
            println!("modules: {}", optional_symbol(repo, value.modules())?);
            println!("layers: {}", optional_symbol(repo, value.layers())?);
            println!("constants: {}", optional_symbol(repo, value.constants())?);
            println!("variables: {}", optional_symbol(repo, value.variables())?);
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("responsibilities", value.responsibilities()),
                    ("non_responsibilities", value.non_responsibilities()),
                    ("features", value.features()),
                    ("non_features", value.non_features()),
                    ("invariants", value.invariants()),
                    ("constraints", value.constraints()),
                    ("design_principles", value.design_principles()),
                    ("external_dependencies", value.external_dependencies()),
                    ("usage", value.usage()),
                    ("design_points", value.design_points()),
                ],
            )?;
        }
        Object::TechnicalModule(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("members: {}", optional_symbol(repo, value.members())?);
            print_symbol_list(repo, "dependencies", &value.dependencies())?;
            println!("constants: {}", optional_symbol(repo, value.constants())?);
            println!("variables: {}", optional_symbol(repo, value.variables())?);
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("responsibilities", value.responsibilities()),
                    ("non_responsibilities", value.non_responsibilities()),
                    ("features", value.features()),
                    ("non_features", value.non_features()),
                    ("invariants", value.invariants()),
                    ("constraints", value.constraints()),
                    ("design_principles", value.design_principles()),
                    ("usage", value.usage()),
                    ("design_points", value.design_points()),
                ],
            )?;
        }
        Object::TechnicalDirectory(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("children: {}", optional_symbol(repo, value.children())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalFile(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("entities: {}", optional_symbol(repo, value.entities())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalLayer(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("members: {}", optional_symbol(repo, value.members())?);
            print_symbol_list(repo, "dependencies", &value.dependencies())?;
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalNamespace(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("children: {}", optional_symbol(repo, value.children())?);
            println!("entities: {}", optional_symbol(repo, value.entities())?);
            println!("constants: {}", optional_symbol(repo, value.constants())?);
            println!("variables: {}", optional_symbol(repo, value.variables())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalClass(value) => {
            println!("parent: {}", optional_symbol(repo, value.parent())?);
            println!("namespace: {}", optional_symbol(repo, value.namespace())?);
            println!("file: {}", optional_symbol(repo, value.file())?);
            println!("module: {}", optional_symbol(repo, value.module())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("superclass: {}", optional_symbol(repo, value.superclass())?);
            print_symbol_list(repo, "interfaces", &value.interfaces())?;
            print_symbol_list(repo, "mixins", &value.mixins())?;
            print_symbol_list(
                repo,
                "explicit_dependencies",
                &value.explicit_dependencies(),
            )?;
            print_symbol_list(
                repo,
                "implicit_dependencies",
                &value.implicit_dependencies(),
            )?;
            println!("method: {}", optional_symbol(repo, value.method())?);
            println!("types: {}", optional_symbol(repo, value.types())?);
            println!("parts: {}", optional_symbol(repo, value.parts())?);
            println!("constants: {}", optional_symbol(repo, value.constants())?);
            println!("members: {}", optional_symbol(repo, value.members())?);
            println!("properties: {}", optional_symbol(repo, value.properties())?);
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("responsibilities", value.responsibilities()),
                    ("non_responsibilities", value.non_responsibilities()),
                    ("features", value.features()),
                    ("non_features", value.non_features()),
                    ("invariants", value.invariants()),
                    ("constraints", value.constraints()),
                    ("design_principles", value.design_principles()),
                    ("usage", value.usage()),
                    ("design_points", value.design_points()),
                ],
            )?;
        }
        Object::TechnicalStruct(value) => {
            println!("parent: {}", optional_symbol(repo, value.parent())?);
            println!("namespace: {}", optional_symbol(repo, value.namespace())?);
            println!("file: {}", optional_symbol(repo, value.file())?);
            println!("module: {}", optional_symbol(repo, value.module())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            print_symbol_list(repo, "dependencies", &value.dependencies())?;
            println!("method: {}", optional_symbol(repo, value.method())?);
            println!("types: {}", optional_symbol(repo, value.types())?);
            println!("constants: {}", optional_symbol(repo, value.constants())?);
            println!("members: {}", optional_symbol(repo, value.members())?);
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("invariants", value.invariants()),
                ],
            )?;
        }
        Object::TechnicalEnum(value) => {
            println!("parent: {}", optional_symbol(repo, value.parent())?);
            println!("namespace: {}", optional_symbol(repo, value.namespace())?);
            println!("file: {}", optional_symbol(repo, value.file())?);
            println!("module: {}", optional_symbol(repo, value.module())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            print_symbol_list(repo, "dependencies", &value.dependencies())?;
            println!("values: {}", optional_symbol(repo, value.values())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalInterface(value) => {
            println!("parent: {}", optional_symbol(repo, value.parent())?);
            println!("namespace: {}", optional_symbol(repo, value.namespace())?);
            println!("file: {}", optional_symbol(repo, value.file())?);
            println!("module: {}", optional_symbol(repo, value.module())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("extends: {}", optional_symbol(repo, value.extends())?);
            print_symbol_list(repo, "dependencies", &value.dependencies())?;
            println!("methods: {}", optional_symbol(repo, value.methods())?);
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("responsibilities", value.responsibilities()),
                    ("non_responsibilities", value.non_responsibilities()),
                    ("features", value.features()),
                    ("non_features", value.non_features()),
                    ("invariants", value.invariants()),
                    ("usage", value.usage()),
                    ("design_points", value.design_points()),
                ],
            )?;
        }
        Object::TechnicalMethod(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("is_static: {}", value.is_static());
            println!("is_async: {}", value.is_async());
            println!("is_abstract: {}", value.is_abstract());
            println!("is_const: {}", value.is_const());
            println!("overrides: {}", optional_symbol(repo, value.overrides())?);
            print_symbol_list(
                repo,
                "explicit_dependencies",
                &value.explicit_dependencies(),
            )?;
            print_symbol_list(
                repo,
                "implicit_dependencies",
                &value.implicit_dependencies(),
            )?;
            println!("arguments: {}", optional_symbol(repo, value.arguments())?);
            println!(
                "return_value: {}",
                optional_symbol(repo, value.return_value())?
            );
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("usage", value.usage()),
                    ("design_points", value.design_points()),
                ],
            )?;
        }
        Object::TechnicalFunction(value) => {
            println!("namespace: {}", optional_symbol(repo, value.namespace())?);
            println!("file: {}", optional_symbol(repo, value.file())?);
            println!("module: {}", optional_symbol(repo, value.module())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("is_async: {}", value.is_async());
            print_symbol_list(
                repo,
                "explicit_dependencies",
                &value.explicit_dependencies(),
            )?;
            print_symbol_list(
                repo,
                "implicit_dependencies",
                &value.implicit_dependencies(),
            )?;
            println!("arguments: {}", optional_symbol(repo, value.arguments())?);
            println!(
                "return_value: {}",
                optional_symbol(repo, value.return_value())?
            );
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("usage", value.usage()),
                    ("design_points", value.design_points()),
                ],
            )?;
        }
        Object::TechnicalMixin(value) => {
            println!("parent: {}", optional_symbol(repo, value.parent())?);
            println!("namespace: {}", optional_symbol(repo, value.namespace())?);
            println!("file: {}", optional_symbol(repo, value.file())?);
            println!("module: {}", optional_symbol(repo, value.module())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            print_symbol_list(repo, "on", &value.on())?;
            println!("superclass: {}", optional_symbol(repo, value.superclass())?);
            print_symbol_list(repo, "interfaces", &value.interfaces())?;
            print_symbol_list(repo, "mixins", &value.mixins())?;
            print_symbol_list(
                repo,
                "explicit_dependencies",
                &value.explicit_dependencies(),
            )?;
            print_symbol_list(
                repo,
                "implicit_dependencies",
                &value.implicit_dependencies(),
            )?;
            println!("methods: {}", optional_symbol(repo, value.methods())?);
            println!("types: {}", optional_symbol(repo, value.types())?);
            print_design_fields(
                repo,
                &[
                    ("position", value.position()),
                    ("responsibilities", value.responsibilities()),
                    ("non_responsibilities", value.non_responsibilities()),
                    ("features", value.features()),
                    ("non_features", value.non_features()),
                    ("invariants", value.invariants()),
                    ("usage", value.usage()),
                    ("design_points", value.design_points()),
                ],
            )?;
        }
        Object::TechnicalPart(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            print_symbol_list(
                repo,
                "explicit_dependencies",
                &value.explicit_dependencies(),
            )?;
            print_symbol_list(
                repo,
                "implicit_dependencies",
                &value.implicit_dependencies(),
            )?;
            println!("members: {}", optional_symbol(repo, value.members())?);
            println!("methods: {}", optional_symbol(repo, value.methods())?);
            println!("types: {}", optional_symbol(repo, value.types())?);
        }
        Object::RequirementUser(value) => {
            println!("display_name: {}", value.display_name());
            println!("age: {}", value.age());
            println!("occupation: {}", value.occupation());
            print_design_fields(
                repo,
                &[
                    ("situations", value.situations()),
                    ("hobbies", value.hobbies()),
                    ("needs", value.needs()),
                ],
            )?;
            println!("priority: {}", priority_display(value.priority()));
            print_symbol_list(repo, "stories", &value.stories())?;
        }
        Object::RequirementStory(value) => {
            print_design_fields(
                repo,
                &[
                    ("preconditions", value.preconditions()),
                    ("description", value.description()),
                    ("flow", value.flow()),
                    ("requirement", value.requirement()),
                ],
            )?;
            println!("frequency: {}", value.frequency());
            println!(
                "protagonist: {}",
                optional_symbol(repo, value.protagonist())?
            );
            print_symbol_list(repo, "users", &value.users())?;
            print_symbol_list(repo, "functions", &value.functions())?;
        }
        Object::RequirementSystem(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("modules: {}", optional_symbol(repo, value.modules())?);
            print_design_fields(
                repo,
                &[
                    ("form", value.form()),
                    ("covered_users", value.covered_users()),
                    ("closed_loop_goal", value.closed_loop_goal()),
                    ("relationships", value.relationships()),
                    ("boundary", value.boundary()),
                ],
            )?;
        }
        Object::RequirementModule(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("functions: {}", optional_symbol(repo, value.functions())?);
            print_design_fields(
                repo,
                &[
                    ("core_concept", value.core_concept()),
                    ("included_concepts", value.included_concepts()),
                    ("concept_relations", value.concept_relations()),
                    ("boundary", value.boundary()),
                ],
            )?;
        }
        Object::RequirementFunction(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("flow: {}", optional_symbol(repo, value.flow())?);
            println!(
                "interactions: {}",
                optional_symbol(repo, value.interactions())?
            );
            print_design_fields(
                repo,
                &[
                    ("initiator", value.initiator()),
                    ("system_change", value.system_change()),
                    ("preconditions", value.preconditions()),
                    ("feedback", value.feedback()),
                    ("description", value.description()),
                ],
            )?;
        }
        Object::RequirementInteraction(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            print_design_fields(
                repo,
                &[("description", value.description()), ("ui", value.ui())],
            )?;
        }
        Object::ObjectList(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("items: {}", value.items().len());
            for item in &value.items() {
                println!("  - {}", repo.resolve(repo.read(*item)?.qname())?);
            }
        }
        Object::Root(value) => {
            println!("source_commit: {}", value.source_commit());
            println!("technical: {}", value.technical().get().to_hex());
            println!("requirement: {}", value.requirement().get().to_hex());
            println!("concept: {}", value.concept().get().to_hex());
        }
        Object::Concept(value) => {
            println!("display_name: {}", value.display_name());
            print_design_fields(repo, &[("explanation", value.explanation())])?;
        }
        Object::DocumentText(value) => {
            println!("text: {}", value.content());
        }
        Object::DocumentTopic(value) => {
            println!("title: {}", value.title());
            println!("body: {}", value.body().get().to_hex());
        }
        Object::DocumentList(value) => {
            println!("items: {}", value.items().len());
        }
        Object::DocumentParagraphList(value) => {
            println!("items: {}", value.items().len());
        }
        Object::TechnicalVariable(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("ty: {}", optional_symbol(repo, value.ty())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalConstant(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("ty: {}", optional_symbol(repo, value.ty())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalMember(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("ty: {}", optional_symbol(repo, value.ty())?);
            println!("is_static: {}", value.is_static());
            println!("is_const: {}", value.is_const());
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalProperty(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("ty: {}", optional_symbol(repo, value.ty())?);
            println!("getter: {}", optional_symbol(repo, value.getter())?);
            println!("setter: {}", optional_symbol(repo, value.setter())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalEnumValue(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalArgument(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("ty: {}", optional_symbol(repo, value.ty())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::TechnicalReturnValue(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("source: {}", optional_symbol(repo, value.source())?);
            println!("ty: {}", optional_symbol(repo, value.ty())?);
            print_design_fields(repo, &[("position", value.position())])?;
        }
        Object::ProjectRoot(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            println!("document: {}", optional_symbol(repo, value.document())?);
        }
        Object::ProjectDocument(value) => {
            println!("parent: {}", repo.resolve(value.parent())?);
            print_design_fields(repo, &[("document", value.document())])?;
        }
        _ => {}
    }
    Ok(())
}

fn show_json(repo: &Repository, hash: ObjectHash, object: &Object) -> Result<()> {
    let qname = repo.resolve(object.qname())?;
    print!(
        "{{\"qname\":\"{}\",\"kind\":\"{}\",\"hash\":\"{}\"",
        json_escape(&qname),
        object.kind().name(),
        hash.to_hex()
    );
    let name = object.name();
    if !name.is_zero() {
        print!(",\"name\":\"{}\"", json_escape(&repo.resolve(name)?));
    }
    let fields = arche_core::repo::doc_fields(object);
    let present = fields
        .into_iter()
        .filter(|(_, value)| !value.is_zero())
        .collect::<Vec<_>>();
    print!(",\"fields\":{{");
    for (index, (field, value)) in present.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        let field_object = repo.read(*value)?;
        print!(
            "\"{}\":{}",
            json_escape(field),
            json_block(repo, &field_object)?
        );
    }
    print!("}}");
    let attributes = repo.object_attributes(object)?;
    if !attributes.is_empty() {
        print!(",\"attributes\":{{");
        for (index, (name, value)) in attributes.iter().enumerate() {
            if index > 0 {
                print!(",");
            }
            let value = if name == "priority" {
                value
                    .parse::<u8>()
                    .ok()
                    .and_then(priority_slug)
                    .unwrap_or(value.as_str())
            } else {
                value.as_str()
            };
            print!("\"{}\":\"{}\"", json_escape(name), json_escape(value));
        }
        print!("}}");
    }
    println!("}}");
    Ok(())
}

fn json_block(repo: &Repository, object: &Object) -> Result<String> {
    Ok(match object {
        Object::DocumentText(value) => format!(
            "{{\"type\":\"DocumentText\",\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}",
            json_escape(value.content())
        ),
        Object::DocumentList(value) => {
            let items = value
                .items()
                .iter()
                .map(|hash| {
                    let item = repo.read(*hash)?;
                    match item {
                        Object::DocumentText(text) => Ok(format!(
                            "{{\"type\":\"text\",\"text\":\"{}\"}}",
                            json_escape(text.content())
                        )),
                        other => Ok(format!(
                            "{{\"type\":\"object\",\"qname\":\"{}\"}}",
                            json_escape(&repo.resolve(other.qname())?)
                        )),
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            format!(
                "{{\"type\":\"DocumentList\",\"items\":[{}]}}",
                items.join(",")
            )
        }
        Object::DocumentParagraphList(value) => {
            let items = value
                .items()
                .iter()
                .map(|hash| json_block(repo, &repo.read(*hash)?))
                .collect::<Result<Vec<_>>>()?;
            format!(
                "{{\"type\":\"DocumentParagraphList\",\"items\":[{}]}}",
                items.join(",")
            )
        }
        Object::DocumentTopic(value) => {
            let body = repo.read(value.body().get())?;
            format!(
                "{{\"type\":\"topic\",\"title\":\"{}\",\"body\":{}}}",
                json_escape(value.title()),
                json_block(repo, &body)?
            )
        }
        Object::DocumentCode(value) => format!(
            "{{\"type\":\"code\",\"language\":\"{}\",\"source\":\"{}\"}}",
            json_escape(value.language()),
            json_escape(value.source())
        ),
        Object::DocumentMermaid(value) => format!(
            "{{\"type\":\"mermaid\",\"source\":\"{}\"}}",
            json_escape(value.source())
        ),
        Object::DocumentHtml(value) => format!(
            "{{\"type\":\"html\",\"source\":\"{}\"}}",
            json_escape(value.source())
        ),
        other => format!(
            "{{\"type\":\"{}\",\"qname\":\"{}\"}}",
            other.kind().name(),
            json_escape(&repo.resolve(other.qname())?)
        ),
    })
}

fn show_markdown(repo: &Repository, _hash: ObjectHash, object: &Object) -> Result<()> {
    let qname = repo.resolve(object.qname())?;
    let name = object.name();
    let title = if name.is_zero() {
        qname.clone()
    } else {
        repo.resolve(name)?
    };
    println!("# {title}\n");
    println!("- qname: `{qname}`");
    println!("- kind: `{}`\n", object.kind().name());
    for (field, value) in arche_core::repo::doc_fields(object) {
        if value.is_zero() {
            continue;
        }
        let field_object = repo.read(value)?;
        println!("## {field}\n");
        print_markdown_block(repo, &field_object, 2)?;
    }
    Ok(())
}

fn print_markdown_block(repo: &Repository, object: &Object, level: usize) -> Result<()> {
    match object {
        Object::DocumentText(value) => {
            println!("{}\n", value.content());
        }
        Object::DocumentList(value) => {
            for hash in value.items().iter() {
                if let Object::DocumentText(text) = repo.read(*hash)? {
                    println!("- {}", text.content());
                }
            }
            println!();
        }
        Object::DocumentParagraphList(value) => {
            for hash in value.items().iter() {
                print_markdown_block(repo, &repo.read(*hash)?, level + 1)?;
            }
        }
        Object::DocumentTopic(value) => {
            println!("{} {}\n", "#".repeat(level), value.title());
            print_markdown_block(repo, &repo.read(value.body().get())?, level + 1)?;
        }
        Object::DocumentCode(value) => {
            println!("```{}\n{}\n```\n", value.language(), value.source());
        }
        Object::DocumentMermaid(value) => {
            println!("```mermaid\n{}\n```\n", value.source());
        }
        Object::DocumentHtml(value) => {
            println!("```html\n{}\n```\n", value.source());
        }
        other => {
            println!(
                "- {}: `{}`\n",
                other.kind().name(),
                repo.resolve(other.qname())?
            );
        }
    }
    Ok(())
}

fn json_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn status(repo: &Repository, porcelain: bool, detail: bool) -> Result<()> {
    let head = repo.head_snapshot()?;
    let index = repo.index_snapshot()?;
    let worktree = repo.worktree_snapshot()?;
    if porcelain {
        return status_porcelain(repo, &index, &worktree, detail);
    }
    println!("repository: {}", repo.root().display());
    println!("branch: {}", repo.current_branch()?);
    println!("HEAD: {}", repo.head_commit()?.to_hex());
    println!();
    println!("Changes to be committed:");
    print_snapshot_changes(repo, &head, &index)?;
    println!();
    println!("Changes not staged for commit:");
    print_snapshot_changes(repo, &index, &worktree)?;
    Ok(())
}

fn status_porcelain(
    repo: &Repository,
    index: &Snapshot,
    worktree: &Snapshot,
    detail: bool,
) -> Result<()> {
    let mut qnames = BTreeSet::new();
    qnames.extend(index.entries().iter().map(|(qname, _)| *qname));
    qnames.extend(worktree.entries().iter().map(|(qname, _)| *qname));
    let mut rows = Vec::new();
    for qname in qnames {
        if qname == SymbolHash::of("root") {
            continue;
        }
        let old_hash = index.get(qname);
        let new_hash = worktree.get(qname);
        let status = match (old_hash, new_hash) {
            (None, Some(_)) => 'A',
            (Some(_), None) => 'D',
            (Some(old), Some(new)) if old != new => 'M',
            _ => continue,
        };
        let hash = new_hash.or(old_hash).expect("one side exists");
        rows.push((qname, status, hash));
    }
    rows.sort_by_key(|(qname, _, _)| qname.to_hex());
    for (qname, status, hash) in rows {
        if detail {
            let object = repo.read(hash)?;
            let source = object_source_path(repo, &object)?;
            println!(
                "{status}\t{}\t{source}\t{}",
                object.kind().name(),
                repo.resolve(qname)?
            );
        } else {
            println!("{status}\t{}", repo.resolve(qname)?);
        }
    }
    Ok(())
}

fn object_source_path(repo: &Repository, object: &Object) -> Result<String> {
    let path = match object {
        Object::TechnicalDirectory(value) => value.source().path,
        Object::TechnicalFile(value) => value.source().path,
        Object::TechnicalClass(value) => value.source().path,
        Object::TechnicalStruct(value) => value.source().path,
        Object::TechnicalEnum(value) => value.source().path,
        Object::TechnicalInterface(value) => value.source().path,
        Object::TechnicalMethod(value) => value.source().path,
        Object::TechnicalFunction(value) => value.source().path,
        Object::TechnicalMixin(value) => value.source().path,
        Object::TechnicalPart(value) => value.source().path,
        Object::TechnicalVariable(value) => value.source().path,
        Object::TechnicalConstant(value) => value.source().path,
        Object::TechnicalMember(value) => value.source().path,
        Object::TechnicalEnumValue(value) => value.source().path,
        Object::TechnicalArgument(value) => value.source().path,
        Object::TechnicalReturnValue(value) => value.source().path,
        _ => return Ok(String::from("-")),
    };
    if path.is_zero() {
        Ok(String::from("-"))
    } else {
        repo.resolve(path)
    }
}

#[allow(clippy::too_many_arguments)]
fn normalize_scalar_value(field: &str, value: &str) -> Result<String> {
    if field != "priority" {
        return Ok(value.to_owned());
    }
    parse_priority(value)
        .map(|priority| priority.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "invalid priority {value:?}; use core, primary, secondary, or peripheral"
            )
        })
}

fn apply_generic_assignments(
    repo: &Repository,
    qname: &str,
    fields: &ObjectFieldArgs,
) -> Result<()> {
    for assignment in &fields.fields {
        let (field, value) = assignment
            .split_once('=')
            .with_context(|| format!("--field expects NAME=VALUE, got {assignment:?}"))?;
        let field = field.trim();
        let value = normalize_scalar_value(field, value)?;
        repo.set_scalar(qname, field, &value)?;
    }
    for assignment in &fields.refs {
        let (field, targets) = assignment.split_once('=').with_context(|| {
            format!("--ref expects FIELD=TARGET[,TARGET...], got {assignment:?}")
        })?;
        let targets = targets
            .split(',')
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        repo.set_refs(qname, field.trim(), &targets)?;
    }
    Ok(())
}

fn apply_user_fields(repo: &Repository, qname: &str, fields: &ObjectFieldArgs) -> Result<()> {
    for (field, value) in [
        ("display_name", fields.display_name.as_deref()),
        ("age", fields.age.as_deref()),
        ("occupation", fields.occupation.as_deref()),
    ] {
        if let Some(value) = value {
            repo.set_scalar(qname, field, value)?;
        }
    }
    if let Some(priority) = fields.priority.as_deref() {
        let value = normalize_scalar_value("priority", priority)?;
        repo.set_scalar(qname, "priority", &value)?;
    }
    for (field, values) in [
        ("situations", &fields.situations),
        ("hobbies", &fields.hobbies),
        ("needs", &fields.needs),
    ] {
        for value in values {
            repo.doc_list_item_add(qname, field, value)?;
        }
    }
    if !fields.stories.is_empty() {
        repo.set_refs(qname, "stories", &fields.stories)?;
    }
    Ok(())
}

fn update_object(repo: &Repository, qname: &str, fields: &ObjectFieldArgs) -> Result<()> {
    if fields.parent.is_some() {
        bail!(
            "cannot update --parent; the parent is part of the object qname. Recreate the object under the new parent instead"
        );
    }
    if fields.prototype.is_some() {
        bail!(
            "cannot update --prototype; the prototype is part of the technical-method identity. Recreate the method instead"
        );
    }

    let (_, object) = repo.show_qname(qname)?;
    let is_user = matches!(object, Object::RequirementUser(_));
    let user_only = fields.age.is_some()
        || fields.occupation.is_some()
        || fields.priority.is_some()
        || !fields.situations.is_empty()
        || !fields.hobbies.is_empty()
        || !fields.needs.is_empty()
        || !fields.stories.is_empty();
    if user_only && !is_user {
        bail!(
            "--age/--occupation/--priority/--situation/--hobby/--need/--story only apply to requirement-user"
        );
    }

    let mut changed = false;
    if is_user {
        apply_user_fields(repo, qname, fields)?;
        changed |= fields.display_name.is_some() || user_only;
    } else if let Some(display_name) = fields.display_name.as_deref() {
        repo.set_scalar(qname, "display_name", display_name)?;
        changed = true;
    }
    for (field, value) in [
        ("namespace", fields.namespace.as_deref()),
        ("file", fields.file.as_deref()),
        ("module", fields.module.as_deref()),
    ] {
        if let Some(value) = value {
            repo.set_string_ref(qname, field, value)?;
            changed = true;
        }
    }
    if let Some(source) = fields.source.as_deref() {
        repo.set_source(qname, source)?;
        changed = true;
    }
    if !fields.fields.is_empty() || !fields.refs.is_empty() {
        apply_generic_assignments(repo, qname, fields)?;
        changed = true;
    }
    if !changed {
        bail!("nothing to update; pass at least one field flag");
    }
    Ok(())
}

fn create_object(
    repo: &Repository,
    kind: &str,
    name: Option<&str>,
    parent: Option<&str>,
    namespace: Option<&str>,
    prototype: Option<&str>,
    source: Option<&str>,
    file: Option<&str>,
    module: Option<&str>,
) -> Result<(SymbolHash, ObjectHash)> {
    let require_parent =
        || parent.ok_or_else(|| anyhow::anyhow!("{kind} requires --parent <qname>"));
    let require_name = || name.ok_or_else(|| anyhow::anyhow!("{kind} requires <name>"));
    let (qname, _) = match kind {
        "technical-system" => repo.create_technical_system(require_name()?),
        "technical-module" => repo.create_technical_module(require_parent()?, require_name()?),
        "technical-layer" => repo.create_technical_layer(require_parent()?, require_name()?),
        "technical-namespace" => {
            repo.create_technical_namespace(require_parent()?, require_name()?)
        }
        "technical-class" => repo.create_technical_class(parent, namespace, require_name()?),
        "technical-struct" => repo.create_technical_struct(parent, namespace, require_name()?),
        "technical-enum" => repo.create_technical_enum(parent, namespace, require_name()?),
        "technical-interface" => {
            repo.create_technical_interface(parent, namespace, require_name()?)
        }
        "technical-mixin" => repo.create_technical_mixin(parent, namespace, require_name()?),
        "technical-function" => {
            let namespace = namespace.ok_or_else(|| {
                anyhow::anyhow!("technical-function requires --namespace <qname>")
            })?;
            repo.create_technical_function(namespace, require_name()?)
        }
        "technical-method" => {
            let prototype = prototype
                .ok_or_else(|| anyhow::anyhow!("technical-method requires --prototype <text>"))?;
            repo.create_technical_method(require_parent()?, prototype)
        }
        "technical-part" => {
            let source = source
                .ok_or_else(|| anyhow::anyhow!("technical-part requires --source <path:lines>"))?;
            repo.create_technical_part(require_parent()?, source)
        }
        "requirement-user" => {
            bail!("requirement-user requires --display-name; use `arche create requirement-user`")
        }
        "requirement-story" => repo.create_requirement_story(require_name()?),
        "requirement-system" => repo.create_requirement_system(require_name()?),
        "requirement-module" => repo.create_requirement_module(require_parent()?, require_name()?),
        "requirement-function" => {
            repo.create_requirement_function(require_parent()?, require_name()?)
        }
        "requirement-interaction" => {
            repo.create_requirement_interaction(require_parent()?, require_name()?)
        }
        other => bail!(
            "unsupported object kind {other}; supported: technical-system, \
             technical-module, technical-layer, technical-namespace, technical-class, \
             technical-struct, technical-enum, technical-interface, technical-method, \
             technical-function, technical-mixin, technical-part, requirement-user, \
             requirement-story, requirement-system, requirement-module, \
             requirement-function, requirement-interaction"
        ),
    }?;
    let qname_text = repo.resolve(qname)?;
    if let Some(file) = file {
        repo.set_string_ref(&qname_text, "file", file)?;
    }
    if let Some(module) = module {
        repo.set_string_ref(&qname_text, "module", module)?;
    }
    if let Some(source) = source
        && kind != "technical-part"
    {
        repo.set_source(&qname_text, source)?;
    }
    let (hash, _) = repo.show_qname(&qname_text)?;
    Ok((qname, hash))
}

fn batch(repo: &Repository, file: &str) -> Result<()> {
    let text = if file == "-" {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
    } else {
        std::fs::read_to_string(file)?
    };
    let plan: serde_json::Value = serde_json::from_str(&text).context("parse batch JSON")?;
    let mut created = 0;
    if let Some(objects) = plan.get("objects").and_then(|value| value.as_array()) {
        for object in objects {
            let field = |name: &str| object.get(name).and_then(|value| value.as_str());
            let kind = field("kind").context("batch object requires kind")?;
            let (qname, hash) = if kind == "requirement-user" {
                let name = field("name").context("requirement-user requires name")?;
                let display_name =
                    field("display_name").context("requirement-user requires display_name")?;
                let (qname, _) = repo.create_requirement_user(name, display_name)?;
                let qname_text = repo.resolve(qname)?;
                apply_batch_user_fields(repo, &qname_text, &object)?;
                let hash = repo.find_by_qname(&qname_text)?;
                (qname, hash)
            } else {
                create_object(
                    repo,
                    kind,
                    field("name"),
                    field("parent"),
                    field("namespace"),
                    field("prototype"),
                    field("source"),
                    field("file"),
                    field("module"),
                )?
            };
            println!("{}", repo.resolve(qname)?);
            println!("object {}", hash.to_hex());
            created += 1;
        }
    }
    if let Some(documents) = plan.get("documents").and_then(|value| value.as_array()) {
        for document in documents {
            let qname = document
                .get("qname")
                .and_then(|value| value.as_str())
                .context("batch document requires qname")?;
            let fields = document
                .get("fields")
                .cloned()
                .context("batch document requires fields")?;
            let projection = serde_json::json!({ "fields": fields });
            repo.import_doc_json(qname, &projection.to_string())?;
            created += 1;
        }
    }
    println!("applied {created} batch entries");
    Ok(())
}

fn apply_batch_user_fields(
    repo: &Repository,
    qname: &str,
    object: &serde_json::Value,
) -> Result<()> {
    for field in ["age", "occupation"] {
        if let Some(value) = object.get(field).and_then(|value| value.as_str()) {
            repo.set_scalar(qname, field, value)?;
        }
    }
    if let Some(priority) = object.get("priority") {
        let priority = priority
            .as_str()
            .map(str::to_owned)
            .or_else(|| priority.as_u64().map(|value| value.to_string()))
            .context("batch priority must be a name or number")?;
        let value = normalize_scalar_value("priority", &priority)?;
        repo.set_scalar(qname, "priority", &value)?;
    }
    for field in ["situations", "hobbies", "needs"] {
        if let Some(values) = object.get(field).and_then(|value| value.as_array()) {
            for value in values {
                if let Some(value) = value.as_str() {
                    repo.doc_list_item_add(qname, field, value)?;
                }
            }
        }
    }
    if let Some(stories) = object.get("stories").and_then(|value| value.as_array()) {
        let stories = stories
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        if !stories.is_empty() {
            repo.set_refs(qname, "stories", &stories)?;
        }
    }
    Ok(())
}

fn doc_grep(repo: &Repository, text: &str) -> Result<()> {
    for (_, object) in repo.list_objects()? {
        let haystack = match &object {
            Object::DocumentText(value) => value.content(),
            Object::DocumentTopic(value) => value.title(),
            Object::DocumentCode(value) => value.source(),
            Object::DocumentMermaid(value) => value.source(),
            Object::DocumentHtml(value) => value.source(),
            _ => continue,
        };
        if haystack.contains(text) {
            println!("{}\t{haystack}", repo.resolve(object.qname())?);
        }
    }
    Ok(())
}

fn doc_layout(repo: &Repository, qname: &str) -> Result<()> {
    let (_, object) = repo.show_qname(qname)?;
    println!("{}", object.kind().name());
    for property in arche_core::PropertyAccess::properties(&object) {
        println!("{}\t{:?}", property.name, property.kind);
    }
    Ok(())
}

fn print_snapshot_changes(repo: &Repository, old: &Snapshot, new: &Snapshot) -> Result<()> {
    let mut qnames = BTreeSet::new();
    qnames.extend(old.entries().iter().map(|(qname, _)| *qname));
    qnames.extend(new.entries().iter().map(|(qname, _)| *qname));
    let mut rows = Vec::new();
    for qname in qnames {
        if qname == SymbolHash::of("root") {
            continue;
        }
        let old_hash = old.get(qname);
        let new_hash = new.get(qname);
        let status = match (old_hash, new_hash) {
            (None, Some(_)) => 'A',
            (Some(_), None) => 'D',
            (Some(old), Some(new)) if old != new => 'M',
            _ => continue,
        };
        rows.push((qname, status));
    }
    rows.sort_by_key(|(qname, _)| qname.to_hex());
    for (qname, status) in rows {
        println!("  {status} {}", repo.resolve(qname)?);
    }
    Ok(())
}

fn diff(repo: &Repository, staged: bool) -> Result<()> {
    let head = repo.head_snapshot()?;
    let index = repo.index_snapshot()?;
    let worktree = repo.worktree_snapshot()?;
    let (old, new, old_label, new_label) = if staged {
        (&head, &index, "HEAD", "index")
    } else {
        (&index, &worktree, "index", "worktree")
    };
    println!("--- {old_label}");
    println!("+++ {new_label}");
    let mut qnames = BTreeSet::new();
    qnames.extend(old.entries().iter().map(|(qname, _)| *qname));
    qnames.extend(new.entries().iter().map(|(qname, _)| *qname));
    let mut rows = Vec::new();
    for qname in qnames {
        if qname == SymbolHash::of("root") {
            continue;
        }
        let old_hash = old.get(qname);
        let new_hash = new.get(qname);
        if old_hash == new_hash {
            continue;
        }
        rows.push((qname, old_hash, new_hash));
    }
    rows.sort_by_key(|(qname, _, _)| qname.to_hex());
    for (qname, old_hash, new_hash) in rows {
        let status = match (old_hash, new_hash) {
            (None, Some(_)) => 'A',
            (Some(_), None) => 'D',
            _ => 'M',
        };
        println!("{status} {}", repo.resolve(qname)?);
        if let (Some(old_hash), Some(new_hash)) = (old_hash, new_hash) {
            let old_object = repo.read(old_hash)?;
            let new_object = repo.read(new_hash)?;
            if let (Object::DocumentText(old_text), Object::DocumentText(new_text)) =
                (&old_object, &new_object)
            {
                println!("  - {}", old_text.content());
                println!("  + {}", new_text.content());
                continue;
            }
            println!(
                "  old {} ({}), new {} ({})",
                old_hash.to_hex(),
                old_object.kind().name(),
                new_hash.to_hex(),
                new_object.kind().name()
            );
        }
    }
    Ok(())
}

fn print_design_fields(repo: &Repository, fields: &[(&str, arche_core::ObjectRef)]) -> Result<()> {
    for (name, value) in fields {
        if value.is_zero() {
            continue;
        }
        let object = repo.read(value.get())?;
        if let Object::DocumentText(text) = object {
            println!("{name}: {}", text.content());
            continue;
        }
        println!("{name}: {}", repo.resolve(object.qname())?);
    }
    Ok(())
}

trait PrintableRef: Copy {
    fn is_empty_ref(self) -> bool;
    fn printable(self, repo: &Repository) -> Result<String>;
}

impl PrintableRef for arche_core::SymbolHash {
    fn is_empty_ref(self) -> bool {
        self.0.is_zero()
    }

    fn printable(self, repo: &Repository) -> Result<String> {
        repo.resolve(self)
    }
}

impl PrintableRef for arche_core::ObjectHash {
    fn is_empty_ref(self) -> bool {
        self.0.is_zero()
    }

    fn printable(self, repo: &Repository) -> Result<String> {
        repo.resolve(repo.read(self)?.qname())
    }
}

impl PrintableRef for arche_core::Hash32 {
    fn is_empty_ref(self) -> bool {
        self.is_zero()
    }

    fn printable(self, _repo: &Repository) -> Result<String> {
        Ok(self.to_hex())
    }
}

impl PrintableRef for arche_core::StringRef {
    fn is_empty_ref(self) -> bool {
        self.is_zero()
    }

    fn printable(self, repo: &Repository) -> Result<String> {
        repo.resolve(self)
    }
}

impl PrintableRef for arche_core::ObjectRef {
    fn is_empty_ref(self) -> bool {
        self.is_zero()
    }

    fn printable(self, repo: &Repository) -> Result<String> {
        repo.resolve(repo.read(self.get())?.qname())
    }
}

impl PrintableRef for arche_core::SourcePath {
    fn is_empty_ref(self) -> bool {
        self.path.is_zero()
    }

    fn printable(self, repo: &Repository) -> Result<String> {
        repo.resolve(self.path)
    }
}

impl PrintableRef for arche_core::SourceSpan {
    fn is_empty_ref(self) -> bool {
        self.path.is_zero()
    }

    fn printable(self, repo: &Repository) -> Result<String> {
        Ok(format!(
            "{}:{}-{}",
            repo.resolve(self.path)?,
            self.start,
            self.end
        ))
    }
}

fn optional_symbol<T: PrintableRef>(repo: &Repository, value: T) -> Result<String> {
    if value.is_empty_ref() {
        Ok(String::from("-"))
    } else {
        value.printable(repo)
    }
}

fn print_symbol_list(
    repo: &Repository,
    label: &str,
    values: &arche_core::StringRefList,
) -> Result<()> {
    let values = values
        .as_slice()
        .iter()
        .map(|value| repo.resolve(*value))
        .collect::<Result<Vec<_>>>()?
        .join(", ");
    println!("{label}: {values}");
    Ok(())
}
