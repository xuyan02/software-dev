//! Loadable skill documents.
//!
//! A skill is a Markdown document that teaches a procedure, a standard or a
//! checklist. Skills are code-owned, embedded at build time, and exposed to
//! agents through the generic `list_skills` and `load_skill` tools. There is
//! no per-skill tool: adding a skill means adding an entry to [`SKILLS`] and a
//! file under `projects/foundry/skills/`.

/// One loadable skill document.
#[derive(Debug, Clone, Copy)]
pub struct Skill {
    /// Stable identifier used by `load_skill`, for example
    /// `project_brief_writing`.
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub content: &'static str,
}

pub const SKILLS: &[Skill] = &[Skill {
    name: "project_brief_writing",
    title: "Project brief writing",
    description: "What a project brief must contain, what it must not contain, and the \
                  quality bar for every section.",
    content: include_str!("../../../skills/project-brief-writing.md"),
}];

pub fn skills() -> &'static [Skill] {
    SKILLS
}

pub fn skill(name: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|skill| skill.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_skill_loads() {
        let skill = skill("project_brief_writing").expect("registered skill");
        assert_eq!(skill.name, "project_brief_writing");
        assert!(!skill.content.is_empty());
        assert!(skill.content.contains("项目简报"));
    }

    #[test]
    fn unknown_skill_is_none() {
        assert!(skill("does_not_exist").is_none());
    }

    #[test]
    fn skill_names_are_unique() {
        let mut names: Vec<&str> = skills().iter().map(|skill| skill.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
