// src/inline.rs
//! Shared recognition of prose delimiters and target-language spans.

// Fenced prose delimiter recognizer
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FenceOpening {
    pub character: u8,
    pub length: usize,
    pub indentation: usize,
    pub info: String,
}

pub(crate) fn fence_opening(text: &str) -> Option<FenceOpening> {
    let indentation = text.bytes().take_while(|byte| *byte == b' ').count();
    if indentation > 3 {
        return None;
    }
    let rest = &text[indentation..];
    let character = *rest.as_bytes().first()?;
    if !matches!(character, b'`' | b'~') {
        return None;
    }
    let length = rest.bytes().take_while(|byte| *byte == character).count();
    if length < 3 {
        return None;
    }
    let info = rest[length..].trim();
    if character == b'`' && info.contains('`') {
        return None;
    }
    Some(FenceOpening {
        character,
        length,
        indentation,
        info: info.to_owned(),
    })
}

pub(crate) fn fence_closes(text: &str, opening: &FenceOpening) -> bool {
    let indentation = text.bytes().take_while(|byte| *byte == b' ').count();
    if indentation > 3 {
        return false;
    }
    let rest = &text[indentation..];
    let length = rest
        .bytes()
        .take_while(|byte| *byte == opening.character)
        .count();
    length >= opening.length && rest[length..].trim().is_empty()
}

pub(crate) fn fenced_line_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut line = 0;
    while line < lines.len() {
        let Some(opening) = fence_opening(lines[line]) else {
            line += 1;
            continue;
        };
        let Some(close) = (line + 1..lines.len())
            .find(|candidate| fence_closes(lines[*candidate], &opening))
        else {
            line += 1;
            continue;
        };
        mask[line..=close].fill(true);
        line = close + 1;
    }
    mask
}

// Inline code-span recognizer
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodeInProse {
    Closed { text: String, consumed: usize },
    Unclosed,
}

pub(crate) fn code_in_prose(text: &str) -> Option<CodeInProse> {
    const OPEN: &str = "@code{";

    let contents = text.strip_prefix(OPEN)?;
    let mut code = String::new();
    let mut depth = 1;
    let mut characters = contents.char_indices().peekable();

    while let Some((offset, character)) = characters.next() {
        if character == '\\' {
            if let Some((_, escaped)) = characters.peek().copied()
                && matches!(escaped, '{' | '}' | '\\')
            {
                characters.next();
                code.push(escaped);
                continue;
            }
            code.push(character);
            continue;
        }

        match character {
            '{' => {
                depth += 1;
                code.push(character);
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(CodeInProse::Closed {
                        text: code,
                        consumed: OPEN.len() + offset + character.len_utf8(),
                    });
                }
                code.push(character);
            }
            _ => code.push(character),
        }
    }

    Some(CodeInProse::Unclosed)
}

// Inline math-span recognizer
pub(crate) fn inline_math_span(text: &str) -> Option<usize> {
    let contents = text.strip_prefix('$')?;
    if contents.starts_with('$') {
        return None;
    }

    let mut backslashes = 0;
    for (offset, character) in contents.char_indices() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        if character == '$'
            && backslashes % 2 == 0
            && !contents[..offset].ends_with('$')
            && !contents[offset + 1..].starts_with('$')
        {
            return Some(1 + offset + 1);
        }
        backslashes = 0;
    }
    None
}

// Pipe-table row recognizer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableStart {
    pub header: Vec<String>,
    pub alignments: Vec<TableAlignment>,
}

pub(crate) fn table_start(header: &str, delimiter: &str) -> Option<TableStart> {
    let header = table_row(header)?;
    let alignments = table_delimiter(delimiter)?;
    if header.is_empty() || header.len() != alignments.len() {
        return None;
    }

    Some(TableStart { header, alignments })
}

pub(crate) fn table_candidate(header: &str, delimiter: &str) -> bool {
    table_row(header).is_some() && table_delimiter(delimiter).is_some()
}

pub(crate) fn table_row(text: &str) -> Option<Vec<String>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let leading_separator = trimmed.starts_with('|');
    let mut trailing_separator = false;
    let mut cells = vec![String::new()];
    let mut separators = 0;
    let mut position = 0;

    while position < text.len() {
        let rest = &text[position..];
        if let Some(consumed) = protected_table_span(rest) {
            cells
                .last_mut()
                .expect("a table row always has a current cell")
                .push_str(&rest[..consumed]);
            if rest[..consumed]
                .chars()
                .any(|character| !character.is_whitespace())
            {
                trailing_separator = false;
            }
            position += consumed;
            continue;
        }

        let character = rest
            .chars()
            .next()
            .expect("position remains on a character boundary");
        position += character.len_utf8();
        if character == '|' {
            separators += 1;
            cells.push(String::new());
            trailing_separator = true;
        } else {
            cells
                .last_mut()
                .expect("a table row always has a current cell")
                .push(character);
            if !character.is_whitespace() {
                trailing_separator = false;
            }
        }
    }

    if separators == 0 {
        return None;
    }
    if leading_separator {
        cells.remove(0);
    }
    if trailing_separator {
        cells.pop();
    }
    for cell in &mut cells {
        *cell = cell.trim().to_owned();
    }
    (!cells.is_empty()).then_some(cells)
}

