//! Requirement-user priority levels.
//!
//! Storage stays numeric; the command line and UI expose names.

pub const PRIORITY_LEVELS: &[(&str, &str, u8)] = &[
    ("core", "核心用户", 1),
    ("primary", "主要用户", 2),
    ("secondary", "次要用户", 3),
    ("peripheral", "边缘用户", 4),
];

/// Parse a priority name (`core`, `核心用户`, ...). Numeric values are accepted
/// only for backward compatibility and are not part of the documented interface.
pub fn parse_priority(text: &str) -> Option<u8> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    for (slug, label, value) in PRIORITY_LEVELS {
        if text.eq_ignore_ascii_case(slug) || text == *label {
            return Some(*value);
        }
    }
    text.parse::<u8>()
        .ok()
        .filter(|value| (1..=4).contains(value))
}

pub fn priority_slug(value: u8) -> Option<&'static str> {
    PRIORITY_LEVELS
        .iter()
        .find(|(_, _, level)| *level == value)
        .map(|(slug, _, _)| *slug)
}

pub fn priority_label(value: u8) -> Option<&'static str> {
    PRIORITY_LEVELS
        .iter()
        .find(|(_, _, level)| *level == value)
        .map(|(_, label, _)| *label)
}

/// Human-readable text, for example `核心用户（core）`.
pub fn priority_display(value: u8) -> String {
    match (priority_label(value), priority_slug(value)) {
        (Some(label), Some(slug)) => format!("{label}（{slug}）"),
        _ if value == 0 => "未标定".to_string(),
        _ => value.to_string(),
    }
}
