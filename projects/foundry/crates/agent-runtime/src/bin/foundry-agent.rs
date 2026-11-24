//! Foundry agent runtime CLI.

use clap::{Args, Parser, Subcommand};
use foundry_agent_runtime::{
    AgentRunRequest, AgentRuntime, CLARIFY_SYSTEM_PROMPT, ContextBundle, EchoTool, FoundryClient,
    IssueCommentTool, IssueGetTool, LocalSandbox, TaskAllowedOperationsTool, TaskCommentTool,
    TaskLinksListTool, TaskOperationTool, ToolRegistry,
};
use graph_agent::{OpenAiCompatibleConfig, OpenAiCompatibleModel, ScriptedModel};
use serde_json::json;
use std::sync::Arc;

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

    let model = Arc::new(ScriptedModel::new(vec![
        graph_agent::LlmResponse::tool_call("echo", json!({ "text": "hello" })),
        graph_agent::LlmResponse::final_answer("demo complete".to_string()),
    ]));
    let runtime = AgentRuntime::new(model, Arc::new(tools));

    let result = runtime
        .run(AgentRunRequest::new(
            "agent-demo",
            "task-demo",
            "You are Foundry's requirements analyst.",
            "Inspect the requirements document and report what is missing.",
        ))
        .await?;

    println!("{}", serde_json::to_string_pretty(&result)?);
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

    match (&args.foundry_url, &args.foundry_token, &args.project_id) {
        (Some(_), Some(_), Some(_)) => {}
        (None, None, None) => {}
        _ => {
            return Err(
                "foundry_url, foundry_token and project_id must be provided together".into(),
            );
        }
    }

    let runtime = AgentRuntime::new(model, Arc::new(tools));

    let mut request = AgentRunRequest::new("agent-cli", "task-cli", args.system, args.message);
    request.context = ContextBundle::default();

    let result = runtime.run(request).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
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
    if let Some(root) = &args.sandbox_root {
        tools.register(Arc::new(LocalSandbox::new(root)?));
    }

    let mut context = ContextBundle::default();
    context.push(
        "current_task",
        args.task_id.clone(),
        format!("task_id = {}", args.task_id),
    );

    let runtime = AgentRuntime::new(model, Arc::new(tools));

    let mut request = AgentRunRequest::new(
        "agent-clarifier",
        args.task_id.clone(),
        CLARIFY_SYSTEM_PROMPT,
        args.message,
    );
    request.context = context;

    let result = runtime.run(request).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
