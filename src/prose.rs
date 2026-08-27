// src/prose.rs
//! Recognition of block-level forms in prose.

use crate::inline::{
    CodeInProse, TableAlignment, code_in_prose, fence_closes, fence_opening,
    inline_math_span, table_row, table_start,
};
use crate::parser::{Line, SourceOrigin};

// Prose block model
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProseBlock {
    Paragraph { start: usize, end: usize },
    List { end: usize, list: ProseList },
    Fence(FencedProse),
    Table(ProseTable),
    DisplayMath { end: usize, source: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProseList {
    pub ordered_start: Option<usize>,
    pub items: Vec<ListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListItem {
    pub parts: Vec<ProseLinePart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProseLinePart {
    pub line: usize,
    pub content_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FencedProse {
    pub start: usize,
    pub end: usize,
    pub content_start: usize,
    pub content_end: usize,
    pub indentation: usize,
    pub info: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProseTable {
    pub end: usize,
    pub alignments: Vec<TableAlignment>,
    pub header: Vec<TableCell>,
    pub body: Vec<Vec<TableCell>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableCell {
    pub line: usize,
    pub text: String,
}

impl ProseBlock {
    pub fn end(&self) -> usize {
        match self {
            Self::Paragraph { end, .. }
            | Self::List { end, .. }
            | Self::DisplayMath { end, .. } => *end,
            Self::Fence(fence) => fence.end,
            Self::Table(table) => table.end,
        }
    }
}

// Inline prose model
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlineElement {
    Text(String),
    Code(String),
    SourceCode {
        text: String,
        origin: SourceOrigin,
    },
    Math(String),
    Strong(Vec<InlineElement>),
    Emphasis(Vec<InlineElement>),
    Link {
        label: Vec<InlineElement>,
        target: String,
        active: bool,
    },
    BlockReference {
        name: String,
        origin: SourceOrigin,
    },
}

pub(crate) struct InlineSource {
    text: String,
    line_origins: Vec<(usize, SourceOrigin)>,
}

impl InlineSource {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            line_origins: Vec::new(),
        }
    }

    pub fn push_line(&mut self, text: &str, origin: &SourceOrigin) {
        if !self.line_origins.is_empty() {
            self.text.push('\n');
        }
        self.line_origins.push((self.text.len(), origin.clone()));
        self.text.push_str(text);
    }

    fn origin_at(&self, position: usize) -> &SourceOrigin {
        let line = self
            .line_origins
            .partition_point(|(start, _)| *start <= position)
            .saturating_sub(1);
        &self.line_origins[line].1
    }

    fn current_line(&self, position: usize, end: usize) -> &str {
        let rest = &self.text[position..end];
        rest.find('\n').map_or(rest, |newline| &rest[..newline])
    }
}

// Classify one prose block
pub(crate) fn prose_block(lines: &[Line], from: usize) -> Option<ProseBlock> {
    let start = (from..lines.len()).find(|line| !lines[*line].text.trim().is_empty())?;

    if let Some(block) = display_math(lines, start) {
        return Some(block);
    }
    if let Some(fence) = fenced_prose(lines, start) {
        return Some(ProseBlock::Fence(fence));
    }
    if let Some(table) = prose_table(lines, start) {
        return Some(ProseBlock::Table(table));
    }
    if let Some(marker) = list_marker(&lines[start].text) {
        return Some(prose_list(lines, start, marker));
    }

    let mut end = start + 1;
    while end < lines.len()
        && !lines[end].text.trim().is_empty()
        && display_math(lines, end).is_none()
        && fenced_prose(lines, end).is_none()
        && prose_table(lines, end).is_none()
        && list_marker(&lines[end].text).is_none()
    {
        end += 1;
    }
    Some(ProseBlock::Paragraph { start, end })
}

// Recognize prose lists
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListMarker {
    Unordered { content_start: usize },
    Ordered { number: usize, content_start: usize },
}

impl ListMarker {
    fn content_start(self) -> usize {
        match self {
            Self::Unordered { content_start } | Self::Ordered { content_start, .. } => {
                content_start
            }
        }
    }

    fn same_kind(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Unordered { .. }, Self::Unordered { .. })
                | (Self::Ordered { .. }, Self::Ordered { .. })
        )
    }
}

fn list_marker(text: &str) -> Option<ListMarker> {
    if text.starts_with("- ") {
        return Some(ListMarker::Unordered { content_start: 2 });
    }

    let digits = text
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 || !text[digits..].starts_with(". ") {
        return None;
    }
    Some(ListMarker::Ordered {
        number: text[..digits].parse().ok()?,
        content_start: digits + 2,
    })
}

fn prose_list(lines: &[Line], start: usize, first_marker: ListMarker) -> ProseBlock {
    let ordered_start = match first_marker {
        ListMarker::Unordered { .. } => None,
        ListMarker::Ordered { number, .. } => Some(number),
    };
    let mut items = Vec::new();
    let mut line = start;

    loop {
        let marker = list_marker(&lines[line].text)
            .expect("the list loop begins at a recognized marker");
        if !first_marker.same_kind(marker) {
            break;
        }

        let content_start = marker.content_start();
        let mut parts = vec![ProseLinePart {
            line,
            content_start,
        }];
        line += 1;
        while line < lines.len() && !lines[line].text.trim().is_empty() {
            if list_marker(&lines[line].text).is_some() {
                break;
            }
            let indentation = lines[line]
                .text
                .bytes()
                .take_while(|byte| *byte == b' ')
                .count();
            if indentation < content_start {
                break;
            }
            parts.push(ProseLinePart {
                line,
                content_start,
            });
            line += 1;
        }
        items.push(ListItem { parts });

        let Some(next_marker) = lines.get(line).and_then(|line| list_marker(&line.text))
        else {
            break;
        };
        if !first_marker.same_kind(next_marker) {
            break;
        }
    }

    ProseBlock::List {
        end: line,
        list: ProseList {
            ordered_start,
            items,
        },
    }
}

// Recognize fenced prose
fn fenced_prose(lines: &[Line], start: usize) -> Option<FencedProse> {
    let opening = fence_opening(&lines[start].text)?;
    let close = (start + 1..lines.len())
        .find(|line| fence_closes(&lines[*line].text, &opening))?;
    Some(FencedProse {
        start,
        end: close + 1,
        content_start: start + 1,
        content_end: close,
        indentation: opening.indentation,
        info: opening.info,
    })
}

// Recognize prose tables
fn prose_table(lines: &[Line], start: usize) -> Option<ProseTable> {
    let syntax = table_start(&lines.get(start)?.text, &lines.get(start + 1)?.text)?;
    let width = syntax.header.len();
    let header = syntax
        .header
        .into_iter()
        .map(|text| TableCell { line: start, text })
        .collect();

    let mut body = Vec::new();
    let mut line = start + 2;
    while line < lines.len()
        && !lines[line].text.trim().is_empty()
        && display_math(lines, line).is_none()
        && fenced_prose(lines, line).is_none()
        && list_marker(&lines[line].text).is_none()
    {
        let Some(cells) = table_row(&lines[line].text) else {
            break;
        };
        let mut row = cells
            .into_iter()
            .take(width)
            .map(|text| TableCell { line, text })
            .collect::<Vec<_>>();
        row.resize_with(width, || TableCell {
            line,
            text: String::new(),
        });
        body.push(row);
        line += 1;
    }

    Some(ProseTable {
        end: line,
        alignments: syntax.alignments,
        header,
        body,
    })
}

// Recognize display equations
fn display_math(lines: &[Line], start: usize) -> Option<ProseBlock> {
    let opening = lines[start].text.trim_start().strip_prefix("$$")?;
    if let Some(closing) = display_math_closing(opening) {
        return Some(ProseBlock::DisplayMath {
            end: start + 1,
            source: opening[..closing].to_owned(),
        });
    }

    let mut source_lines = Vec::new();
    if !opening.trim().is_empty() {
        source_lines.push(opening);
    }
    for (line, located) in lines.iter().enumerate().skip(start + 1) {
        let text = &located.text;
        if let Some(closing) = display_math_closing(text) {
            let final_line = &text[..closing];
            if !final_line.trim().is_empty() {
                source_lines.push(final_line);
            }
            return Some(ProseBlock::DisplayMath {
                end: line + 1,
                source: source_lines.join("\n"),
            });
        }
        source_lines.push(text);
    }
    None
}

fn display_math_closing(text: &str) -> Option<usize> {
    text.match_indices("$$").find_map(|(offset, _)| {
        (!delimiter_is_escaped(text, offset) && text[offset + 2..].trim().is_empty())
            .then_some(offset)
    })
}

fn delimiter_is_escaped(text: &str, offset: usize) -> bool {
    text[..offset]
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count()
        % 2
        == 1
}

// Parse inline prose
pub(crate) fn inline_elements(source: &InlineSource) -> Vec<InlineElement> {
    inline_elements_in(source, 0..source.text.len())
}

fn inline_elements_in(
    source: &InlineSource,
    range: std::ops::Range<usize>,
) -> Vec<InlineElement> {
    let mut elements = Vec::new();
    let mut text = String::new();
    let mut position = range.start;

    while position < range.end {
        let rest = &source.text[position..range.end];
        let line_rest = source.current_line(position, range.end);
        let origin = source.origin_at(position);

        if let Some(after_open) = line_rest.strip_prefix("@{")
            && let Some(end) = after_open.find('}')
        {
            push_inline_text(&mut elements, &mut text);
            elements.push(InlineElement::BlockReference {
                name: after_open[..end].to_owned(),
                origin: origin.clone(),
            });
            position += 2 + end + 1;
            continue;
        }

        if let Some(after_open) = line_rest.strip_prefix('`')
            && let Some(end) = after_open.find('`')
        {
            push_inline_text(&mut elements, &mut text);
            elements.push(InlineElement::Code(after_open[..end].to_owned()));
            position += 1 + end + 1;
            continue;
        }

        if let Some(code) = code_in_prose(line_rest) {
            match code {
                CodeInProse::Closed {
                    text: code,
                    consumed,
                } => {
                    push_inline_text(&mut elements, &mut text);
                    elements.push(InlineElement::SourceCode {
                        text: code,
                        origin: origin.clone(),
                    });
                    position += consumed;
                    continue;
                }
                CodeInProse::Unclosed => {
                    text.push_str(line_rest);
                    position += line_rest.len();
                    continue;
                }
            }
        }

        if let Some(consumed) = inline_math_span(line_rest) {
            push_inline_text(&mut elements, &mut text);
            elements.push(InlineElement::Math(line_rest[1..consumed - 1].to_owned()));
            position += consumed;
            continue;
        }

        if let Some(after_open) = rest.strip_prefix("**") {
            if let Some(end) = after_open.find("**") {
                push_inline_text(&mut elements, &mut text);
                elements.push(InlineElement::Strong(inline_elements_in(
                    source,
                    position + 2..position + 2 + end,
                )));
                position += 2 + end + 2;
            } else {
                text.push_str("**");
                position += 2;
            }
            continue;
        }

        if let Some(after_open) = rest.strip_prefix('*')
            && let Some(end) = after_open.find('*')
        {
            push_inline_text(&mut elements, &mut text);
            elements.push(InlineElement::Emphasis(inline_elements_in(
                source,
                position + 1..position + 1 + end,
            )));
            position += 1 + end + 1;
            continue;
        }

        if let Some(after_open) = line_rest.strip_prefix('[')
            && let Some(label_end) = after_open.find("](")
        {
            let after_target_open = &after_open[label_end + 2..];
            if let Some(target_end) = after_target_open.find(')') {
                push_inline_text(&mut elements, &mut text);
                let target = after_target_open[..target_end].to_owned();
                elements.push(InlineElement::Link {
                    label: inline_elements_in(
                        source,
                        position + 1..position + 1 + label_end,
                    ),
                    active: safe_link_target(&target),
                    target,
                });
                position += 1 + label_end + 2 + target_end + 1;
                continue;
            }
        }

        if let Some(after_escape) = line_rest.strip_prefix('\\')
            && let Some(character) = after_escape.chars().next()
        {
            text.push(character);
            position += 1 + character.len_utf8();
            continue;
        }

        let character = rest
            .chars()
            .next()
            .expect("position is on a character boundary");
        text.push(character);
        position += character.len_utf8();
    }

    push_inline_text(&mut elements, &mut text);
    elements
}

fn push_inline_text(elements: &mut Vec<InlineElement>, text: &mut String) {
    if !text.is_empty() {
        elements.push(InlineElement::Text(std::mem::take(text)));
    }
}

fn safe_link_target(target: &str) -> bool {
    if target.chars().any(char::is_control) {
        return false;
    }
    let target = target.trim_start().to_ascii_lowercase();
    !target.starts_with("javascript:")
        && !target.starts_with("data:")
        && !target.starts_with("vbscript:")
}
