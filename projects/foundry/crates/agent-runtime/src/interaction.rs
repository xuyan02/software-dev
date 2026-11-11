//! Human-in-the-loop questions.
//!
//! The agent asks one structured question at a time and waits for the human.
//! The tool call is recorded in the trajectory, so the question and answer are
//! part of the audit trail.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default = "default_allow_free_text")]
    pub allow_free_text: bool,
    #[serde(default)]
    pub context: Option<String>,
}

fn default_allow_free_text() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub text: String,
    #[serde(default)]
    pub selected_option: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

impl Answer {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            selected_option: None,
            skipped: false,
        }
    }

    pub fn selected(option: impl Into<String>) -> Self {
        let option = option.into();
        Self {
            text: option.clone(),
            selected_option: Some(option),
            skipped: false,
        }
    }

    pub fn skipped() -> Self {
        Self {
            text: String::new(),
            selected_option: None,
            skipped: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InteractionError {
    #[error("interaction failed: {0}")]
    Failed(String),
    #[error("interaction cancelled")]
    Cancelled,
}

#[async_trait]
pub trait HumanInteraction: Send + Sync {
    async fn ask(&self, question: Question) -> Result<Answer, InteractionError>;
}

pub struct HumanAskTool {
    interaction: Arc<dyn HumanInteraction>,
}

impl HumanAskTool {
    pub fn new(interaction: Arc<dyn HumanInteraction>) -> Self {
        Self { interaction }
    }
}

#[async_trait]
impl Tool for HumanAskTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "human_ask".to_string(),
            description: "Ask the human one question and wait for the answer. Use this \
                          instead of guessing when intent is unclear."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "prompt": { "type": "string" },
                    "options": { "type": "array", "items": { "type": "string" } },
                    "allow_free_text": { "type": "boolean", "default": true },
                    "context": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let prompt = arguments
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing prompt".to_string()))?;
        let options = arguments
            .get("options")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let allow_free_text = arguments
            .get("allow_free_text")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let question = Question {
            id: uuid::Uuid::now_v7().to_string(),
            prompt: prompt.to_string(),
            options,
            allow_free_text,
            context: arguments
                .get("context")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        match self.interaction.ask(question).await {
            Ok(answer) if answer.skipped => Ok(ToolOutput::success("human skipped the question")),
            Ok(answer) => Ok(ToolOutput::success(match answer.selected_option {
                Some(option) => format!("{option}\n{}", answer.text),
                None => answer.text,
            })),
            Err(error) => Ok(ToolOutput::error(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct ScriptedInteraction {
        answers: Mutex<VecDeque<Answer>>,
        questions: Mutex<Vec<Question>>,
    }

    #[async_trait]
    impl HumanInteraction for ScriptedInteraction {
        async fn ask(&self, question: Question) -> Result<Answer, InteractionError> {
            self.questions.lock().unwrap().push(question);
            self.answers
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| InteractionError::Failed("no answer".to_string()))
        }
    }

    #[tokio::test]
    async fn asks_the_human_and_returns_the_answer() {
        let interaction = Arc::new(ScriptedInteraction {
            answers: Mutex::new(vec![Answer::text("700 件/月")].into()),
            questions: Mutex::new(Vec::new()),
        });
        let tool = HumanAskTool::new(interaction.clone());
        let output = tool
            .call(
                json!({ "prompt": "问题的影响有多大？" }),
                &ToolContext {
                    agent_id: "a".to_string(),
                    task_id: "t".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(output.content, "700 件/月");
        let questions = interaction.questions.lock().unwrap();
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].prompt, "问题的影响有多大？");
    }

    #[tokio::test]
    async fn reports_skipped_answers() {
        let interaction = Arc::new(ScriptedInteraction {
            answers: Mutex::new(vec![Answer::skipped()].into()),
            questions: Mutex::new(Vec::new()),
        });
        let tool = HumanAskTool::new(interaction);
        let output = tool
            .call(
                json!({ "prompt": "优先级？" }),
                &ToolContext {
                    agent_id: "a".to_string(),
                    task_id: "t".to_string(),
                },
            )
            .await
            .unwrap();
        assert!(output.content.contains("skipped"));
    }
}
