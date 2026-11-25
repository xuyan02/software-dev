use anyhow::{Result, bail};

// Help topics are authored as standalone Markdown files under
// `projects/arche/help/` and embedded into the binary at compile time.
const OVERVIEW: &str = include_str!("../../../help/overview.md");
const OBJECTS: &str = include_str!("../../../help/objects.md");
const FIELDS: &str = include_str!("../../../help/fields.md");
const DESIGN: &str = include_str!("../../../help/design.md");
const WRITING: &str = include_str!("../../../help/writing.md");
const WRITING_DETAIL: &str = include_str!("../../../help/writing-detail.md");
const KINDS: &str = include_str!("../../../help/kinds.md");
const NAMING: &str = include_str!("../../../help/naming.md");
const TECHNICAL: &str = include_str!("../../../help/technical.md");
const REQUIREMENT: &str = include_str!("../../../help/requirement.md");
const REQUIREMENT_USER: &str = include_str!("../../../help/requirement-user.md");
const STORY: &str = include_str!("../../../help/story.md");
const REQUIREMENT_SYSTEM: &str = include_str!("../../../help/requirement-system.md");
const REQUIREMENT_MODULE: &str = include_str!("../../../help/requirement-module.md");
const REQUIREMENT_FUNCTION: &str = include_str!("../../../help/requirement-function.md");
const REQUIREMENT_INTERACTION: &str = include_str!("../../../help/requirement-interaction.md");
const CONCEPT: &str = include_str!("../../../help/concept.md");
const DOCUMENT: &str = include_str!("../../../help/document.md");
const STAGING: &str = include_str!("../../../help/staging.md");
const SOURCE: &str = include_str!("../../../help/source.md");
const REMOTE: &str = include_str!("../../../help/remote.md");
const EXAMPLES: &str = include_str!("../../../help/examples.md");

const DESIGN_KINDS: &[&str] = &[
    "TechnicalSystem",
    "TechnicalModule",
    "TechnicalLayer",
    "TechnicalDirectory",
    "TechnicalFile",
    "TechnicalNamespace",
    "TechnicalClass",
    "TechnicalStruct",
    "TechnicalEnum",
    "TechnicalInterface",
    "TechnicalMixin",
    "TechnicalMethod",
    "TechnicalFunction",
    "TechnicalPart",
    "TechnicalVariable",
    "TechnicalConstant",
    "TechnicalMember",
    "TechnicalProperty",
    "TechnicalEnumValue",
    "TechnicalArgument",
    "TechnicalReturnValue",
    "RequirementUser",
    "RequirementStory",
    "RequirementSystem",
    "RequirementModule",
    "RequirementFunction",
    "RequirementInteraction",
];

fn design_kind(topic: &str) -> Option<&'static str> {
    DESIGN_KINDS
        .iter()
        .copied()
        .find(|kind| kind.to_ascii_lowercase() == topic)
}

fn print_design_section(kind: &str) {
    let mut printing = false;
    for line in DESIGN.lines() {
        if !printing {
            if line == kind {
                printing = true;
                println!("{line}");
            }
            continue;
        }
        if DESIGN_KINDS.contains(&line) {
            break;
        }
        println!("{line}");
    }
}

pub fn print(topic: Option<&str>) -> Result<()> {
    let topic = topic.map(|topic| topic.to_ascii_lowercase());
    match topic.as_deref() {
        None | Some("overview") => print!("{OVERVIEW}"),
        Some("objects") => print!("{OBJECTS}"),
        Some("fields") => print!("{FIELDS}"),
        Some("design") | Some("definitions") => print!("{DESIGN}"),
        Some("writing") | Some("document-writing") => print!("{WRITING}"),
        Some("writing-detail") | Some("writing-guide") => print!("{WRITING_DETAIL}"),
        Some("kinds") | Some("kind") => print!("{KINDS}"),
        Some("naming") | Some("names") => print!("{NAMING}"),
        Some(topic) if design_kind(topic).is_some() => {
            print_design_section(design_kind(topic).expect("checked"))
        }
        Some("technical") => print!("{TECHNICAL}"),
        Some("requirement") | Some("requirements") => print!("{REQUIREMENT}"),
        Some("story") | Some("stories") => print!("{STORY}"),
        Some("requirement-user") => print!("{REQUIREMENT_USER}"),
        Some("requirement-system") => print!("{REQUIREMENT_SYSTEM}"),
        Some("requirement-module") => print!("{REQUIREMENT_MODULE}"),
        Some("requirement-function") => print!("{REQUIREMENT_FUNCTION}"),
        Some("requirement-interaction") => print!("{REQUIREMENT_INTERACTION}"),
        Some("concept") | Some("concepts") => print!("{CONCEPT}"),
        Some("document") | Some("documents") => print!("{DOCUMENT}"),
        Some("staging") | Some("index") => print!("{STAGING}"),
        Some("source") => print!("{SOURCE}"),
        Some("remote") => print!("{REMOTE}"),
        Some("examples") => print!("{EXAMPLES}"),
        Some(topic) if topic.starts_with("technical") => print!("{FIELDS}"),
        Some(topic) if topic.starts_with("requirement") => print!("{REQUIREMENT}"),
        Some(topic) if topic.starts_with("concept") => print!("{CONCEPT}"),
        Some(topic) if topic.starts_with("document") => print!("{DOCUMENT}"),
        Some(other) => bail!("unknown help topic {other}"),
    }
    Ok(())
}
