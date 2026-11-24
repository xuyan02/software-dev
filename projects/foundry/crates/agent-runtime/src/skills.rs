//! Code-owned skill prompts.
//!
//! A skill is an ID mapped to one prompt file. Agent definitions list skill
//! IDs; the runtime appends every configured skill to the base system prompt.

use std::collections::HashSet;
use std::fmt;

pub struct Skill {
    pub id: &'static str,
    pub prompt: &'static str,
}

const SKILLS: &[Skill] = &[
    Skill {
        id: "arche",
        prompt: include_str!("../skills/arche.md"),
    },
    Skill {
        id: "design-writing",
        prompt: include_str!("../skills/design-writing.md"),
    },
    Skill {
        id: "design-draft",
        prompt: include_str!("../skills/design-draft.md"),
    },
    Skill {
        id: "design-content-review",
        prompt: include_str!("../skills/design-content-review.md"),
    },
    Skill {
        id: "code-summary",
        prompt: include_str!("../skills/code-summary.md"),
    },
    Skill {
        id: "design-consistency",
        prompt: include_str!("../skills/design-consistency.md"),
    },
    Skill {
        id: "issue-loop",
        prompt: include_str!("../skills/issue-loop.md"),
    },
    Skill {
        id: "code-inspection",
        prompt: include_str!("../skills/code-inspection.md"),
    },
    Skill {
        id: "project-document-writing-guide",
        prompt: include_str!("../../../docs/design/project-document-writing-guide.md"),
    },
    Skill {
        id: "requirements-arche",
        prompt: include_str!("../skills/requirements-arche.md"),
    },
    Skill {
        id: "requirements-arche-review",
        prompt: include_str!("../skills/requirements-arche-review.md"),
    },
    Skill {
        id: "development-record",
        prompt: include_str!("../skills/development-record.md"),
    },
    Skill {
        id: "development-review",
        prompt: include_str!("../skills/development-review.md"),
    },
    Skill {
        id: "test-plan-writing-guide",
        prompt: include_str!("../skills/test-plan-writing-guide.md"),
    },
    Skill {
        id: "test-execution",
        prompt: include_str!("../skills/test-execution.md"),
    },
    Skill {
        id: "test-report-review",
        prompt: include_str!("../skills/test-report-review.md"),
    },
    Skill {
        id: "project-document-writing",
        prompt: include_str!("../skills/project-document-writing.md"),
    },
    Skill {
        id: "style-review",
        prompt: include_str!("../skills/style-review.md"),
    },
    Skill {
        id: "project-document-content-review",
        prompt: include_str!("../skills/project-document-content-review.md"),
    },
    Skill {
        id: "project-clarification",
        prompt: include_str!("../skills/project-clarification.md"),
    },
];

pub fn skill(id: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|skill| skill.id == id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillError(pub String);

impl fmt::Display for SkillError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown skill: {}", self.0)
    }
}

impl std::error::Error for SkillError {}

