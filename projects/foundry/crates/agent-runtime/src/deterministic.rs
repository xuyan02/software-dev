//! Deterministic runtimes for stages that do not need an LLM.
//!
//! These roles only read project metadata, run shell commands, record evidence
//! in task comments and send the workflow operation implied by the exit status.
//! Keeping them out of the model loop makes the automation reproducible and
//! cheaper; stages that need judgment stay on the generic LLM runtime.

use crate::{AgentRunRequest, RunError, RunResult, Shell, ShellOutput, ToolContext, ToolRegistry};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

const MAX_EVIDENCE: usize = 12_000;
const DESIGN_BATCH_LIMIT: usize = 5;
const DEVELOPMENT_BATCH_LIMIT: usize = 5;
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "c", "cc", "cpp", "h", "hpp", "go", "java", "kt", "kts", "scala", "cs", "fs", "swift",
    "m", "mm", "py", "rb", "php", "js", "jsx", "ts", "tsx", "vue", "svelte", "html", "css", "scss",
    "sass", "less", "sql", "sh", "bash", "zsh", "fish", "ps1", "lua", "r", "jl", "ex", "exs",
    "erl", "hs", "ml", "clj", "dart", "sol", "zig", "nim",
];
const MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "composer.json",
    "CMakeLists.txt",
    "Makefile",
    "Dockerfile",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeterministicMode {
    Assessor,
    CodeTester,
    CodeMerger,
    CodeSummaryMerger,
    DesignConsistencyIntegrator,
    DesignConsistencyScanner,
    DesignCleanupDesign,
    DesignCleanupDesignChange,
    DesignCleanupDesignCorrection,
    DesignCleanupDesignBaseline,
    DesignDispatcher,
    DesignVerifier,
    DevelopmentDispatcher,
    DevelopmentVerifier,
    RequirementsDispatcher,
    RequirementsVerifier,
    TestDispatcher,
    TestVerifier,
    ReleaseManager,
    ReleaseVerifier,
    ReleaseDeployer,
    ReleaseObserver,
}

/// Hybrid roles keep the LLM for judgment stages but use code for the listed
/// mechanical stages.
pub fn handles_stage(mode: DeterministicMode, stage: Option<&str>) -> bool {
    match mode {
        DeterministicMode::DesignConsistencyIntegrator => {
            matches!(stage, Some("integration_review" | "merge" | "cleanup"))
        }
        DeterministicMode::DesignCleanupDesign
        | DeterministicMode::DesignCleanupDesignChange
        | DeterministicMode::DesignCleanupDesignCorrection
        | DeterministicMode::DesignCleanupDesignBaseline => stage == Some("cleanup"),
        _ => true,
    }
}

