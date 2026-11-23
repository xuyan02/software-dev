use std::path::PathBuf;

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use arche::snapshot::Snapshot;
use arche::{ConceptRelation, Object, ObjectHash, Repository, SymbolHash};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "arche", version, about = "A versioned design repository")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    Push,
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
    Create {
        /// Object kind, for example technical-system.
        kind: String,
        /// Local object name.
        name: String,
        /// Parent qname for nested objects.
        #[arg(long)]
        parent: Option<String>,
    },
    /// Document content operations.
    Doc {
        #[command(subcommand)]
        command: DocCommand,
    },
    /// Comment operations.
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
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
        /// Object qnames to stage.
        qnames: Vec<String>,
    },
    /// Restore index entries from HEAD.
    Unstage {
        /// Object qnames to unstage.
        qnames: Vec<String>,
    },
    /// Show repository status.
    Status,
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
    },
    /// Reset index and optionally worktree to HEAD.
    Reset {
        #[arg(long)]
        hard: bool,
    },
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
        /// Only list comments for this target.
        target: Option<String>,
        /// Only open comments.
        #[arg(long)]
        open: bool,
    },
    /// Resolve a comment.
    Resolve {
        /// Comment qname.
        qname: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConceptCommand {
    /// Create a concept directory.
    Mkdir {
        /// Directory qname under concepts/domain, concepts/requirements, or concepts/technical.
        qname: String,
    },
    /// Create a concept.
    Create {
        /// Concept qname.
        qname: String,
        /// Display name. Defaults to the last path segment.
        #[arg(long)]
        name: Option<String>,
        /// One-sentence summary.
        #[arg(long)]
        summary: String,
    },
    /// Link two concepts.
    Link {
        #[command(subcommand)]
        command: ConceptLinkCommand,
    },
    /// List concepts.
    List,
}

#[derive(Debug, Subcommand)]
enum ConceptLinkCommand {
    /// Add a concept link.
    Add {
        /// Source concept qname.
        qname: String,
        /// Target concept qname.
        target: String,
        /// refines, realizes, or corresponds.
        relation: String,
    },
    /// Remove links from a source concept to a target concept.
    Remove {
        /// Source concept qname.
        qname: String,
        /// Target concept qname.
        target: String,
    },
}

#[derive(Debug, Subcommand)]
enum RequirementCommand {
    /// Create a requirement object.
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
        name: String,
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
    /// Align TechnicalFile objects with tracked source files.
    Sync,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { dir, source } => {
            let repo = Repository::init_with_source(
                dir.unwrap_or_else(|| PathBuf::from(".")),
                source.as_deref(),
            )?;
            println!("Initialized Arche repository at {}", repo.dir().display());
        }
        Command::Clone { remote, dir } => {
            let repo = Repository::clone_from(&remote, dir.unwrap_or_else(|| PathBuf::from(".")))?;
            println!("Cloned Arche repository into {}", repo.root().display());
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
        Command::Push => {
            Repository::open(".")?.remote_push()?;
        }
        Command::Fetch => {
            Repository::open(".")?.remote_fetch()?;
        }
        Command::Pull => {
            Repository::open(".")?.remote_pull()?;
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
                SourceCommand::Sync => {
                    let count = repo.source_sync()?;
                    println!("synced {count} source files");
                }
            }
        }
        Command::Create { kind, name, parent } => {
            let repo = Repository::open(".")?;
            match kind.as_str() {
                "technical-system" => {
                    let (qname, hash) = repo.create_technical_system(&name)?;
                    println!("{}", repo.resolve(qname)?);
                    println!("object {}", hash.to_hex());
                }
                "technical-module" => {
                    let parent = parent.ok_or_else(|| {
                        anyhow::anyhow!("technical-module requires --parent <qname>")
                    })?;
                    let (qname, hash) = repo.create_technical_module(&parent, &name)?;
                    println!("{}", repo.resolve(qname)?);
                    println!("object {}", hash.to_hex());
                }
                other => bail!(
                    "unsupported object kind {other}; supported: technical-system, technical-module"
                ),
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
            }
        }
        Command::Comment { command } => {
            let repo = Repository::open(".")?;
            match command {
                CommentCommand::Add { target, text } => {
                    let (qname, hash) = repo.comment_add(&target, &text)?;
                    println!("{}", repo.resolve(qname)?);
                    println!("object {}", hash.to_hex());
                }
                CommentCommand::List { target, open } => {
                    for comment in repo.comment_list(target.as_deref())? {
                        if open && comment.resolved {
                            continue;
                        }
                        let state = if comment.resolved { "resolved" } else { "open" };
                        println!(
                            "{}\t{}\t{}\t{}",
                            repo.resolve(comment.qname)?,
                            state,
                            repo.resolve(comment.target)?,
                            comment.body_text()
                        );
                    }
                }
                CommentCommand::Resolve { qname } => {
                    repo.comment_resolve(&qname)?;
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
                    summary,
                } => {
                    let name = name.unwrap_or_else(|| {
                        qname
                            .rsplit_once('/')
                            .map(|(_, name)| name.to_owned())
                            .unwrap_or_else(|| qname.clone())
                    });
                    let (qname, hash) = repo.concept_create(&qname, &name, &summary)?;
                    println!("{}", repo.resolve(qname)?);
                    println!("object {}", hash.to_hex());
                }
                ConceptCommand::Link { command } => match command {
                    ConceptLinkCommand::Add {
                        qname,
                        target,
                        relation,
                    } => {
                        let relation = match relation.as_str() {
                            "refines" => ConceptRelation::Refines,
                            "realizes" => ConceptRelation::Realizes,
                            "corresponds" => ConceptRelation::Corresponds,
                            other => bail!("unknown concept relation {other}"),
                        };
                        repo.concept_link_add(&qname, &target, relation)?;
                    }
                    ConceptLinkCommand::Remove { qname, target } => {
                        repo.concept_link_remove(&qname, &target)?;
                    }
                },
                ConceptCommand::List => {
                    for (qname, concept) in repo.concept_list()? {
                        println!("{}\t{}", repo.resolve(qname)?, repo.resolve(concept.name)?);
                    }
                }
            }
        }
        Command::Requirement { command } => {
            let repo = Repository::open(".")?;
            match command {
                RequirementCommand::Create { command } => {
                    let (qname, hash) = match command {
                        RequirementCreateCommand::User { name } => {
                            repo.create_requirement_user(&name)?
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
        Command::Add { all, file, qnames } => {
            let repo = Repository::open(".")?;
            if let Some(file) = file {
                repo.add_file(&file)?;
            } else if all || qnames.is_empty() {
                repo.add_all()?;
            } else {
                repo.add(&qnames)?;
            }
        }
        Command::Unstage { qnames } => {
            let repo = Repository::open(".")?;
            repo.unstage(&qnames)?;
        }
        Command::Status => {
            let repo = Repository::open(".")?;
            status(&repo)?;
        }
        Command::Diff { staged } => {
            let repo = Repository::open(".")?;
            diff(&repo, staged)?;
        }
        Command::Commit {
            message,
            allow_empty,
        } => {
            let repo = Repository::open(".")?;
            let hash = repo.commit_index(&message, allow_empty)?;
            println!("{}", hash.to_hex());
        }
        Command::Reset { hard } => {
            let repo = Repository::open(".")?;
            repo.reset(hard)?;
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

fn show_object(repo: &Repository, hash: arche::ObjectHash, object: &Object) -> Result<()> {
    println!("qname: {}", repo.resolve(object.qname())?);
    if let Some(name) = object.name() {
        println!("name:  {}", repo.resolve(name)?);
    }
    println!("kind:  {}", object.kind().name());
    println!("hash:  {}", hash.to_hex());

    match object {
        Object::Technical(value) => {
            println!("files: {}", optional_symbol(repo, value.files)?);
            println!("namespaces: {}", optional_symbol(repo, value.namespaces)?);
            println!("systems: {}", optional_symbol(repo, value.systems)?);
        }
        Object::Requirements(value) => {
            println!("users: {}", optional_symbol(repo, value.users)?);
            println!("stories: {}", optional_symbol(repo, value.stories)?);
            println!("systems: {}", optional_symbol(repo, value.systems)?);
        }
        Object::TechnicalSystem(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            println!("members: {}", optional_symbol(repo, value.members)?);
            println!("modules: {}", optional_symbol(repo, value.modules)?);
            println!("layers: {}", optional_symbol(repo, value.layers)?);
            print_design_fields(
                repo,
                &[
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
            )?;
        }
        Object::TechnicalModule(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            println!("members: {}", optional_symbol(repo, value.members)?);
            println!("modules: {}", optional_symbol(repo, value.modules)?);
            println!("layers: {}", optional_symbol(repo, value.layers)?);
            print_design_fields(
                repo,
                &[
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
            )?;
        }
        Object::TechnicalDirectory(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            println!("source_path: {}", optional_symbol(repo, value.source_path)?);
            println!("children: {}", optional_symbol(repo, value.children)?);
            print_design_fields(repo, &[("position", value.position)])?;
        }
        Object::TechnicalFile(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            println!("source_path: {}", optional_symbol(repo, value.source_path)?);
            println!("source_hash: {}", value.source_hash);
            println!("children: {}", optional_symbol(repo, value.children)?);
            print_design_fields(repo, &[("position", value.position)])?;
        }
        Object::RequirementUser(value) => {
            print_design_fields(
                repo,
                &[
                    ("age", value.age),
                    ("basic_situation", value.basic_situation),
                    ("hobbies", value.hobbies),
                    ("occupation", value.occupation),
                    ("typical_scenario", value.typical_scenario),
                    ("needs", value.needs),
                    ("priority", value.priority),
                ],
            )?;
        }
        Object::RequirementStory(value) => {
            print_design_fields(
                repo,
                &[
                    ("preconditions", value.preconditions),
                    ("frequency", value.frequency),
                    ("description", value.description),
                    ("related_requirements", value.related_requirements),
                ],
            )?;
        }
        Object::RequirementSystem(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            println!("modules: {}", optional_symbol(repo, value.modules)?);
            print_design_fields(
                repo,
                &[
                    ("form", value.form),
                    ("covered_users", value.covered_users),
                    ("closed_loop_goal", value.closed_loop_goal),
                    ("relationships", value.relationships),
                    ("boundary", value.boundary),
                ],
            )?;
        }
        Object::RequirementModule(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            println!("functions: {}", optional_symbol(repo, value.functions)?);
            print_design_fields(
                repo,
                &[
                    ("core_concept", value.core_concept),
                    ("included_concepts", value.included_concepts),
                    ("concept_relations", value.concept_relations),
                    ("boundary", value.boundary),
                ],
            )?;
        }
        Object::RequirementFunction(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            println!("flow: {}", optional_symbol(repo, value.flow)?);
            println!(
                "interactions: {}",
                optional_symbol(repo, value.interactions)?
            );
            print_design_fields(
                repo,
                &[
                    ("initiator", value.initiator),
                    ("system_change", value.system_change),
                    ("preconditions", value.preconditions),
                    ("feedback", value.feedback),
                    ("description", value.description),
                ],
            )?;
        }
        Object::RequirementInteraction(value) => {
            println!("parent: {}", repo.resolve(value.parent)?);
            print_design_fields(
                repo,
                &[("description", value.description), ("ui", value.ui)],
            )?;
        }
        Object::ObjectList(value) => {
            println!("owner: {}", repo.resolve(value.owner)?);
            println!("items: {}", value.items.len());
            for item in &value.items {
                println!("  - {}", repo.resolve(*item)?);
            }
        }
        Object::View(value) => {
            println!("source_commit: {}", value.source_commit);
            println!("technical: {}", value.technical.to_hex());
            println!("requirements: {}", value.requirements.to_hex());
            println!("concepts: {}", value.concepts.to_hex());
            if let Some(comments) = value.comments {
                println!("comments: {}", comments.to_hex());
            }
        }
        Object::Concept(value) => {
            print_design_fields(
                repo,
                &[
                    ("summary", value.summary),
                    ("explanation", value.explanation),
                    ("characteristics", value.characteristics),
                    ("relations", value.relations),
                    ("aliases", value.aliases),
                ],
            )?;
            for link in &value.links {
                println!("link: {} ({:?})", repo.resolve(link.target)?, link.relation);
            }
        }
        Object::Comment(value) => {
            println!("target: {}", repo.resolve(value.target)?);
            println!("resolved: {}", value.resolved);
            println!("text: {}", value.body_text());
        }
        Object::DocumentText(value) => {
            println!("text: {}", arche::object::inline_plain_text(&value.content));
        }
        Object::DocumentTopic(value) => {
            println!("title: {}", arche::object::inline_plain_text(&value.title));
            println!("body: {}", value.body.to_hex());
        }
        Object::DocumentList(value) => {
            println!("ordered: {}", value.ordered);
            println!("items: {}", value.items.len());
        }
        Object::DocumentParagraphList(value) => {
            println!("items: {}", value.items.len());
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
    if let Some(name) = object.name() {
        print!(",\"name\":\"{}\"", json_escape(&repo.resolve(name)?));
    }
    let fields = arche::repo::doc_fields(object);
    let present = fields
        .into_iter()
        .filter_map(|(field, value)| value.map(|value| (field, value)))
        .collect::<Vec<_>>();
    print!(",\"fields\":{{");
    for (index, (field, value)) in present.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        let field_qname = repo.resolve(*value)?;
        let kind = repo
            .show_qname(&field_qname)
            .map(|(_, object)| object.kind().name().to_owned())
            .unwrap_or_else(|_| "missing".to_owned());
        print!(
            "\"{}\":{{\"qname\":\"{}\",\"kind\":\"{}\"}}",
            json_escape(field),
            json_escape(&field_qname),
            json_escape(&kind)
        );
    }
    println!("}}}}");
    Ok(())
}

fn show_markdown(repo: &Repository, _hash: ObjectHash, object: &Object) -> Result<()> {
    let qname = repo.resolve(object.qname())?;
    let title = object
        .name()
        .map(|name| repo.resolve(name))
        .transpose()?
        .unwrap_or_else(|| qname.clone());
    println!("# {title}\n");
    println!("- qname: `{qname}`");
    println!("- kind: `{}`\n", object.kind().name());
    for (field, value) in arche::repo::doc_fields(object) {
        let Some(value) = value else { continue };
        let field_qname = repo.resolve(value)?;
        println!("## {field}\n");
        match repo.show_qname(&field_qname) {
            Ok((_, Object::DocumentText(text))) => {
                println!("{}\n", arche::object::inline_plain_text(&text.content));
            }
            Ok((_, Object::DocumentTopic(topic))) => {
                println!("### {}\n", arche::object::inline_plain_text(&topic.title));
            }
            Ok((_, Object::DocumentMermaid(value))) => {
                println!("```mermaid\n{}\n```\n", value.source);
            }
            Ok((_, Object::DocumentHtml(value))) => {
                println!("```html\n{}\n```\n", value.source);
            }
            Ok((_, Object::DocumentList(value))) => {
                println!("- items: {}\n", value.items.len());
            }
            Ok((_, Object::DocumentParagraphList(value))) => {
                println!("- paragraphs: {}\n", value.items.len());
            }
            Ok((_, other)) => {
                println!("- {}: `{}`\n", other.kind().name(), field_qname);
            }
            Err(_) => {}
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

fn status(repo: &Repository) -> Result<()> {
    let head = repo.head_snapshot()?;
    let index = repo.index_snapshot()?;
    let worktree = repo.worktree_snapshot()?;
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

fn print_snapshot_changes(repo: &Repository, old: &Snapshot, new: &Snapshot) -> Result<()> {
    let mut qnames = BTreeSet::new();
    qnames.extend(old.entries().iter().map(|(qname, _)| *qname));
    qnames.extend(new.entries().iter().map(|(qname, _)| *qname));
    let mut rows = Vec::new();
    for qname in qnames {
        if qname == SymbolHash::of("view") {
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
        if qname == SymbolHash::of("view") {
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
                println!(
                    "  - {}",
                    arche::object::inline_plain_text(&old_text.content)
                );
                println!(
                    "  + {}",
                    arche::object::inline_plain_text(&new_text.content)
                );
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

fn print_design_fields(
    repo: &Repository,
    fields: &[(&str, Option<arche::SymbolHash>)],
) -> Result<()> {
    for (name, value) in fields {
        if let Some(value) = value {
            let qname = repo.resolve(*value)?;
            if let Ok((_, Object::DocumentText(text))) = repo.show_qname(&qname) {
                println!(
                    "{name}: {}",
                    arche::object::inline_plain_text(&text.content)
                );
                continue;
            }
            println!("{name}: {qname}");
        }
    }
    Ok(())
}

fn optional_symbol(repo: &Repository, value: Option<arche::SymbolHash>) -> Result<String> {
    match value {
        Some(value) => repo.resolve(value),
        None => Ok(String::from("-")),
    }
}