/// Append every configured skill to the base prompt. Unknown skill IDs are an
/// error: a configured skill must never be silently dropped.
pub fn compose_system_prompt(base: &str, skills: &[String]) -> Result<String, SkillError> {
    if skills.is_empty() {
        return Ok(base.to_string());
    }
    let mut out = base.trim_end().to_string();
    out.push_str("\n\n## Skills\n");
    let mut seen = HashSet::new();
    for id in skills {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let skill = skill(id).ok_or_else(|| SkillError(id.clone()))?;
        out.push_str("\n### ");
        out.push_str(skill.id);
        out.push_str("\n\n");
        out.push_str(skill.prompt.trim());
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_configured_skill() {
        let prompt = compose_system_prompt("base", &["arche".to_string()]).unwrap();
        assert!(prompt.starts_with("base"));
        assert!(prompt.contains("## Skills"));
        assert!(prompt.contains("### arche"));
    }

    #[test]
    fn appends_project_document_writing_guide() {
        let prompt =
            compose_system_prompt("base", &["project-document-writing-guide".to_string()]).unwrap();
        assert!(prompt.contains("### project-document-writing-guide"));
        assert!(prompt.contains("项目文档写作说明书"));
    }

    #[test]
    fn appends_project_document_writing_skill() {
        let prompt =
            compose_system_prompt("base", &["project-document-writing".to_string()]).unwrap();
        assert!(prompt.contains("### project-document-writing"));
        assert!(prompt.contains("项目文档撰写技能"));
    }

    #[test]
    fn appends_style_review_skill() {
        let prompt = compose_system_prompt("base", &["style-review".to_string()]).unwrap();
        assert!(prompt.contains("### style-review"));
        assert!(prompt.contains("文风审查技能"));
    }

    #[test]
    fn appends_project_document_content_review_skill() {
        let prompt =
            compose_system_prompt("base", &["project-document-content-review".to_string()])
                .unwrap();
        assert!(prompt.contains("### project-document-content-review"));
        assert!(prompt.contains("项目说明书内容审查技能"));
    }

    #[test]
    fn appends_project_clarification_skill() {
        let prompt = compose_system_prompt("base", &["project-clarification".to_string()]).unwrap();
        assert!(prompt.contains("### project-clarification"));
        assert!(prompt.contains("项目澄清技能"));
    }

    #[test]
    fn appends_design_development_and_test_skills() {
        for (id, title) in [
            ("development-record", "开发记录技能"),
            ("development-review", "开发记录审查技能"),
            ("test-plan-writing-guide", "测试计划说明书"),
            ("test-execution", "测试执行技能"),
            ("test-report-review", "测试报告审查技能"),
        ] {
            let prompt = compose_system_prompt("base", &[id.to_string()]).unwrap();
            assert!(prompt.contains(&format!("### {id}")));
            assert!(prompt.contains(title));
        }
    }

    #[test]
    fn rejects_unknown_skill() {
        let error = compose_system_prompt("base", &["missing".to_string()]).unwrap_err();
        assert_eq!(error.0, "missing");
    }
    #[test]
    fn appends_design_content_review_skill() {
        let prompt = compose_system_prompt("base", &["design-content-review".to_string()]).unwrap();
        assert!(prompt.contains("### design-content-review"));
        assert!(prompt.contains("设计内容评审技能"));
        assert!(prompt.contains("不要只找设计错误"));
    }

    #[test]
    fn appends_design_writing_skill() {
        let prompt = compose_system_prompt("base", &["design-writing".to_string()]).unwrap();
        assert!(prompt.contains("### design-writing"));
        assert!(prompt.contains("设计文档撰写技能"));
        assert!(prompt.contains("置放"));
        assert!(prompt.contains("只包含常量的枚举"));
        assert!(prompt.contains("变体携带数据的"));
        assert!(prompt.contains("复杂度分治"));
    }

    #[test]
    fn appends_code_summary_skill() {
        let prompt = compose_system_prompt("base", &["code-summary".to_string()]).unwrap();
        assert!(prompt.contains("### code-summary"));
        assert!(prompt.contains("代码总结技能"));
        assert!(prompt.contains("反推设计"));
        assert!(prompt.contains("用户提示词"));
        assert!(prompt.contains("文件总结"));
        assert!(prompt.contains("目录总结"));
        assert!(prompt.contains("修改工作项"));
        assert!(prompt.contains("文件 Review"));
        assert!(prompt.contains("目录 Review"));
        assert!(prompt.contains("整体 Review"));
        assert!(prompt.contains("修改阶段"));
    }

    #[test]
    fn arche_skill_covers_object_kinds_and_documents() {
        let skill = skill("arche").expect("arche skill");
        for kind in [
            "Directory",
            "File",
            "Namespace",
            "Class",
            "Struct",
            "Enum",
            "Interface",
            "Method",
            "Function",
            "Part",
            "Mixin",
        ] {
            assert!(skill.prompt.contains(kind), "missing {kind}");
        }
        assert!(skill.prompt.contains("documents"));
        assert!(skill.prompt.contains("design-writing"));
        assert!(skill.prompt.contains("arche source sync"));
        assert!(skill.prompt.contains("变体携带数据"));
    }
}
