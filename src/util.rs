// src/util.rs
//! Small utilities shared by Litweb's core modules.

// Reference and whitespace helpers
pub(crate) fn block_reference(text: &str) -> Option<&str> {
    let stripped = text.trim();
    (stripped.starts_with("@{") && stripped.ends_with('}') && stripped.len() >= 3)
        .then(|| &stripped[2..stripped.len() - 1])
}

pub(crate) fn leading_whitespace(text: &str) -> &str {
    let end = text
        .char_indices()
        .find_map(|(index, character)| (!character.is_whitespace()).then_some(index))
        .unwrap_or(text.len());
    &text[..end]
}
