//! Minimal evaluation harness for agent runs.
//!
//! A case checks the final answer and stop reason. Later stages add holdout
//! tasks, rubrics and scoring; the runtime contract stays the same.

use serde::Serialize;

use crate::{AgentRunRequest, AgentRuntime, StopReason, Usage};

#[derive(Debug, Clone)]
pub struct EvaluationCase {
    pub id: String,
    pub system_prompt: String,
    pub user_message: String,
    pub expected_contains: Vec<String>,
}

impl EvaluationCase {
    pub fn new(
        id: impl Into<String>,
        system_prompt: impl Into<String>,
        user_message: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            system_prompt: system_prompt.into(),
            user_message: user_message.into(),
            expected_contains: Vec::new(),
        }
    }

    pub fn expect(mut self, text: impl Into<String>) -> Self {
        self.expected_contains.push(text.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationReport {
    pub case_id: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    pub steps: u32,
    pub usage: Usage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn evaluate_case(runtime: &AgentRuntime, case: &EvaluationCase) -> EvaluationReport {
    let result = runtime
        .run_to_completion(AgentRunRequest::new(
            "evaluation",
            &case.id,
            &case.system_prompt,
            &case.user_message,
        ))
        .await;
    match result {
        Ok(result) => {
            let final_message = result.final_message.unwrap_or_default();
            let passed = result.stop_reason == StopReason::Completed
                && case
                    .expected_contains
                    .iter()
                    .all(|expected| final_message.contains(expected));
            EvaluationReport {
                case_id: case.id.clone(),
                passed,
                final_message: Some(final_message),
                stop_reason: Some(result.stop_reason),
                steps: result.steps,
                usage: result.usage,
                cost_usd: result.cost_usd,
                error: None,
            }
        }
        Err(error) => EvaluationReport {
            case_id: case.id.clone(),
            passed: false,
            final_message: None,
            stop_reason: None,
            steps: 0,
            usage: Usage::default(),
            cost_usd: None,
            error: Some(error.to_string()),
        },
    }
}

/// Run each case with a fresh runtime from the factory.
pub async fn evaluate_suite(
    runtime_factory: impl Fn() -> AgentRuntime,
    cases: &[EvaluationCase],
) -> Vec<EvaluationReport> {
    let mut reports = Vec::with_capacity(cases.len());
    for case in cases {
        let runtime = runtime_factory();
        reports.push(evaluate_case(&runtime, case).await);
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, InMemoryTrajectory, ModelResponse, ScriptedModel, ToolRegistry};
    use std::sync::Arc;

    fn runtime(responses: Vec<ModelResponse>) -> AgentRuntime {
        AgentRuntime::new(
            Arc::new(ScriptedModel::new(responses)),
            Arc::new(ToolRegistry::new()),
            Arc::new(InMemoryTrajectory::new()),
            Budget::default(),
        )
    }

    #[tokio::test]
    async fn passes_when_the_final_answer_contains_the_expected_text() {
        let case = EvaluationCase::new("case-1", "system", "do it").expect("done");
        let report = evaluate_case(
            &runtime(vec![ModelResponse::final_answer("all done")]),
            &case,
        )
        .await;
        assert!(report.passed);
        assert_eq!(report.stop_reason, Some(StopReason::Completed));
    }

    #[tokio::test]
    async fn fails_when_the_answer_is_wrong() {
        let case = EvaluationCase::new("case-2", "system", "do it").expect("done");
        let report = evaluate_case(
            &runtime(vec![ModelResponse::final_answer("not yet")]),
            &case,
        )
        .await;
        assert!(!report.passed);
    }

    #[tokio::test]
    async fn reports_runtime_errors() {
        let case = EvaluationCase::new("case-3", "system", "do it");
        let report = evaluate_case(&runtime(Vec::new()), &case).await;
        assert!(!report.passed);
        assert!(report.error.is_some());
    }
}
