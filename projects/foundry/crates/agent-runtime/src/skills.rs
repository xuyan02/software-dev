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
        id: "document",
        prompt: include_str!("../skills/document.md"),
    },
    Skill {
        id: "project-document-writing-guide",
        prompt: include_str!("../../../docs/design/project-document-writing-guide.md"),
    },
    Skill {
        id: "requirements-analysis",
        prompt: include_str!("../skills/requirements-analysis.md"),
    },
    Skill {
        id: "requirements-document-review",
        prompt: include_str!("../skills/requirements-document-review.md"),
    },
    Skill {
        id: "requirements-feasibility-review",
        prompt: include_str!("../skills/requirements-feasibility-review.md"),
    },
    Skill {
        id: "architecture-document-writing-guide",
        prompt: include_str!("../skills/architecture-document-writing-guide.md"),
    },
    Skill {
        id: "architecture-design",
        prompt: include_str!("../skills/architecture-design.md"),
    },
    Skill {
        id: "architecture-review",
        prompt: include_str!("../skills/architecture-review.md"),
    },
    Skill {
        id: "interaction-design",
        prompt: include_str!("../skills/interaction-design.md"),
    },
    Skill {
        id: "interaction-design-review",
        prompt: include_str!("../skills/interaction-design-review.md"),
    },
    Skill {
        id: "ui-design",
        prompt: include_str!("../skills/ui-design.md"),
    },
    Skill {
        id: "ui-design-review",
        prompt: include_str!("../skills/ui-design-review.md"),
    },
    Skill {
        id: "requirements-document-writing-guide",
        prompt: include_str!("../skills/requirements-document-writing-guide.md"),
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
        let prompt = compose_system_prompt("base", &["document".to_string()]).unwrap();
        assert!(prompt.starts_with("base"));
        assert!(prompt.contains("## Skills"));
        assert!(prompt.contains("### document"));
        assert!(prompt.contains("Document tool"));
    }

    #[test]
    fn appends_project_document_writing_guide() {
        let prompt =
            compose_system_prompt("base", &["project-document-writing-guide".to_string()]).unwrap();
        assert!(prompt.contains("### project-document-writing-guide"));
        assert!(prompt.contains("项目文档写作说明书"));
    }

    #[test]
    fn appends_requirements_analysis_skill() {
        let prompt = compose_system_prompt("base", &["requirements-analysis".to_string()]).unwrap();
        assert!(prompt.contains("### requirements-analysis"));
        assert!(prompt.contains("需求分析技能"));
    }

    #[test]
    fn appends_requirements_document_review_skill() {
        let prompt =
            compose_system_prompt("base", &["requirements-document-review".to_string()]).unwrap();
        assert!(prompt.contains("### requirements-document-review"));
        assert!(prompt.contains("需求文档审查技能"));
    }

    #[test]
    fn appends_requirements_feasibility_review_skill() {
        let prompt =
            compose_system_prompt("base", &["requirements-feasibility-review".to_string()])
                .unwrap();
        assert!(prompt.contains("### requirements-feasibility-review"));
        assert!(prompt.contains("需求可行性审查技能"));
    }

    #[test]
    fn appends_design_skills() {
        for (id, title) in [
            ("interaction-design", "交互设计技能"),
            ("interaction-design-review", "交互设计审查技能"),
            ("ui-design", "UI 设计技能"),
            ("ui-design-review", "UI 设计审查技能"),
        ] {
            let prompt = compose_system_prompt("base", &[id.to_string()]).unwrap();
            assert!(prompt.contains(&format!("### {id}")));
            assert!(prompt.contains(title));
        }
    }

    #[test]
    fn appends_requirements_document_writing_guide() {
        let prompt =
            compose_system_prompt("base", &["requirements-document-writing-guide".to_string()])
                .unwrap();
        assert!(prompt.contains("### requirements-document-writing-guide"));
        assert!(prompt.contains("需求文档写作说明书"));
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
        assert!(prompt.contains("项目文档内容审查技能"));
    }

    #[test]
    fn appends_project_clarification_skill() {
        let prompt = compose_system_prompt("base", &["project-clarification".to_string()]).unwrap();
        assert!(prompt.contains("### project-clarification"));
        assert!(prompt.contains("项目澄清技能"));
    }

    #[test]
    fn appends_architecture_skills() {
        for (id, title) in [
            ("architecture-document-writing-guide", "架构设计说明书"),
            ("architecture-design", "架构设计技能"),
            ("architecture-review", "架构设计审查技能"),
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
}
