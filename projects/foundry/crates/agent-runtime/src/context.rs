//! Explicit context assembly for a run.
//!
//! Phase 0 keeps this deliberately small: a bundle is an ordered list of
//! cited items that renders into the user message. Later stages add token
//! budgeting, permission filtering and retrieval.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextItem {
    pub kind: String,
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ContextBundle {
    pub items: Vec<ContextItem>,
}

impl ContextBundle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(
        &mut self,
        kind: impl Into<String>,
        source: impl Into<String>,
        content: impl Into<String>,
    ) -> &mut Self {
        self.items.push(ContextItem {
            kind: kind.into(),
            source: source.into(),
            content: content.into(),
        });
        self
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn render(&self) -> String {
        self.items
            .iter()
            .map(|item| format!("[{}] {}\n{}", item.kind, item.source, item.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
