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
        id: "requirements-document-writing-guide",
        prompt: include_str!("../../../docs/design/requirements-document-writing-guide.md"),
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
    fn rejects_unknown_skill() {
        let error = compose_system_prompt("base", &["missing".to_string()]).unwrap_err();
        assert_eq!(error.0, "missing");
    }
}
