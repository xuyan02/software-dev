//! Read-only diagnostic for task/agent assignment and run state.
//!
//! Run with:
//! `cargo run -p foundry-adapter-postgres --example dbcheck`

use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = database_url()?;
    let pool = sqlx::PgPool::connect(&url).await?;

    println!("== tasks ==");
    let rows = sqlx::query(
        "select id, title, workflow, stage, state from tasks order by created_at",
    )
    .fetch_all(&pool)
    .await?;
    for row in &rows {
        let id: Uuid = row.try_get("id")?;
        let title: String = row.try_get("title")?;
        let workflow: String = row.try_get("workflow")?;
        let stage: Option<String> = row.try_get("stage")?;
        let state: String = row.try_get("state")?;
        println!("{id} state={state} stage={stage:?} workflow={workflow} title={title:?}");
    }

    println!("\n== task_agent_assignments ==");
    let rows = sqlx::query(
        "select a.task_id, a.role, a.agent_definition_id, d.name, d.enabled \
         from task_agent_assignments a \
         join agent_definitions d on d.id = a.agent_definition_id \
         order by a.task_id",
    )
    .fetch_all(&pool)
    .await?;
    for row in &rows {
        let task_id: Uuid = row.try_get("task_id")?;
        let role: String = row.try_get("role")?;
        let definition_id: Uuid = row.try_get("agent_definition_id")?;
        let name: String = row.try_get("name")?;
        let enabled: bool = row.try_get("enabled")?;
        println!("{task_id} {role} -> {name} ({definition_id}) enabled={enabled}");
    }

    println!("\n== task_actors ==");
    let rows = sqlx::query(
        "select task_id, role_key, account_id, active from task_actors order by task_id",
    )
    .fetch_all(&pool)
    .await?;
    for row in &rows {
        let task_id: Uuid = row.try_get("task_id")?;
        let role: String = row.try_get("role_key")?;
        let account_id: Uuid = row.try_get("account_id")?;
        let active: bool = row.try_get("active")?;
        println!("{task_id} {role} -> {account_id} active={active}");
    }

    println!("\n== task links (issue) ==");
    let rows = sqlx::query(
        "select task_id, kind, relation, target_id from task_links \
         where kind = 'issue' order by task_id",
    )
    .fetch_all(&pool)
    .await?;
    for row in &rows {
        let task_id: Uuid = row.try_get("task_id")?;
        let kind: String = row.try_get("kind")?;
        let relation: String = row.try_get("relation")?;
        let target_id: Option<Uuid> = row.try_get("target_id")?;
        println!("{task_id} {kind}/{relation} -> {target_id:?}");
    }

    println!("\n== issue comments ==");
    let rows = sqlx::query(
        "select issue_id, id, author_kind, author_id, created_at, body \
         from issue_comments order by created_at",
    )
    .fetch_all(&pool)
    .await?;
    for row in &rows {
        let issue_id: Uuid = row.try_get("issue_id")?;
        let id: Uuid = row.try_get("id")?;
        let author_kind: String = row.try_get("author_kind")?;
        let author_id: String = row.try_get("author_id")?;
        let created_at: OffsetDateTime = row.try_get("created_at")?;
        let body: String = row.try_get("body")?;
        let preview: String = body.chars().take(80).collect();
        println!("{issue_id} {id} {author_kind}/{author_id} {created_at}: {preview}");
    }

    println!("\n== IssueCommented events ==");
    let rows = sqlx::query(
        "select seq, actor, payload, created_at from events \
         where kind = 'issue_commented' order by seq desc limit 10",
    )
    .fetch_all(&pool)
    .await?;
    for row in &rows {
        let seq: i64 = row.try_get("seq")?;
        let actor: serde_json::Value = row.try_get("actor")?;
        let payload: serde_json::Value = row.try_get("payload")?;
        let created_at: OffsetDateTime = row.try_get("created_at")?;
        let issue_id = payload.get("id").and_then(serde_json::Value::as_str);
        let last_comment = payload
            .get("comments")
            .and_then(serde_json::Value::as_array)
            .and_then(|comments| comments.last())
            .and_then(|comment| comment.get("id"))
            .and_then(serde_json::Value::as_str);
        println!(
            "seq={seq} {created_at} actor={actor} issue={issue_id:?} last_comment={last_comment:?}"
        );
    }

    println!("\n== consumer cursors ==");
    let rows = sqlx::query(
        "select consumer_id, partition_key, last_seq from event_consumer_cursors order by consumer_id",
    )
    .fetch_all(&pool)
    .await?;
    for row in &rows {
        let consumer_id: String = row.try_get("consumer_id")?;
        let partition_key: String = row.try_get("partition_key")?;
        let last_seq: i64 = row.try_get("last_seq")?;
        println!("{consumer_id}/{partition_key} last_seq={last_seq}");
    }

    Ok(())
}

fn database_url() -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(url) = std::env::var("DATABASE_URL")
        && !url.trim().is_empty()
    {
        return Ok(url);
    }
    let home = std::env::var("HOME")?;
    let text = std::fs::read_to_string(format!("{home}/.foundry/foundry.toml"))?;
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("database_url") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        return Ok(rest.trim().trim_matches('"').to_string());
    }
    Err("DATABASE_URL is unset and database_url is missing from ~/.foundry/foundry.toml".into())
}