pub async fn run(
    mode: DeterministicMode,
    tools: &Arc<ToolRegistry>,
    shell: Option<&Arc<dyn Shell>>,
    request: AgentRunRequest,
) -> Result<RunResult, RunError> {
    let context = ToolContext {
        agent_id: request.agent_id.clone(),
        task_id: request.task_id.clone(),
        project_id: request.project_id.clone(),
    };
    let task_id = parse_task_id(&request)?;
    let result = match mode {
        DeterministicMode::Assessor => assessor(tools, shell, &context, &request, task_id).await,
        DeterministicMode::CodeTester => {
            code_tester(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::CodeMerger => {
            code_merger(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::CodeSummaryMerger => {
            code_summary_merger(tools, &context, &request, task_id).await
        }
        DeterministicMode::DesignConsistencyIntegrator => {
            design_consistency_integrator(tools, &context, &request, task_id).await
        }
        DeterministicMode::DesignConsistencyScanner => {
            design_consistency_scanner(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::DesignCleanupDesign
        | DeterministicMode::DesignCleanupDesignChange
        | DeterministicMode::DesignCleanupDesignCorrection
        | DeterministicMode::DesignCleanupDesignBaseline => {
            design_cleanup(mode, tools, &context, task_id).await
        }
        DeterministicMode::DesignDispatcher => {
            design_dispatcher(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::DesignVerifier => {
            design_verifier(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::DevelopmentDispatcher => {
            development_dispatcher(tools, &context, &request, task_id).await
        }
        DeterministicMode::DevelopmentVerifier => {
            development_verifier(tools, &context, &request, task_id).await
        }
        DeterministicMode::RequirementsDispatcher => {
            requirements_dispatcher(tools, &context, &request, task_id).await
        }
        DeterministicMode::RequirementsVerifier => {
            requirements_verifier(tools, &context, &request, task_id).await
        }
        DeterministicMode::TestDispatcher => {
            test_dispatcher(tools, &context, &request, task_id).await
        }
        DeterministicMode::TestVerifier => test_verifier(tools, &context, &request, task_id).await,
        DeterministicMode::ReleaseManager => {
            release_manager(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::ReleaseVerifier => {
            release_verifier(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::ReleaseDeployer => {
            release_deployer(tools, shell, &context, &request, task_id).await
        }
        DeterministicMode::ReleaseObserver => {
            release_observer(tools, shell, &context, &request, task_id).await
        }
    };
    result.map(|message| RunResult {
        final_message: Some(message),
    })
}

async fn assessor(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let project = project_info(tools, context).await?;
    let project_id = context
        .project_id
        .as_deref()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .ok_or_else(|| RunError::Graph("assessment requires a project id".to_string()))?;
    let Some(url) = project.get("git_repository").and_then(Value::as_str) else {
        let evidence = "项目没有 git_repository，无法读取源码。";
        comment(tools, context, task_id, evidence).await?;
        send(tools, context, task_id, "assessment:failed").await?;
        return Ok(evidence.to_string());
    };
    let branch = project
        .get("git_branch")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (arche, prepared) = ensure_arche(shell, &project, project_id).await?;
    if prepared.exit_code != Some(0) {
        let evidence = evidence(
            "项目评估",
            &format!("无法准备 Arche 工作区\n{}", shell_evidence(&prepared)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "assessment:failed").await?;
        return Ok(evidence);
    }
    let mut fetch = format!("cd {} && arche source fetch", quote(&arche));
    if !branch.is_empty() {
        fetch.push_str(&format!(" && arche source checkout {}", quote(branch)));
    }
    let fetched = shell.exec(&fetch).await.map_err(shell_error)?;
    if fetched.exit_code != Some(0) {
        let evidence = evidence(
            "项目评估",
            &format!("无法读取源码仓库 {url}\n{}", shell_evidence(&fetched)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "assessment:failed").await?;
        return Ok(evidence);
    }

    let listing = shell
        .exec(&format!("cd {} && arche source ls", quote(&arche)))
        .await
        .map_err(shell_error)?;
    if listing.exit_code != Some(0) {
        let evidence = evidence(
            "项目评估",
            &format!("无法列出源码文件\n{}", shell_evidence(&listing)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "assessment:failed").await?;
        return Ok(evidence);
    }
    let files = listing
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let has_code = has_code_files(files.iter().copied());
    let languages = language_counts(files.iter().copied());
    let commit = shell
        .exec(&format!("git -C {} rev-parse --short HEAD", quote(&arche)))
        .await
        .ok()
        .filter(|output| output.exit_code == Some(0))
        .map(|output| output.stdout.trim().to_string())
        .unwrap_or_default();
    let language_summary = languages
        .iter()
        .map(|(language, count)| format!("{language}={count}"))
        .collect::<Vec<_>>()
        .join(", ");
    let evidence = evidence(
        "项目评估",
        &format!(
            "branch={}\ncommit={}\nfiles={}\nhas_code={}\nlanguages={}\nmanifests={}",
            if branch.is_empty() {
                "(default)"
            } else {
                branch
            },
            commit,
            files.len(),
            has_code,
            language_summary,
            manifest_names(files.iter().copied()).join(", "),
        ),
    );
    let _ = request;
    comment(tools, context, task_id, &evidence).await?;
    let operation = if has_code {
        "assessment:code_present"
    } else {
        "assessment:no_code"
    };
    send(tools, context, task_id, operation).await?;
    Ok(evidence)
}

async fn code_tester(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let project = project_info(tools, context).await?;
    let project_id = context
        .project_id
        .as_deref()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .ok_or_else(|| RunError::Graph("code test requires a project id".to_string()))?;
    let url = project
        .get("git_repository")
        .and_then(Value::as_str)
        .ok_or_else(|| RunError::Graph("project has no git_repository".to_string()))?;
    let branch = project
        .get("git_branch")
        .and_then(Value::as_str)
        .unwrap_or("");
    let work_branch = request
        .task_input
        .get("code_branch")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            request
                .task_input
                .get("issue_ids")
                .and_then(Value::as_array)
                .and_then(|ids| ids.first())
                .and_then(Value::as_str)
                .map(|id| format!("code/{id}"))
        })
        .ok_or_else(|| {
            RunError::Graph("code test input has neither code_branch nor issue_ids".to_string())
        })?;
    if !valid_branch(&work_branch) {
        let evidence = evidence("代码验证", &format!("非法分支名 {work_branch}"));
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "test:failed").await?;
        return Ok(evidence);
    }
    let repo = format!("/work/{project_id}/repo");
    let prepared = prepare_checkout(shell, &repo, url, branch).await?;
    if prepared.exit_code != Some(0) {
        let evidence = evidence(
            "代码验证",
            &format!("无法准备代码仓库\n{}", shell_evidence(&prepared)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "test:failed").await?;
        return Ok(evidence);
    }
    let checkout = shell
        .exec(&format!(
            "git -C {} fetch --all --prune && git -C {} checkout {} && git -C {} reset --hard origin/{}",
            quote(&repo),
            quote(&repo),
            quote(&work_branch),
            quote(&repo),
            quote(&work_branch),
        ))
        .await
        .map_err(shell_error)?;
    if checkout.exit_code != Some(0) {
        let evidence = evidence(
            "代码验证",
            &format!("无法检出分支 {work_branch}\n{}", shell_evidence(&checkout)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "test:failed").await?;
        return Ok(evidence);
    }
    let head = shell
        .exec(&format!("git -C {} rev-parse HEAD", quote(&repo)))
        .await
        .ok()
        .filter(|output| output.exit_code == Some(0))
        .map(|output| output.stdout.trim().to_string())
        .unwrap_or_default();
    let (command, verified) = run_verification(shell, &repo).await?;
    let passed = verified.exit_code == Some(0);
    let evidence = evidence(
        "代码验证",
        &format!(
            "branch={}\nhead={}\ncommand={}\nexit_code={}\n{}",
            work_branch,
            head,
            command,
            verified.exit_code.unwrap_or(-1),
            shell_evidence(&verified),
        ),
    );
    comment(tools, context, task_id, &evidence).await?;
    send(
        tools,
        context,
        task_id,
        if passed { "test:passed" } else { "test:failed" },
    )
    .await?;
    Ok(evidence)
}

async fn code_summary_merger(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let stage = request.stage.as_deref().unwrap_or("");
    let merge_request = tool_json(
        tools,
        context,
        "merge_request",
        json!({ "action": "get_for_task", "task_id": task_id }),
    )
    .await;
    let Ok(merge_request) = merge_request else {
        let evidence = evidence("CodeSummary 合入", "找不到组任务的合并请求。");
        comment(tools, context, task_id, &evidence).await?;
        send(
            tools,
            context,
            task_id,
            if stage == "cleanup" {
                "cleanup:failed"
            } else {
                "merge:failed"
            },
        )
        .await?;
        return Ok(evidence);
    };
    let merge_request = merge_request.get("merge_request").unwrap_or(&merge_request);
    if stage == "cleanup" {
        let branch = merge_request
            .get("source_branch")
            .and_then(Value::as_str)
            .or_else(|| {
                request
                    .task_input
                    .get("source_branch")
                    .and_then(Value::as_str)
            })
            .unwrap_or("");
        if branch.is_empty() {
            let evidence = evidence("CodeSummary 清理", "合并请求没有 source_branch。");
            comment(tools, context, task_id, &evidence).await?;
            send(tools, context, task_id, "cleanup:failed").await?;
            return Ok(evidence);
        }
        let output = tools
            .call(
                "merge_request",
                json!({ "action": "delete_branch", "branch": branch }),
                context,
            )
            .await;
        let cleaned = !output.is_error;
        let evidence = evidence(
            "CodeSummary 清理",
            &format!("branch={branch}\n{}", output.content),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(
            tools,
            context,
            task_id,
            if cleaned {
                "cleanup:cleaned"
            } else {
                "cleanup:failed"
            },
        )
        .await?;
        return Ok(evidence);
    }
    let Some(merge_request_id) = merge_request.get("id").and_then(Value::as_str) else {
        let evidence = evidence("CodeSummary 合入", "合并请求缺少 id。");
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "merge:failed").await?;
        return Ok(evidence);
    };
    let merge_request_id = merge_request_id.to_string();
    let output = tools
        .call(
            "merge_request",
            json!({ "action": "merge", "merge_request_id": merge_request_id }),
            context,
        )
        .await;
    let (operation, evidence) = if output.is_error {
        let operation = if output.content.to_ascii_lowercase().contains("conflict") {
            "merge:conflict"
        } else {
            "merge:failed"
        };
        (operation, evidence("CodeSummary 合入", &output.content))
    } else {
        (
            "merge:merged",
            evidence("CodeSummary 合入", &format!("merged {merge_request_id}")),
        )
    };
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, operation).await?;
    Ok(evidence)
}

async fn design_consistency_integrator(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let stage = request.stage.as_deref().unwrap_or("");
    if stage == "integration_review" {
        let branch = format!("design-consistency/{task_id}");
        let created = tools
            .call(
                "merge_request",
                json!({
                    "action": "create_for_task",
                    "task_id": task_id,
                    "source_branch": branch,
                    "target_branch": "source",
                    "title": format!("Design consistency integration {task_id}"),
                    "description": "Aggregated design consistency changes for human review.",
                }),
                context,
            )
            .await;
        let (operation, evidence) = if created.is_error {
            (
                "integration_review:failed",
                evidence("设计一致性集成", &created.content),
            )
        } else {
            (
                "integration_review:submitted",
                evidence(
                    "设计一致性集成",
                    &format!("source_branch={branch}\ntarget_branch=source"),
                ),
            )
        };
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, operation).await?;
        return Ok(evidence);
    }
    if stage == "cleanup" {
        let branch = format!("design-consistency/{task_id}");
        let output = tools
            .call(
                "merge_request",
                json!({ "action": "delete_branch", "branch": branch }),
                context,
            )
            .await;
        let cleaned = !output.is_error;
        let evidence = evidence(
            "设计一致性清理",
            &format!("branch={branch}\n{}", output.content),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(
            tools,
            context,
            task_id,
            if cleaned {
                "cleanup:cleaned"
            } else {
                "cleanup:failed"
            },
        )
        .await?;
        return Ok(evidence);
    }
    let merge_request = match tool_json(
        tools,
        context,
        "merge_request",
        json!({ "action": "get_for_task", "task_id": task_id }),
    )
    .await
    {
        Ok(merge_request) => merge_request,
        Err(_) => {
            let evidence = evidence("设计一致性合入", "找不到集成合并请求。");
            comment(tools, context, task_id, &evidence).await?;
            send(tools, context, task_id, "merge:failed").await?;
            return Ok(evidence);
        }
    };
    let merge_request = merge_request.get("merge_request").unwrap_or(&merge_request);
    let Some(merge_request_id) = merge_request.get("id").and_then(Value::as_str) else {
        let evidence = evidence("设计一致性合入", "集成合并请求缺少 id。");
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "merge:failed").await?;
        return Ok(evidence);
    };
    let merge_request_id = merge_request_id.to_string();
    let output = tools
        .call(
            "merge_request",
            json!({ "action": "merge", "merge_request_id": merge_request_id }),
            context,
        )
        .await;
    let (operation, evidence) = if output.is_error {
        let operation = if output.content.to_ascii_lowercase().contains("conflict") {
            "merge:conflict"
        } else {
            "merge:failed"
        };
        (operation, evidence("设计一致性合入", &output.content))
    } else {
        (
            "merge:merge",
            evidence("设计一致性合入", &format!("merged {merge_request_id}")),
        )
    };
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, operation).await?;
    Ok(evidence)
}

async fn design_cleanup(
    mode: DeterministicMode,
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
) -> Result<String, RunError> {
    let prefix = match mode {
        DeterministicMode::DesignCleanupDesign => "design",
        DeterministicMode::DesignCleanupDesignChange => "design-change",
        DeterministicMode::DesignCleanupDesignCorrection => "design-correction",
        DeterministicMode::DesignCleanupDesignBaseline => "design-baseline",
        _ => return Err(RunError::Graph("not a design cleanup mode".to_string())),
    };
    let branch = format!("{prefix}/{task_id}");
    let output = tools
        .call(
            "merge_request",
            json!({ "action": "delete_branch", "branch": branch }),
            context,
        )
        .await;
    let cleaned = !output.is_error;
    let evidence = evidence(
        "设计分支清理",
        &format!("branch={branch}\n{}", output.content),
    );
    comment(tools, context, task_id, &evidence).await?;
    send(
        tools,
        context,
        task_id,
        if cleaned { "cleanup:cleaned" } else { "failed" },
    )
    .await?;
    Ok(evidence)
}

async fn design_consistency_scanner(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    _request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let project = project_info(tools, context).await?;
    let project_id = context
        .project_id
        .as_deref()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .ok_or_else(|| RunError::Graph("consistency scan requires a project id".to_string()))?;
    let git_branch = project
        .get("git_branch")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (arche, prepared) = ensure_arche(shell, &project, project_id).await?;
    if prepared.exit_code != Some(0) {
        let evidence = evidence(
            "设计一致性扫描",
            &format!("无法准备设计空间\n{}", shell_evidence(&prepared)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "dispatch:failed").await?;
        return Ok(evidence);
    }
    let changed = shell
        .exec(&format!(
            "cd {} && arche source fetch && arche source checkout {} && arche source changes",
            quote(&arche),
            quote(git_branch)
        ))
        .await
        .map_err(shell_error)?;
    if changed.exit_code != Some(0) {
        let evidence = evidence(
            "设计一致性扫描",
            &format!("无法读取源码变化\n{}", shell_evidence(&changed)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "dispatch:failed").await?;
        return Ok(evidence);
    }
    let files = changed_paths(&changed.stdout);
    if files.is_empty() {
        comment(tools, context, task_id, "源码没有需要同步的变化。").await?;
        send(tools, context, task_id, "dispatch:no_changes").await?;
        return Ok("源码没有需要同步的变化。".to_string());
    }
    let target_branch = format!("design-consistency/{task_id}");
    let sync = shell
        .exec(&format!(
            "cd {arche} && (arche branch create {branch} >/dev/null 2>&1 || true) && arche checkout {branch} && arche source sync && arche add -A && arche commit -m 'Design consistency base sync' --author design-consistency --allow-empty && arche push",
            arche = quote(&arche),
            branch = quote(&target_branch),
        ))
        .await
        .map_err(shell_error)?;
    if sync.exit_code != Some(0) {
        let evidence = evidence(
            "设计一致性扫描",
            &format!("基础同步失败\n{}", shell_evidence(&sync)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "dispatch:failed").await?;
        return Ok(evidence);
    }
    let source_commit = shell
        .exec(&format!(
            "worktree=$(cd {} && arche source worktree) && git -C \"$worktree\" rev-parse HEAD",
            quote(&arche)
        ))
        .await
        .ok()
        .filter(|output| output.exit_code == Some(0))
        .map(|output| output.stdout.trim().to_string())
        .unwrap_or_default();
    let groups = group_changed_files(&files, 50);
    let tasks = groups
        .iter()
        .enumerate()
        .map(|(index, files)| {
            let directories = directories_for(files);
            json!({
                "workflow": "code_summary_group",
                "title": format!("Design summary group {}", index + 1),
                "description": format!(
                    "处理 {} 个文件的设计总结：把本组文件对应的设计对象和设计字段更新到与源码一致。\n目标集成分支：{}。\n源码 commit：{}。",
                    files.len(),
                    target_branch,
                    source_commit,
                ),
                "input": {
                    "directories": directories,
                    "files": files,
                    "target_branch": target_branch,
                    "source_git_commit": source_commit,
                }
            })
        })
        .collect::<Vec<_>>();
    let spawned = tool_json(
        tools,
        context,
        "task_spawn_children",
        json!({
            "task_id": task_id,
            "sequential": true,
            "tasks": tasks,
        }),
    )
    .await?;
    let count = spawned.as_array().map(Vec::len).unwrap_or(0);
    if count == 0 {
        let evidence = evidence("设计一致性扫描", "没有创建任何设计总结子任务。");
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "dispatch:failed").await?;
        return Ok(evidence);
    }
    let evidence = format!(
        "源码变化 {} 个文件，已分 {} 组并创建子任务，集成分支 {target_branch}，源码 commit {source_commit}。",
        files.len(),
        count
    );
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "dispatch:groups_created").await?;
    Ok(evidence)
}

fn changed_paths(output: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in output.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let Some(code) = fields.first() else {
            continue;
        };
        match code.chars().next() {
            Some('D') => continue,
            Some('R') => {
                if let Some(path) = fields.last() {
                    paths.push((*path).to_string());
                }
            }
            Some('A' | 'M') => {
                if let Some(path) = fields.get(1) {
                    paths.push((*path).to_string());
                }
            }
            _ => {}
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn group_changed_files(files: &[String], size: usize) -> Vec<Vec<String>> {
    if files.is_empty() || size == 0 {
        return Vec::new();
    }
    files.chunks(size).map(|chunk| chunk.to_vec()).collect()
}

fn directories_for(files: &[String]) -> Vec<String> {
    let mut directories = std::collections::BTreeSet::new();
    for file in files {
        let mut current = file.as_str();
        while let Some((parent, _)) = current.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            directories.insert(parent.to_string());
            current = parent;
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| (path.matches('/').count(), path.clone()));
    directories
}

async fn design_dispatcher(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let waiting = request.stage.as_deref() == Some("waiting");
    let pooled = tool_json(tools, context, "design_pool_batch", json!({})).await?;
    let selected = select_pool_issues(&pooled, DESIGN_BATCH_LIMIT);
    if selected.is_empty() {
        comment(tools, context, task_id, "设计池没有可立即开始的任务。").await?;
        if !waiting {
            send(tools, context, task_id, "dispatch:nothing").await?;
        }
        return Ok("设计池没有可立即开始的任务。".to_string());
    }
    if waiting {
        comment(
            tools,
            context,
            task_id,
            "设计池有新的可开始任务，重新分派。",
        )
        .await?;
        send(tools, context, task_id, "waiting:work_arrived").await?;
        return Ok("设计池有新的可开始任务。".to_string());
    }
    let project = project_info(tools, context).await?;
    let project_id = context
        .project_id
        .as_deref()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .ok_or_else(|| RunError::Graph("design dispatch requires a project id".to_string()))?;
    let (arche, prepared) = ensure_arche(shell, &project, project_id).await?;
    if prepared.exit_code != Some(0) {
        let evidence = evidence(
            "设计分派",
            &format!("无法准备设计空间\n{}", shell_evidence(&prepared)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "failed").await?;
        return Ok(evidence);
    }
    let has_design = design_has_objects(shell, &arche).await?;
    let round_title = format!("设计迭代 {}", &task_id.to_string()[..8]);
    let round_description = format!(
        "本轮设计目标：处理 {} 个设计池 Issue。\n\n{}",
        selected.len(),
        selected
            .iter()
            .map(|issue| format!(
                "- {} ({})",
                issue_title(issue),
                issue_id(issue).unwrap_or_default()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let round = tool_json(
        tools,
        context,
        "issue_create",
        json!({
            "title": round_title,
            "description": round_description,
            "kind": "task_request",
            "labels": ["design-iteration-round"],
        }),
    )
    .await?;
    let Some(round_id) = issue_id(&round) else {
        let evidence = "无法创建设计轮次 Issue。";
        comment(tools, context, task_id, evidence).await?;
        send(tools, context, task_id, "failed").await?;
        return Ok(evidence.to_string());
    };
    transition_issue(tools, context, &round_id, "in_progress", None).await?;
    tool_json(
        tools,
        context,
        "task_add_link",
        json!({
            "task_id": task_id,
            "kind": "issue",
            "target_id": round_id,
            "title": round_title,
            "relation": "references",
            "active": true,
            "metadata": { "role": "design_iteration_round" }
        }),
    )
    .await?;

    let workflow = if has_design {
        "design"
    } else {
        "design_baseline"
    };
    let tasks = selected
        .iter()
        .map(|issue| {
            let id = issue_id(issue).unwrap_or_default();
            let title = issue_title(issue);
            let description = issue_description(issue);
            json!({
                "workflow": workflow,
                "title": format!("Design: {title}"),
                "description": description,
                "input": {
                    "issue_ids": [id],
                    "requirements": description,
                }
            })
        })
        .collect::<Vec<_>>();
    let spawned = tool_json(
        tools,
        context,
        "task_spawn_children",
        json!({
            "task_id": task_id,
            "sequential": false,
            "tasks": tasks,
        }),
    )
    .await?;
    let spawned_count = spawned.as_array().map(Vec::len).unwrap_or(0);
    if spawned_count == 0 {
        let evidence = "设计任务创建失败：没有返回子任务。";
        comment(tools, context, task_id, evidence).await?;
        send(tools, context, task_id, "failed").await?;
        return Ok(evidence.to_string());
    }
    for issue in &selected {
        if let Some(id) = issue_id(issue) {
            transition_issue(tools, context, &id, "in_progress", None).await?;
        }
    }
    let evidence = format!("已创建 {spawned_count} 个 {workflow} 子任务，轮次 Issue {round_id}。");
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "dispatch:dispatched").await?;
    Ok(evidence)
}

async fn design_verifier(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let children = children(tools, context, task_id).await?;
    let designed = children
        .iter()
        .filter(|child| {
            matches!(
                child.get("workflow").and_then(Value::as_str),
                Some("design" | "design_baseline")
            )
        })
        .collect::<Vec<_>>();
    let completed = !designed.is_empty()
        && designed.iter().all(|child| {
            child.get("state").and_then(Value::as_str) == Some("completed")
                && child.get("result").and_then(Value::as_str) == Some("completed")
        });

    let project = project_info(tools, context).await?;
    let project_id = context
        .project_id
        .as_deref()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .ok_or_else(|| RunError::Graph("design verify requires a project id".to_string()))?;
    let (arche, prepared) = ensure_arche(shell, &project, project_id).await?;
    let checks = if prepared.exit_code == Some(0) {
        shell
            .exec(&format!(
                "cd {} && arche structure check && arche doc check && arche check",
                quote(&arche)
            ))
            .await
            .map_err(shell_error)?
    } else {
        prepared
    };
    let checks_passed = checks.exit_code == Some(0);

    let mut issues = issue_ids(request);
    for child in &designed {
        if let Some(ids) = child
            .get("input")
            .and_then(|input| input.get("issue_ids"))
            .and_then(Value::as_array)
        {
            for id in ids.iter().filter_map(Value::as_str) {
                if !issues.contains(&id.to_string()) {
                    issues.push(id.to_string());
                }
            }
        }
    }
    let mut actionable = Vec::new();
    for issue_id in issues {
        if issue_is_actionable(tools, context, &issue_id).await? {
            actionable.push(issue_id);
        }
    }
    let round_id = design_round_issue(tools, context, task_id).await?;
    if completed && checks_passed {
        for issue_id in &actionable {
            handoff_issue(
                tools,
                context,
                issue_id,
                "development",
                "Development",
                "根据设计空间 source 分支中已合入的设计实现需求。读取来源 Issue、需求文档和设计空间，完成代码、测试和合入。",
            )
            .await?;
            transition_issue(
                tools,
                context,
                issue_id,
                "resolved",
                Some("Design completed, reviewed, approved and merged."),
            )
            .await?;
            transition_issue(tools, context, issue_id, "closed", None).await?;
        }
        if let Some(round_id) = &round_id {
            close_issue(tools, context, round_id, "Design round completed.").await?;
        }
        let evidence = format!(
            "本轮 {} 个设计任务全部完成，arche 检查通过，{} 个 Issue 已关闭。",
            designed.len(),
            actionable.len()
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "verify:ready").await?;
        return Ok(evidence);
    }
    for issue_id in &actionable {
        transition_issue_pool(
            tools,
            context,
            issue_id,
            "pooled",
            Some("Design round did not complete or arche checks failed."),
            Some("design"),
        )
        .await?;
    }
    if let Some(round_id) = &round_id {
        close_issue(
            tools,
            context,
            round_id,
            "Design round blocked; issues returned to the design pool.",
        )
        .await?;
    }
    let evidence = evidence(
        "设计验收",
        &format!(
            "completed={completed}\nchecks_passed={checks_passed}\nissues_returned={}\n{}",
            actionable.len(),
            shell_evidence(&checks),
        ),
    );
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "verify:blocked").await?;
    Ok(evidence)
}

async fn ensure_arche(
    shell: &Arc<dyn Shell>,
    project: &Value,
    project_id: &str,
) -> Result<(String, ShellOutput), RunError> {
    let url = project
        .get("design_space_url")
        .and_then(Value::as_str)
        .unwrap_or("");
    let branch = project
        .get("design_space_branch")
        .and_then(Value::as_str)
        .filter(|branch| !branch.is_empty())
        .unwrap_or("source");
    let arche = format!("/work/{project_id}/arche");
    let command = format!(
        "mkdir -p $(dirname {dir}) && if [ -d {dir}/.arche ]; then cd {dir} && arche checkout {branch} && arche pull; else arche clone {url} {dir} --branch {branch}; fi",
        dir = quote(&arche),
        branch = quote(branch),
        url = quote(url),
    );
    let output = shell.exec(&command).await.map_err(shell_error)?;
    Ok((arche, output))
}

async fn design_has_objects(shell: &Arc<dyn Shell>, arche: &str) -> Result<bool, RunError> {
    let output = shell
        .exec(&format!("cd {} && arche search --has-design", quote(arche)))
        .await
        .map_err(shell_error)?;
    Ok(output.exit_code == Some(0) && !output.stdout.trim().is_empty())
}

async fn design_round_issue(
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
) -> Result<Option<String>, RunError> {
    let links = tool_json(
        tools,
        context,
        "task_links_list",
        json!({ "task_id": task_id }),
    )
    .await?;
    Ok(links.as_array().and_then(|links| {
        links
            .iter()
            .filter(|link| {
                link.get("metadata")
                    .and_then(|metadata| metadata.get("role"))
                    .and_then(Value::as_str)
                    == Some("design_iteration_round")
            })
            .max_by_key(|link| link.get("created_at").and_then(Value::as_str).unwrap_or(""))
            .and_then(|link| {
                link.get("target_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    }))
}

async fn close_issue(
    tools: &ToolRegistry,
    context: &ToolContext,
    issue_id: &str,
    resolution: &str,
) -> Result<(), RunError> {
    transition_issue(tools, context, issue_id, "resolved", Some(resolution)).await?;
    transition_issue(tools, context, issue_id, "closed", None).await
}

async fn code_merger(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let project = project_info(tools, context).await?;
    let project_id = context
        .project_id
        .as_deref()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .ok_or_else(|| RunError::Graph("code merge requires a project id".to_string()))?;
    let url = project
        .get("git_repository")
        .and_then(Value::as_str)
        .ok_or_else(|| RunError::Graph("project has no git_repository".to_string()))?;
    let target = project
        .get("git_branch")
        .and_then(Value::as_str)
        .unwrap_or("");
    let work_branch = branch_from_input(request)?;
    if !valid_branch(&work_branch) {
        return fail_merge(
            tools,
            context,
            task_id,
            &format!("非法分支名 {work_branch}"),
        )
        .await;
    }
    let repo = format!("/work/{project_id}/repo");
    let stage = request.stage.as_deref().unwrap_or("");
    let prepared = match prepare_checkout(shell, &repo, url, target).await {
        Ok(prepared) => prepared,
        Err(error) => {
            let evidence = evidence("代码合入", &format!("无法准备代码仓库：{error}"));
            comment(tools, context, task_id, &evidence).await?;
            send(
                tools,
                context,
                task_id,
                if stage == "cleanup" {
                    "failed"
                } else {
                    "merge:failed"
                },
            )
            .await?;
            return Ok(evidence);
        }
    };
    if prepared.exit_code != Some(0) {
        let evidence = evidence(
            "代码合入",
            &format!("无法准备代码仓库\n{}", shell_evidence(&prepared)),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(
            tools,
            context,
            task_id,
            if stage == "cleanup" {
                "failed"
            } else {
                "merge:failed"
            },
        )
        .await?;
        return Ok(evidence);
    }
    if stage == "cleanup" {
        let output = shell
            .exec(&format!(
                "git -C {} push origin --delete {}",
                quote(&repo),
                quote(&work_branch)
            ))
            .await
            .map_err(shell_error)?;
        let already_gone = output.stderr.contains("remote ref does not exist")
            || output.stderr.contains("remote branch");
        let cleaned = output.exit_code == Some(0) || already_gone;
        let evidence = evidence(
            "代码清理",
            &format!(
                "branch={work_branch}\nexit_code={}\n{}",
                output.exit_code.unwrap_or(-1),
                shell_evidence(&output),
            ),
        );
        comment(tools, context, task_id, &evidence).await?;
        send(
            tools,
            context,
            task_id,
            if cleaned { "cleanup:cleaned" } else { "failed" },
        )
        .await?;
        return Ok(evidence);
    }

    let task = tool_json(tools, context, "task_get", json!({ "task_id": task_id })).await?;
    let Some(recorded_head) = recorded_head(&task) else {
        return fail_merge(
            tools,
            context,
            task_id,
            "验证阶段没有记录 head，无法确认分支未被修改",
        )
        .await;
    };
    let fetched = shell
        .exec(&format!("git -C {} fetch --all --prune", quote(&repo)))
        .await
        .map_err(shell_error)?;
    if fetched.exit_code != Some(0) {
        return fail_merge(
            tools,
            context,
            task_id,
            &format!("无法 fetch\n{}", shell_evidence(&fetched)),
        )
        .await;
    }
    let actual_head = shell
        .exec(&format!(
            "git -C {} rev-parse origin/{}",
            quote(&repo),
            quote(&work_branch)
        ))
        .await
        .ok()
        .filter(|output| output.exit_code == Some(0))
        .map(|output| output.stdout.trim().to_string())
        .unwrap_or_default();
    if actual_head != recorded_head {
        return fail_merge(
            tools,
            context,
            task_id,
            &format!(
                "分支 {} 的 head 已变化：验证时 {recorded_head}，当前 {actual_head}",
                work_branch
            ),
        )
        .await;
    }
    let merged = shell
        .exec(&format!(
            "git -C {repo} checkout {target} && git -C {repo} pull --ff-only && git -C {repo} merge --ff-only origin/{branch} && git -C {repo} push origin {target}",
            repo = quote(&repo),
            target = quote(target),
            branch = quote(&work_branch),
        ))
        .await
        .map_err(shell_error)?;
    if merged.exit_code != Some(0) {
        return fail_merge(
            tools,
            context,
            task_id,
            &format!("合入失败\n{}", shell_evidence(&merged)),
        )
        .await;
    }
    let resolution =
        format!("Merged {work_branch} at {recorded_head} into {target} after verification.");
    for issue_id in issue_ids(request) {
        transition_issue(tools, context, &issue_id, "resolved", Some(&resolution)).await?;
    }
    let evidence = evidence(
        "代码合入",
        &format!(
            "branch={work_branch}\nhead={recorded_head}\ntarget={target}\n{}",
            shell_evidence(&merged),
        ),
    );
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "merge:merged").await?;
    Ok(evidence)
}

async fn fail_merge(
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
    reason: &str,
) -> Result<String, RunError> {
    let evidence = evidence("代码合入", reason);
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "merge:failed").await?;
    Ok(evidence)
}

async fn development_dispatcher(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let waiting = request.stage.as_deref() == Some("waiting");
    let pooled = tool_json(tools, context, "development_pool_batch", json!({})).await?;
    let selected = select_pool_issues(&pooled, DEVELOPMENT_BATCH_LIMIT);
    if selected.is_empty() {
        comment(tools, context, task_id, "开发池没有可立即开始的任务。").await?;
        if !waiting {
            send(tools, context, task_id, "dispatch:nothing").await?;
        }
        return Ok("开发池没有可立即开始的任务。".to_string());
    }
    if waiting {
        comment(
            tools,
            context,
            task_id,
            "开发池有新的可开始任务，重新分派。",
        )
        .await?;
        send(tools, context, task_id, "waiting:work_arrived").await?;
        return Ok("开发池有新的可开始任务。".to_string());
    }
    let tasks = selected
        .iter()
        .map(|issue| {
            let id = issue_id(issue).unwrap_or_default();
            let title = issue_title(issue);
            let description = issue_description(issue);
            json!({
                "workflow": "code_change",
                "title": format!("Implement: {title}"),
                "description": description,
                "input": {
                    "issue_ids": [id],
                    "requirements": description,
                }
            })
        })
        .collect::<Vec<_>>();
    let spawned = tool_json(
        tools,
        context,
        "task_spawn_children",
        json!({
            "task_id": task_id,
            "sequential": false,
            "tasks": tasks,
        }),
    )
    .await?;
    let spawned_count = spawned.as_array().map(Vec::len).unwrap_or(0);
    if spawned_count == 0 {
        let evidence = "开发任务创建失败：没有返回子任务。";
        comment(tools, context, task_id, evidence).await?;
        send(tools, context, task_id, "failed").await?;
        return Ok(evidence.to_string());
    }
    for issue in &selected {
        if let Some(id) = issue_id(issue) {
            transition_issue(tools, context, &id, "in_progress", None).await?;
        }
    }
    let evidence = format!("已创建 {spawned_count} 个 code_change 子任务。");
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "dispatch:dispatched").await?;
    Ok(evidence)
}

async fn development_verifier(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let children = children(tools, context, task_id).await?;
    let changes = children
        .iter()
        .filter(|child| child.get("workflow").and_then(Value::as_str) == Some("code_change"))
        .collect::<Vec<_>>();
    let verified = !changes.is_empty()
        && changes.iter().all(|child| {
            child.get("state").and_then(Value::as_str) == Some("completed")
                && child.get("result").and_then(Value::as_str) == Some("completed")
        });
    let mut issues = issue_ids(request);
    for child in &changes {
        if let Some(ids) = child
            .get("input")
            .and_then(|input| input.get("issue_ids"))
            .and_then(Value::as_array)
        {
            for id in ids.iter().filter_map(Value::as_str) {
                if !issues.contains(&id.to_string()) {
                    issues.push(id.to_string());
                }
            }
        }
    }
    let mut actionable = Vec::new();
    for issue_id in issues {
        if issue_is_actionable(tools, context, &issue_id).await? {
            actionable.push(issue_id);
        }
    }
    let issues = actionable;
    if verified {
        for issue_id in &issues {
            handoff_issue(
                tools,
                context,
                issue_id,
                "tests",
                "Test",
                "对已合入的代码和构建产物执行最终产物验收测试。读取来源 Issue、设计空间、代码提交和验收条件，提交测试报告与证据。",
            )
            .await?;
            transition_issue(
                tools,
                context,
                issue_id,
                "resolved",
                Some("Implemented, verified and merged."),
            )
            .await?;
            transition_issue(tools, context, issue_id, "closed", None).await?;
        }
        let evidence = format!(
            "本轮 {} 个 code_change 子任务全部完成，{} 个 Issue 已关闭。",
            changes.len(),
            issues.len()
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "verify:ready").await?;
        return Ok(evidence);
    }
    for issue_id in &issues {
        transition_issue_pool(
            tools,
            context,
            issue_id,
            "pooled",
            Some("Development round did not finish with completed code changes."),
            Some("development"),
        )
        .await?;
    }
    let evidence = format!(
        "本轮 {} 个 code_change 子任务中存在未完成或未通过项，相关 Issue 已退回开发池。",
        changes.len()
    );
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "verify:blocked").await?;
    Ok(evidence)
}

async fn requirements_dispatcher(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    batch_dispatcher(
        tools,
        context,
        task_id,
        request.stage.as_deref() == Some("waiting"),
        "requirements_pool_batch",
        "requirements_analysis",
        "需求分析",
    )
    .await
}

async fn test_dispatcher(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    batch_dispatcher(
        tools,
        context,
        task_id,
        request.stage.as_deref() == Some("waiting"),
        "tests_pool_batch",
        "test_execution",
        "测试执行",
    )
    .await
}

async fn batch_dispatcher(
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
    waiting: bool,
    pool_tool: &str,
    child_workflow: &str,
    label: &str,
) -> Result<String, RunError> {
    let pooled = tool_json(tools, context, pool_tool, json!({})).await?;
    let selected = select_pool_issues(&pooled, 5);
    if selected.is_empty() {
        comment(
            tools,
            context,
            task_id,
            &format!("{label}池没有可立即开始的任务。"),
        )
        .await?;
        if !waiting {
            send(tools, context, task_id, "dispatch:nothing").await?;
        }
        return Ok(format!("{label}池没有可立即开始的任务。"));
    }
    if waiting {
        comment(
            tools,
            context,
            task_id,
            &format!("{label}池有新的可开始任务，重新分派。"),
        )
        .await?;
        send(tools, context, task_id, "waiting:work_arrived").await?;
        return Ok(format!("{label}池有新的可开始任务。"));
    }
    let issue_ids = selected.iter().filter_map(issue_id).collect::<Vec<_>>();
    let requirements = selected
        .iter()
        .map(|issue| format!("- {}: {}", issue_title(issue), issue_description(issue)))
        .collect::<Vec<_>>()
        .join("\n");
    let task = tool_json(
        tools,
        context,
        "task_spawn_children",
        json!({
            "task_id": task_id,
            "sequential": false,
            "tasks": [{
                "workflow": child_workflow,
                "title": format!("{label}: {}", selected.len()),
                "description": requirements,
                "input": {
                    "issue_ids": issue_ids,
                    "requirements": requirements,
                }
            }]
        }),
    )
    .await?;
    if task.as_array().map(Vec::len).unwrap_or(0) == 0 {
        let evidence = format!("{label}任务创建失败：没有返回子任务。");
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "failed").await?;
        return Ok(evidence);
    }
    for issue in &selected {
        if let Some(id) = issue_id(issue) {
            transition_issue(tools, context, &id, "in_progress", None).await?;
        }
    }
    let evidence = format!(
        "已创建 1 个 {child_workflow} 子任务，处理 {} 个{label}议题。",
        selected.len()
    );
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "dispatch:dispatched").await?;
    Ok(evidence)
}

async fn requirements_verifier(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    batch_verifier(
        tools,
        context,
        request,
        task_id,
        "requirements_analysis",
        "requirements",
        "需求",
        Some("design"),
        "Design",
        "根据需求文档和来源 Issue 进行设计。读取设计空间 source 分支、需求文档和项目说明书，更新设计对象并提交设计 MR。",
        false,
    )
    .await
}

async fn test_verifier(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    batch_verifier(
        tools,
        context,
        request,
        task_id,
        "test_execution",
        "tests",
        "测试",
        None,
        "",
        "",
        true,
    )
    .await
}

async fn batch_verifier(
    tools: &ToolRegistry,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
    child_workflow: &str,
    pool: &str,
    label: &str,
    next_pool: Option<&str>,
    next_title_prefix: &str,
    next_description: &str,
    release: bool,
) -> Result<String, RunError> {
    let children = children(tools, context, task_id).await?;
    let tasks = children
        .iter()
        .filter(|child| child.get("workflow").and_then(Value::as_str) == Some(child_workflow))
        .collect::<Vec<_>>();
    let verified = !tasks.is_empty()
        && tasks.iter().all(|child| {
            child.get("state").and_then(Value::as_str) == Some("completed")
                && matches!(
                    child.get("result").and_then(Value::as_str),
                    Some("completed" | "approved")
                )
        });
    let mut issues = issue_ids(request);
    for child in &tasks {
        if let Some(ids) = child
            .get("input")
            .and_then(|input| input.get("issue_ids"))
            .and_then(Value::as_array)
        {
            for id in ids.iter().filter_map(Value::as_str) {
                if !issues.contains(&id.to_string()) {
                    issues.push(id.to_string());
                }
            }
        }
    }
    let mut actionable = Vec::new();
    for issue_id in issues {
        if issue_is_actionable(tools, context, &issue_id).await? {
            actionable.push(issue_id);
        }
    }
    if verified {
        if let Some(next_pool) = next_pool {
            for issue_id in &actionable {
                handoff_issue(
                    tools,
                    context,
                    issue_id,
                    next_pool,
                    next_title_prefix,
                    next_description,
                )
                .await?;
            }
        }
        if release && !tasks.is_empty() {
            handoff_release(tools, context, task_id, &actionable).await?;
        }
        for issue_id in &actionable {
            transition_issue(
                tools,
                context,
                issue_id,
                "resolved",
                Some(&format!("{label}迭代已完成并合入。")),
            )
            .await?;
            transition_issue(tools, context, issue_id, "closed", None).await?;
        }
        let evidence = format!(
            "本轮 {} 个{label}子任务全部完成，{} 个 Issue 已关闭。",
            tasks.len(),
            actionable.len()
        );
        comment(tools, context, task_id, &evidence).await?;
        send(tools, context, task_id, "verify:ready").await?;
        return Ok(evidence);
    }
    for issue_id in &actionable {
        transition_issue_pool(
            tools,
            context,
            issue_id,
            "pooled",
            Some(&format!("{label}迭代未完成。")),
            Some(pool),
        )
        .await?;
    }
    let evidence = format!(
        "本轮 {} 个{label}子任务存在未完成或未通过项，相关 Issue 已退回{label}池。",
        tasks.len()
    );
    comment(tools, context, task_id, &evidence).await?;
    send(tools, context, task_id, "verify:blocked").await?;
    Ok(evidence)
}

async fn children(
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
) -> Result<Vec<Value>, RunError> {
    let value = tool_json(
        tools,
        context,
        "task_children",
        json!({ "task_id": task_id }),
    )
    .await?;
    Ok(value.as_array().cloned().unwrap_or_default())
}

fn select_pool_issues(issues: &Value, limit: usize) -> Vec<Value> {
    issues
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|issue| {
                    issue.get("status").and_then(Value::as_str) == Some("pooled")
                        && issue.get("duplicate_of").is_none_or(Value::is_null)
                })
                .take(limit)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn issue_id(issue: &Value) -> Option<String> {
    issue.get("id").and_then(Value::as_str).map(str::to_string)
}

fn issue_title(issue: &Value) -> String {
    issue
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Untitled issue")
        .to_string()
}

fn issue_description(issue: &Value) -> String {
    issue
        .get("description")
        .or_else(|| issue.get("body"))
        .and_then(Value::as_str)
        .filter(|description| !description.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| issue_title(issue))
}

fn issue_ids(request: &AgentRunRequest) -> Vec<String> {
    request
        .task_input
        .get("issue_ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn branch_from_input(request: &AgentRunRequest) -> Result<String, RunError> {
    if let Some(branch) = request
        .task_input
        .get("code_branch")
        .and_then(Value::as_str)
    {
        return Ok(branch.to_string());
    }
    issue_ids(request)
        .first()
        .map(|id| format!("code/{id}"))
        .ok_or_else(|| {
            RunError::Graph("code_change input has neither code_branch nor issue_ids".to_string())
        })
}

fn recorded_head(task: &Value) -> Option<String> {
    let comments = task.get("comments")?.as_array()?;
    comments.iter().rev().find_map(|comment| {
        comment
            .get("body")
            .and_then(Value::as_str)?
            .lines()
            .find_map(|line| line.trim().strip_prefix("head="))
            .map(str::trim)
            .filter(|head| head.len() >= 7)
            .map(str::to_string)
    })
}

async fn transition_issue(
    tools: &ToolRegistry,
    context: &ToolContext,
    issue_id: &str,
    to: &str,
    resolution: Option<&str>,
) -> Result<(), RunError> {
    transition_issue_pool(tools, context, issue_id, to, resolution, None).await
}

async fn transition_issue_pool(
    tools: &ToolRegistry,
    context: &ToolContext,
    issue_id: &str,
    to: &str,
    resolution: Option<&str>,
    pool: Option<&str>,
) -> Result<(), RunError> {
    tool_json(
        tools,
        context,
        "issue_transition",
        json!({
            "issue_id": issue_id,
            "to": to,
            "resolution": resolution,
            "pool": if to == "pooled" { pool } else { None },
        }),
    )
    .await
    .map(|_| ())
}

async fn issue_is_actionable(
    tools: &ToolRegistry,
    context: &ToolContext,
    issue_id: &str,
) -> Result<bool, RunError> {
    let issue = tool_json(tools, context, "issue_get", json!({ "issue_id": issue_id })).await?;
    Ok(!matches!(
        issue.get("status").and_then(Value::as_str),
        Some("closed" | "resolved" | "rejected" | "duplicate")
    ))
}

async fn handoff_issue(
    tools: &ToolRegistry,
    context: &ToolContext,
    source_issue_id: &str,
    next_pool: &str,
    title_prefix: &str,
    description: &str,
) -> Result<(), RunError> {
    let label = format!("handoff:{next_pool}:{source_issue_id}");
    let issues = tool_json(tools, context, "issue_list", json!({})).await?;
    if has_active_handoff(&issues, &label) {
        return Ok(());
    }
    let source = tool_json(
        tools,
        context,
        "issue_get",
        json!({ "issue_id": source_issue_id }),
    )
    .await?;
    let title = issue_title(&source);
    tool_json(
        tools,
        context,
        "issue_create",
        json!({
            "kind": "task_request",
            "title": format!("{title_prefix}: {title}"),
            "description": format!("{description}\n\n来源 Issue: {source_issue_id}"),
            "labels": ["handoff", label],
            "pool": next_pool,
        }),
    )
    .await?;
    Ok(())
}

fn has_active_handoff(issues: &Value, label: &str) -> bool {
    issues.as_array().is_some_and(|issues| {
        issues.iter().any(|issue| {
            let labeled = issue
                .get("labels")
                .and_then(Value::as_array)
                .is_some_and(|labels| {
                    labels
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|value| value == label)
                });
            let active = !matches!(
                issue.get("status").and_then(Value::as_str),
                Some("closed" | "rejected" | "duplicate")
            );
            labeled && active
        })
    })
}

async fn handoff_release(
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
    issue_ids: &[String],
) -> Result<(), RunError> {
    let existing = children(tools, context, task_id).await?;
    if existing.iter().any(|child| {
        child.get("workflow").and_then(Value::as_str) == Some("release")
            && !matches!(
                child.get("state").and_then(Value::as_str),
                Some("completed" | "failed")
            )
    }) {
        return Ok(());
    }
    let project = project_info(tools, context).await?;
    let name = project
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("project");
    tool_json(
        tools,
        context,
        "task_spawn_children",
        json!({
            "task_id": task_id,
            "sequential": false,
            "tasks": [{
                "workflow": "release",
                "title": format!("Release: {name}"),
                "description": format!(
                    "测试验收已通过，准备发布。\n\n测试 Issue: {}",
                    issue_ids.join(", ")
                ),
                "input": { "issue_ids": issue_ids },
            }]
        }),
    )
    .await?;
    Ok(())
}

async fn release_manager(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let repo = match prepare_release_repo(tools, shell, context, request).await {
        Ok(repo) => repo,
        Err(error) => {
            let evidence = evidence("发布候选", &format!("无法准备代码仓库：{error}"));
            comment(tools, context, task_id, &evidence).await?;
            send(tools, context, task_id, "failed").await?;
            return Ok(evidence);
        }
    };
    send(tools, context, task_id, "candidate:started").await?;
    let (command, output) = run_script_or(shell, &repo, "release.sh", build_fallback()).await?;
    let passed = output.exit_code == Some(0);
    let evidence = evidence(
        "发布候选",
        &format!(
            "command={command}\nexit_code={}\n{}",
            output.exit_code.unwrap_or(-1),
            shell_evidence(&output),
        ),
    );
    comment(tools, context, task_id, &evidence).await?;
    let operation = if passed {
        "candidate:completed"
    } else {
        "failed"
    };
    send(tools, context, task_id, operation).await?;
    Ok(evidence)
}

async fn release_verifier(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let repo = match prepare_release_repo(tools, shell, context, request).await {
        Ok(repo) => repo,
        Err(error) => {
            let evidence = evidence("发布验证", &format!("无法准备代码仓库：{error}"));
            comment(tools, context, task_id, &evidence).await?;
            send(tools, context, task_id, "verification:verification_failed").await?;
            return Ok(evidence);
        }
    };
    let (command, output) = run_script_or(shell, &repo, "verify.sh", test_fallback()).await?;
    let passed = output.exit_code == Some(0);
    let evidence = evidence(
        "发布验证",
        &format!(
            "command={command}\nexit_code={}\n{}",
            output.exit_code.unwrap_or(-1),
            shell_evidence(&output),
        ),
    );
    comment(tools, context, task_id, &evidence).await?;
    send(
        tools,
        context,
        task_id,
        if passed {
            "verification:verified"
        } else {
            "verification:verification_failed"
        },
    )
    .await?;
    Ok(evidence)
}

async fn release_deployer(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let repo = match prepare_release_repo(tools, shell, context, request).await {
        Ok(repo) => repo,
        Err(error) => {
            let evidence = evidence("发布部署", &format!("无法准备代码仓库：{error}"));
            comment(tools, context, task_id, &evidence).await?;
            send(tools, context, task_id, "deployment:deploy_failed").await?;
            return Ok(evidence);
        }
    };
    let command = format!(
        "cd {} && if [ -f scripts/deploy.sh ]; then bash scripts/deploy.sh; else echo 'scripts/deploy.sh is missing' >&2; exit 2; fi",
        quote(&repo)
    );
    let output = shell.exec(&command).await.map_err(shell_error)?;
    let passed = output.exit_code == Some(0);
    let evidence = evidence(
        "发布部署",
        &format!(
            "command={command}\nexit_code={}\n{}",
            output.exit_code.unwrap_or(-1),
            shell_evidence(&output),
        ),
    );
    comment(tools, context, task_id, &evidence).await?;
    send(
        tools,
        context,
        task_id,
        if passed {
            "deployment:deployed"
        } else {
            "deployment:deploy_failed"
        },
    )
    .await?;
    Ok(evidence)
}

async fn release_observer(
    tools: &ToolRegistry,
    shell: Option<&Arc<dyn Shell>>,
    context: &ToolContext,
    request: &AgentRunRequest,
    task_id: Uuid,
) -> Result<String, RunError> {
    let shell = required_shell(shell)?;
    let repo = match prepare_release_repo(tools, shell, context, request).await {
        Ok(repo) => repo,
        Err(error) => {
            let evidence = evidence("发布观察", &format!("无法准备代码仓库：{error}"));
            comment(tools, context, task_id, &evidence).await?;
            send(tools, context, task_id, "observation:unhealthy").await?;
            return Ok(evidence);
        }
    };
    let command = format!(
        "cd {} && if [ -f scripts/observe.sh ]; then bash scripts/observe.sh; elif [ -f scripts/smoke.sh ]; then bash scripts/smoke.sh; else echo 'scripts/observe.sh and scripts/smoke.sh are both missing' >&2; exit 2; fi",
        quote(&repo)
    );
    let output = shell.exec(&command).await.map_err(shell_error)?;
    let healthy = output.exit_code == Some(0);
    let evidence = evidence(
        "发布观察",
        &format!(
            "command={command}\nexit_code={}\n{}",
            output.exit_code.unwrap_or(-1),
            shell_evidence(&output),
        ),
    );
    comment(tools, context, task_id, &evidence).await?;
    send(
        tools,
        context,
        task_id,
        if healthy {
            "observation:healthy"
        } else {
            "observation:unhealthy"
        },
    )
    .await?;
    Ok(evidence)
}

async fn prepare_release_repo(
    tools: &ToolRegistry,
    shell: &Arc<dyn Shell>,
    context: &ToolContext,
    _request: &AgentRunRequest,
) -> Result<String, RunError> {
    let project = project_info(tools, context).await?;
    let project_id = context
        .project_id
        .as_deref()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .ok_or_else(|| RunError::Graph("release requires a project id".to_string()))?;
    let url = project
        .get("git_repository")
        .and_then(Value::as_str)
        .ok_or_else(|| RunError::Graph("project has no git_repository".to_string()))?;
    let branch = project
        .get("git_branch")
        .and_then(Value::as_str)
        .unwrap_or("");
    let repo = format!("/work/{project_id}/repo");
    let prepared = prepare_checkout(shell, &repo, url, branch).await?;
    if prepared.exit_code != Some(0) {
        return Err(RunError::Graph(format!(
            "cannot prepare release checkout: {}",
            shell_evidence(&prepared)
        )));
    }
    Ok(repo)
}

async fn prepare_checkout(
    shell: &Arc<dyn Shell>,
    dir: &str,
    url: &str,
    branch: &str,
) -> Result<ShellOutput, RunError> {
    let checkout = if branch.is_empty() {
        String::new()
    } else {
        format!("git checkout {} && git pull --ff-only", quote(branch))
    };
    let command = format!(
        "mkdir -p $(dirname {dir}) && if [ -d {dir}/.git ]; then cd {dir} && git remote set-url origin {url} && git fetch --all --prune && {checkout}; else git clone {branch_arg} {url} {dir}; fi",
        dir = quote(dir),
        url = quote(url),
        branch_arg = if branch.is_empty() {
            String::new()
        } else {
            format!("--branch {}", quote(branch))
        },
        checkout = if checkout.is_empty() {
            "git fetch --all --prune".to_string()
        } else {
            checkout
        },
    );
    shell.exec(&command).await.map_err(shell_error)
}

async fn run_verification(
    shell: &Arc<dyn Shell>,
    repo: &str,
) -> Result<(String, ShellOutput), RunError> {
    run_script_or(shell, repo, "verify.sh", test_fallback()).await
}

async fn run_script_or(
    shell: &Arc<dyn Shell>,
    repo: &str,
    script: &str,
    fallback: &str,
) -> Result<(String, ShellOutput), RunError> {
    let command = format!(
        "cd {} && if [ -f scripts/{} ]; then bash scripts/{}; else {}; fi",
        quote(repo),
        script,
        script,
        fallback
    );
    let output = shell.exec(&command).await.map_err(shell_error)?;
    Ok((command, output))
}

fn build_fallback() -> &'static str {
    "if [ -f Cargo.toml ]; then cargo build --workspace; \
     elif [ -f package.json ]; then npm run build --if-present; \
     elif [ -f go.mod ]; then go build ./...; \
     elif [ -f pyproject.toml ]; then python -m build; \
     else echo 'no supported build manifest' >&2; exit 2; fi"
}

fn test_fallback() -> &'static str {
    "if [ -f Cargo.toml ]; then cargo test --workspace --all-targets; \
     elif [ -f package.json ]; then npm test --if-present; \
     elif [ -f go.mod ]; then go test ./...; \
     elif [ -f pyproject.toml ]; then python -m pytest -q; \
     else echo 'no supported test manifest' >&2; exit 2; fi"
}

async fn project_info(tools: &ToolRegistry, context: &ToolContext) -> Result<Value, RunError> {
    tool_json(tools, context, "project", json!({ "action": "get" })).await
}

async fn tool_json(
    tools: &ToolRegistry,
    context: &ToolContext,
    name: &str,
    arguments: Value,
) -> Result<Value, RunError> {
    let output = tools.call(name, arguments, context).await;
    if output.is_error {
        return Err(RunError::Graph(format!(
            "{name} failed: {}",
            output.content
        )));
    }
    serde_json::from_str(&output.content)
        .map_err(|error| RunError::Graph(format!("{name} returned invalid JSON: {error}")))
}

async fn comment(
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
    body: &str,
) -> Result<(), RunError> {
    let output = tools
        .call(
            "task_comment",
            json!({ "task_id": task_id, "body": body }),
            context,
        )
        .await;
    if output.is_error {
        return Err(RunError::Graph(format!(
            "task_comment failed: {}",
            output.content
        )));
    }
    Ok(())
}

async fn send(
    tools: &ToolRegistry,
    context: &ToolContext,
    task_id: Uuid,
    operation: &str,
) -> Result<(), RunError> {
    let output = tools
        .call(
            "task_operation",
            json!({ "task_id": task_id, "operation": operation }),
            context,
        )
        .await;
    if output.is_error {
        return Err(RunError::Graph(format!(
            "task_operation {operation} failed: {}",
            output.content
        )));
    }
    Ok(())
}

fn parse_task_id(request: &AgentRunRequest) -> Result<Uuid, RunError> {
    request
        .task_id
        .parse()
        .map_err(|_| RunError::Graph(format!("invalid task id {}", request.task_id)))
}

fn required_shell(shell: Option<&Arc<dyn Shell>>) -> Result<&Arc<dyn Shell>, RunError> {
    shell.ok_or_else(|| RunError::Graph("deterministic runtime requires a shell".to_string()))
}

fn shell_error(error: crate::ToolError) -> RunError {
    RunError::Graph(error.to_string())
}

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && !branch.contains("..")
        && !branch.starts_with('/')
        && !branch.ends_with('/')
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_evidence(output: &ShellOutput) -> String {
    let text = format!(
        "stdout:\n{}\nstderr:\n{}",
        output.stdout.trim(),
        output.stderr.trim()
    );
    truncate(&text, MAX_EVIDENCE)
}

fn evidence(title: &str, body: &str) -> String {
    truncate(&format!("## {title}\n{body}"), MAX_EVIDENCE + 256)
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(limit).collect();
    truncated.push('…');
    truncated
}

fn has_code_files<'a>(paths: impl Iterator<Item = &'a str>) -> bool {
    paths.into_iter().any(|path| {
        let name = path.rsplit('/').next().unwrap_or(path);
        MANIFESTS.contains(&name)
            || path
                .rsplit_once('.')
                .map(|(_, extension)| {
                    SOURCE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
                })
                .unwrap_or(false)
    })
}

fn language_counts<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for path in paths {
        let Some((_, extension)) = path.rsplit_once('.') else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();
        if SOURCE_EXTENSIONS.contains(&extension.as_str()) {
            *counts.entry(extension).or_insert(0) += 1;
        }
    }
    counts
}

fn manifest_names<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut names = paths
        .into_iter()
        .filter_map(|path| path.rsplit('/').next())
        .filter(|name| MANIFESTS.contains(name))
        .map(str::to_string)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_an_active_handoff_and_allows_a_new_round_after_it_closes() {
        let label = "handoff:design:issue-1";
        let active = json!([{
            "status": "pooled",
            "labels": ["handoff", label]
        }]);
        assert!(has_active_handoff(&active, label));
        let closed = json!([{
            "status": "closed",
            "labels": ["handoff", label]
        }]);
        assert!(!has_active_handoff(&closed, label));
    }

    #[test]
    fn detects_source_files_and_ignores_documents() {
        assert!(has_code_files(
            ["src/main.rs", "README.md", "docs/design.md"].into_iter()
        ));
        assert!(has_code_files(["Cargo.toml"].into_iter()));
        assert!(!has_code_files(
            ["README.md", "docs/design.md", "images/logo.png"].into_iter()
        ));
    }

    #[test]
    fn branches_are_restricted_to_git_safe_characters() {
        assert!(valid_branch("code/issue-1"));
        assert!(valid_branch("release/2026.11"));
        assert!(!valid_branch("../main"));
        assert!(!valid_branch("/main"));
        assert!(!valid_branch("code;rm -rf /"));
        assert!(!valid_branch(""));
    }

    #[test]
    fn language_counts_only_include_known_source_extensions() {
        let counts =
            language_counts(["src/a.rs", "src/b.rs", "web/app.ts", "README.md"].into_iter());
        assert_eq!(counts.get("rs"), Some(&2));
        assert_eq!(counts.get("ts"), Some(&1));
        assert!(!counts.contains_key("md"));
    }

    #[test]
    fn pool_selection_skips_duplicates_and_caps_the_batch() {
        let pooled = json!([
            { "id": "1", "title": "one", "status": "pooled" },
            { "id": "2", "title": "duplicate", "status": "pooled", "duplicate_of": "1" },
            { "id": "3", "title": "three", "status": "pooled" },
            { "id": "4", "title": "four", "status": "pooled" },
            { "id": "5", "title": "five", "status": "pooled" },
            { "id": "6", "title": "six", "status": "pooled" },
            { "id": "7", "title": "seven", "status": "pooled" }
        ]);
        let selected = select_pool_issues(&pooled, 5);
        let ids = selected
            .iter()
            .filter_map(|issue| issue.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["1", "3", "4", "5", "6"]);
    }

    #[test]
    fn changed_paths_keep_add_modify_and_rename_targets() {
        let paths = changed_paths(
            "A\tsrc/new.rs\nM\tsrc/old.rs\nD\tsrc/gone.rs\nR\told/name.rs\tnew/name.rs\n",
        );
        assert_eq!(paths, vec!["new/name.rs", "src/new.rs", "src/old.rs"]);
    }

    #[test]
    fn changed_files_group_and_expose_parent_directories() {
        let files = vec![
            "src/a.rs".to_string(),
            "src/mod/b.rs".to_string(),
            "web/app.ts".to_string(),
        ];
        let groups = group_changed_files(&files, 2);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 2);
        let directories = directories_for(&files);
        assert_eq!(directories, vec!["src", "web", "src/mod"]);
    }

    #[test]
    fn hybrid_modes_only_claim_their_mechanical_stages() {
        assert!(handles_stage(
            DeterministicMode::DesignCleanupDesign,
            Some("cleanup")
        ));
        assert!(!handles_stage(
            DeterministicMode::DesignCleanupDesign,
            Some("proposal")
        ));
        assert!(handles_stage(
            DeterministicMode::DesignConsistencyIntegrator,
            Some("integration_review")
        ));
        assert!(handles_stage(
            DeterministicMode::DesignConsistencyIntegrator,
            Some("merge")
        ));
        assert!(!handles_stage(
            DeterministicMode::DesignConsistencyIntegrator,
            Some("integration_fix")
        ));
    }

    #[test]
    fn recorded_head_uses_the_latest_verification_comment() {
        let task = json!({
            "comments": [
                { "body": "## 代码验证\nbranch=code/1\nhead=aaaaaaa" },
                { "body": "human review looks good" },
                { "body": "## 代码验证\nbranch=code/1\nhead=bbbbbbb" }
            ]
        });
        assert_eq!(recorded_head(&task).as_deref(), Some("bbbbbbb"));
        assert_eq!(recorded_head(&json!({ "comments": [] })), None);
    }
}
