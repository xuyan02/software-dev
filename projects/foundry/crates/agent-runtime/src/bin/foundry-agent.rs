//! Foundry agent runtime CLI.

use clap::{Args, Parser, Subcommand};
use foundry_agent_runtime::{
    AgentRunRequest, AgentRuntime, Budget, CLARIFY_SYSTEM_PROMPT, ContextBundle, DocumentTool,
    EchoTool, FoundryClient, FoundryRequirementsApplyTool, FoundryRequirementsDraftTool,
    FoundryRequirementsGetTool, FoundryRequirementsValidateTool, InMemoryTrajectory,
    IssueCommentTool, IssueGetTool, LocalSandbox, ModelResponse, OpenAiCompatibleConfig,
    OpenAiCompatibleModel, Pricing, RequirementsGetTool, RunOutcome, ScriptedModel,
    TaskAllowedOperationsTool, TaskCommentTool, TaskLinksListTool, TaskOperationTool, ToolRegistry,
};
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

#[derive(Parser)]
#[command(name = "foundry-agent", version, about = "Foundry agent runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a deterministic scripted agent.
    Demo,
    /// Run an agent against an OpenAI-compatible endpoint.
    Run(Box<RunArgs>),
    /// Clarify a project through its unified Issue and draft its requirements document.
    Clarify(Box<ClarifyArgs>),
}

#[derive(Args)]
struct RunArgs {
    #[arg(
        long,
        env = "OPENAI_BASE_URL",
        default_value = "http://127.0.0.1:11434/v1"
    )]
    base_url: String,
    #[arg(long, env = "OPENAI_MODEL", default_value = "qwen2.5-coder")]
    model: String,
    #[arg(long, env = "OPENAI_API_KEY", default_value = "")]
    api_key: String,
    #[arg(long, default_value = "You are Foundry's software engineering agent.")]
    system: String,
    #[arg(long)]
    message: String,
    #[arg(long, env = "FOUNDRY_URL")]
    foundry_url: Option<String>,
    #[arg(long, env = "FOUNDRY_TOKEN")]
    foundry_token: Option<String>,
    #[arg(long, alias = "project", env = "FOUNDRY_PROJECT")]
    project_id: Option<String>,
    #[arg(long, env = "FOUNDRY_SANDBOX_ROOT")]
    sandbox_root: Option<String>,
    #[arg(long, env = "INPUT_PRICE_PER_MILLION", default_value_t = 0.0)]
    input_price: f64,
    #[arg(long, env = "OUTPUT_PRICE_PER_MILLION", default_value_t = 0.0)]
    output_price: f64,
}

#[derive(Args)]
struct ClarifyArgs {
    #[arg(
        long,
        env = "OPENAI_BASE_URL",
        default_value = "http://127.0.0.1:11434/v1"
    )]
    base_url: String,
    #[arg(long, env = "OPENAI_MODEL", default_value = "qwen2.5-coder")]
    model: String,
    #[arg(long, env = "OPENAI_API_KEY", default_value = "")]
    api_key: String,
    #[arg(long, env = "FOUNDRY_URL")]
    foundry_url: String,
    #[arg(long, env = "FOUNDRY_TOKEN")]
    foundry_token: String,
    #[arg(long, alias = "project", env = "FOUNDRY_PROJECT")]
    project_id: String,
    /// Task whose links include the unified clarification Issue.
    #[arg(long, env = "FOUNDRY_TASK")]
    task_id: String,
    #[arg(long, default_value = "Help me clarify this project.")]
    message: String,
    #[arg(long, env = "FOUNDRY_SANDBOX_ROOT")]
    sandbox_root: Option<String>,
    #[arg(long, env = "INPUT_PRICE_PER_MILLION", default_value_t = 0.0)]
    input_price: f64,
    #[arg(long, env = "OUTPUT_PRICE_PER_MILLION", default_value_t = 0.0)]
    output_price: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Demo => demo().await,
        Command::Run(args) => run(*args).await,
        Command::Clarify(args) => clarify(*args).await,
    }
}

async fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));
    tools.register(Arc::new(RequirementsGetTool::new(json!({
        "users": [],
        "functions": []
    }))));

    let model = Arc::new(ScriptedModel::new(vec![
        ModelResponse::tool_call("requirements_get", json!({})),
        ModelResponse::final_answer(
            "The requirements document is currently empty; start with a user model.",
        ),
    ]));
    let trajectory = Arc::new(InMemoryTrajectory::new());
    let runtime = AgentRuntime::new(
        model,
        Arc::new(tools),
        trajectory.clone(),
        Budget::default(),
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new(
            "agent-demo",
            "task-demo",
            "You are Foundry's requirements analyst.",
            "Inspect the requirements document and report what is missing.",
        ))
        .await?;

    println!("{}", serde_json::to_string_pretty(&result)?);
    println!("{}", serde_json::to_string_pretty(&trajectory.steps())?);
    Ok(())
}

