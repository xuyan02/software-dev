//! Deprecated: the single generic worker has been replaced by managed agent
//! processes.
//!
//! Each agent now runs as its own `foundry-agentd` process, started and
//! supervised by the control plane (`/agents`). This binary is kept only so
//! older scripts fail loudly instead of claiming runs without an agent
//! identity.

fn main() -> anyhow::Result<()> {
    anyhow::bail!(
        "foundry-worker is deprecated; configure and start agents in the control plane /agents page"
    )
}