pub(crate) fn table_line_mask(lines: &[&str]) -> Vec<bool> {
    let fenced = fenced_line_mask(lines);
    let mut mask = vec![false; lines.len()];
    let mut line = 0;

    while line + 1 < lines.len() {
        if fenced[line]
            || fenced[line + 1]
            || !table_candidate(lines[line], lines[line + 1])
        {
            line += 1;
            continue;
        }

        let start = line;
        line += 2;
        while line < lines.len()
            && !fenced[line]
            && !lines[line].trim().is_empty()
            && table_row(lines[line]).is_some()
        {
            line += 1;
        }
        mask[start..line].fill(true);
    }
    mask
}

fn table_alignment(text: &str) -> Option<TableAlignment> {
    let mut delimiter = text.trim();
    let leading = delimiter.starts_with(':');
    if leading {
        delimiter = &delimiter[1..];
    }
    let trailing = delimiter.ends_with(':');
    if trailing {
        delimiter = &delimiter[..delimiter.len() - 1];
    }
    if delimiter.len() < 3 || !delimiter.bytes().all(|byte| byte == b'-') {
        return None;
    }
    Some(match (leading, trailing) {
        (true, true) => TableAlignment::Center,
        (false, true) => TableAlignment::Right,
        _ => TableAlignment::Left,
    })
}

fn table_delimiter(text: &str) -> Option<Vec<TableAlignment>> {
    table_row(text)?
        .iter()
        .map(|cell| table_alignment(cell))
        .collect()
}

fn protected_table_span(text: &str) -> Option<usize> {
    if let Some(after_escape) = text.strip_prefix('\\') {
        return Some(1 + after_escape.chars().next().map_or(0, char::len_utf8));
    }

    if let Some(after_open) = text.strip_prefix('`')
        && let Some(end) = after_open.find('`')
    {
        return Some(1 + end + 1);
    }

    if let Some(CodeInProse::Closed { consumed, .. }) = code_in_prose(text) {
        return Some(consumed);
    }

    if let Some(after_open) = text.strip_prefix("@{")
        && let Some(end) = after_open.find('}')
    {
        return Some(2 + end + 1);
    }

    if let Some(consumed) = inline_math_span(text) {
        return Some(consumed);
    }

    if let Some(after_open) = text.strip_prefix('[')
        && let Some(label_end) = after_open.find("](")
    {
        let after_target_open = &after_open[label_end + 2..];
        if let Some(target_end) = after_target_open.find(')') {
            return Some(1 + label_end + 2 + target_end + 1);
        }
    }

    None
}

// Collect inline source-code spans
pub(crate) fn source_code_spans(text: &str) -> Vec<String> {
    let mut spans = Vec::new();
    collect_source_code_spans(text, &mut spans);
    spans
}

fn collect_source_code_spans(text: &str, spans: &mut Vec<String>) {
    let mut position = 0;
    while position < text.len() {
        let rest = &text[position..];

        if let Some(after_open) = rest.strip_prefix("@{")
            && let Some(end) = after_open.find('}')
        {
            position += 2 + end + 1;
            continue;
        }

        if let Some(after_open) = rest.strip_prefix('`')
            && let Some(end) = after_open.find('`')
        {
            position += 1 + end + 1;
            continue;
        }

        if let Some(consumed) = inline_math_span(rest) {
            position += consumed;
            continue;
        }

        if let Some(code) = code_in_prose(rest) {
            match code {
                CodeInProse::Closed { text, consumed } => {
                    spans.push(text);
                    position += consumed;
                    continue;
                }
                CodeInProse::Unclosed => break,
            }
        }

        if let Some(after_open) = rest.strip_prefix("**")
            && let Some(end) = after_open.find("**")
        {
            collect_source_code_spans(&after_open[..end], spans);
            position += 2 + end + 2;
            continue;
        }

        if let Some(after_open) = rest.strip_prefix('*')
            && let Some(end) = after_open.find('*')
        {
            collect_source_code_spans(&after_open[..end], spans);
            position += 1 + end + 1;
            continue;
        }

        if let Some(after_open) = rest.strip_prefix('[')
            && let Some(label_end) = after_open.find("](")
        {
            let after_target_open = &after_open[label_end + 2..];
            if let Some(target_end) = after_target_open.find(')') {
                collect_source_code_spans(&after_open[..label_end], spans);
                position += 1 + label_end + 2 + target_end + 1;
                continue;
            }
        }

        if let Some(after_escape) = rest.strip_prefix('\\')
            && let Some(character) = after_escape.chars().next()
        {
            position += 1 + character.len_utf8();
            continue;
        }

        let character = rest
            .chars()
            .next()
            .expect("position is on a character boundary");
        position += character.len_utf8();
    }
}