async fn run(args: RunArgs) -> Result<(), Box<dyn std::error::Error>> {
    let model = Arc::new(OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
        args.base_url,
        args.api_key,
        args.model,
    ))?);

    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));
    if let Some(root) = &args.sandbox_root {
        tools.register(Arc::new(LocalSandbox::new(root)?));
    }

    let mut context = ContextBundle::default();
    match (&args.foundry_url, &args.foundry_token, &args.project_id) {
        (Some(url), Some(token), Some(project_id)) => {
            let client = Arc::new(FoundryClient::new(url, token, project_id));
            register_foundry_tools(&mut tools, &client);
            context = client.requirements_context().await?;
        }
        (None, None, None) => {}
        _ => {
            return Err(
                "foundry_url, foundry_token and project_id must be provided together".into(),
            );
        }
    }

    let trajectory = Arc::new(InMemoryTrajectory::new());
    let mut runtime = AgentRuntime::new(
        model,
        Arc::new(tools),
        trajectory.clone(),
        Budget::default(),
    );
    if args.input_price > 0.0 || args.output_price > 0.0 {
        runtime = runtime.with_pricing(Pricing::new(args.input_price, args.output_price));
    }

    let mut request = AgentRunRequest::new("agent-cli", "task-cli", args.system, args.message);
    request.context = context;

    let result = runtime.run_to_completion(request).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    println!("{}", serde_json::to_string_pretty(&trajectory.steps())?);
    Ok(())
}

async fn clarify(args: ClarifyArgs) -> Result<(), Box<dyn std::error::Error>> {
    let model = Arc::new(OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
        args.base_url,
        args.api_key,
        args.model,
    ))?);
    let client = Arc::new(FoundryClient::new(
        args.foundry_url,
        args.foundry_token,
        args.project_id.clone(),
    ));
    let prompt = CliPrompt::new();

    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));
    tools.register(Arc::new(TaskAllowedOperationsTool::new(Arc::clone(
        &client,
    ))));
    tools.register(Arc::new(TaskOperationTool::new(Arc::clone(&client))));
    tools.register(Arc::new(TaskCommentTool::new(Arc::clone(&client))));
    tools.register(Arc::new(TaskLinksListTool::new(Arc::clone(&client))));
    tools.register(Arc::new(IssueGetTool::new(Arc::clone(&client))));
    tools.register(Arc::new(IssueCommentTool::new(Arc::clone(&client))));
    register_foundry_tools(&mut tools, &client);
    if let Some(root) = &args.sandbox_root {
        tools.register(Arc::new(LocalSandbox::new(root)?));
    }

    let mut context = ContextBundle::default();
    if let Ok(document) = client.get_anchor_document_toc("project", None).await {
        context.push("project_document", args.project_id.clone(), document);
    }
    if let Ok(document) = client.get_document().await {
        context.push(
            "requirements_document",
            args.project_id.clone(),
            serde_json::to_string_pretty(&document)?,
        );
    }
    context.push(
        "current_task",
        args.task_id.clone(),
        format!("task_id = {}", args.task_id),
    );

    let trajectory = Arc::new(InMemoryTrajectory::new());
    let mut runtime = AgentRuntime::new(
        model,
        Arc::new(tools),
        trajectory.clone(),
        Budget::default(),
    );
    if args.input_price > 0.0 || args.output_price > 0.0 {
        runtime = runtime.with_pricing(Pricing::new(args.input_price, args.output_price));
    }

    let mut request = AgentRunRequest::new(
        "agent-clarifier",
        args.task_id.clone(),
        CLARIFY_SYSTEM_PROMPT,
        args.message,
    );
    request.context = context;

    let mut outcome = runtime.run(request).await?;
    loop {
        match outcome {
            RunOutcome::Finished(result) => {
                println!("{}", serde_json::to_string_pretty(&result)?);
                break;
            }
            RunOutcome::WaitingForIssueComment { pause, checkpoint } => {
                println!(
                    "\nAgent 已在 Issue {} 下评论。你可以在 Issue 里回复，也可以在这里输入。",
                    pause.issue_id
                );
                let reply = prompt.ask("回复（直接回车结束）: ").await?;
                if reply.is_empty() {
                    println!("已结束等待。");
                    break;
                }
                client.comment_issue(pause.issue_id, &reply, false).await?;
                outcome = runtime.resume(checkpoint, reply, None).await?;
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&trajectory.steps())?);

    Ok(())
}

fn register_foundry_tools(tools: &mut ToolRegistry, client: &Arc<FoundryClient>) {
    tools.register(Arc::new(DocumentTool::new(Arc::clone(client))));
    tools.register(Arc::new(FoundryRequirementsGetTool::new(Arc::clone(
        client,
    ))));
    tools.register(Arc::new(FoundryRequirementsDraftTool::new(Arc::clone(
        client,
    ))));
    tools.register(Arc::new(FoundryRequirementsApplyTool::new(Arc::clone(
        client,
    ))));
    tools.register(Arc::new(FoundryRequirementsValidateTool::new(Arc::clone(
        client,
    ))));
}

struct CliPrompt {
    stdin: Mutex<BufReader<tokio::io::Stdin>>,
}

impl CliPrompt {
    fn new() -> Self {
        Self {
            stdin: Mutex::new(BufReader::new(tokio::io::stdin())),
        }
    }

    async fn ask(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        use std::io::Write;
        print!("{prompt}");
        std::io::stdout().flush()?;
        let mut line = String::new();
        let read = self.stdin.lock().await.read_line(&mut line).await?;
        if read == 0 {
            return Ok(String::new());
        }
        Ok(line.trim().to_string())
    }
}
