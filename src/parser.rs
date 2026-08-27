// src/parser.rs
//! Data structures and parsing for `.lit` input.

// Parser imports
use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use crate::inline::{fence_closes, fence_opening, table_candidate, table_row};

// Parser source model
// Source origins
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceOrigin {
    pub file: String,
    pub line: usize,
}

impl SourceOrigin {
    pub fn new(file: impl Into<String>, line: usize) -> Self {
        Self {
            file: file.into(),
            line,
        }
    }
}

impl fmt::Display for SourceOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

// Located source lines
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub origin: SourceOrigin,
    pub text: String,
}

impl Line {
    fn new(text: impl Into<String>, file: &str, line: usize) -> Self {
        Self {
            origin: SourceOrigin::new(file, line),
            text: text.into(),
        }
    }
}

// Commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    CodeType,
    CommentType,
    ColorScheme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub kind: CommandKind,
    pub name: String,
    pub arguments: String,
    pub origin: SourceOrigin,
}

// Code-block modifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Additive,
    Redefinition,
    NoWeave,
    NoHeader,
}

// Blocks
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    pub name: String,
    pub quoted_name: bool,
    pub modifiers: Vec<Modifier>,
    pub code_type: String,
    pub comment_string: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Prose,
    Code(CodeBlock),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub origin: SourceOrigin,
    pub kind: BlockKind,
    pub lines: Vec<Line>,
}

impl Block {
    pub fn is_code(&self) -> bool {
        matches!(self.kind, BlockKind::Code(_))
    }

    pub fn code(&self) -> Option<&CodeBlock> {
        match &self.kind {
            BlockKind::Prose => None,
            BlockKind::Code(code) => Some(code),
        }
    }

    pub fn text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(&line.text);
            text.push('\n');
        }
        text
    }
}

// Sections
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub origin: SourceOrigin,
    pub title: String,
    pub commands: Vec<Command>,
    pub blocks: Vec<Block>,
    pub number: usize,
}

// Chapters
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub origin: SourceOrigin,
    pub title: String,
    pub title_origin: Option<SourceOrigin>,
    pub commands: Vec<Command>,
    pub sections: Vec<Section>,
    pub file: String,
    pub major_number: usize,
    pub minor_number: usize,
    pub book: Option<BookChapter>,
}

impl Chapter {
    pub fn number(&self) -> String {
        if self.minor_number == 0 {
            self.major_number.to_string()
        } else {
            format!("{}.{}", self.major_number, self.minor_number)
        }
    }
}

// Book chapter metadata
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookChapter {
    pub navigation_label: String,
    pub label_origin: SourceOrigin,
    pub source_path: PathBuf,
}

// Book metadata
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookMetadata {
    pub introduction: String,
    pub introduction_lines: Vec<Line>,
}

// Programs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramKind {
    Chapter,
    Book(BookMetadata),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub kind: ProgramKind,
    pub origin: SourceOrigin,
    pub title: String,
    pub title_origin: Option<SourceOrigin>,
    pub commands: Vec<Command>,
    pub chapters: Vec<Chapter>,
    pub file: String,
    pub text: String,
}

impl Program {
    pub fn is_book(&self) -> bool {
        matches!(&self.kind, ProgramKind::Book(_))
    }

    pub fn book(&self) -> Option<&BookMetadata> {
        match &self.kind {
            ProgramKind::Chapter => None,
            ProgramKind::Book(book) => Some(book),
        }
    }
}

// Parser diagnostics and entry points
// Parse error model
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub origin: SourceOrigin,
    pub kind: ParseErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    InvalidUtf8 { valid_up_to: usize },
    UnsupportedCommand { name: String },
    MissingCommandArguments { name: String },
    PageCommandAfterSection { name: String },
    CodeBeforeSection,
    EmptyBlockName,
    MalformedQuotedName { name: String },
    InvalidModifier { name: String },
    UnsupportedModifier { name: String },
    DuplicateModifier { name: String },
    ConflictingModifiers,
    DuplicateDefinition { name: String },
    MalformedClosingDelimiter { text: String },
    UnclosedBlock { name: String },
}

// Render one parse error
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: error: ", self.origin)?;
        match &self.kind {
            ParseErrorKind::InvalidUtf8 { valid_up_to } => {
                write!(f, "input is not UTF-8 at byte {valid_up_to}")
            }
            ParseErrorKind::UnsupportedCommand { name } => {
                write!(f, "command {name} is not supported yet")
            }
            ParseErrorKind::MissingCommandArguments { name } => {
                write!(f, "command {name} requires arguments")
            }
            ParseErrorKind::PageCommandAfterSection { name } => {
                write!(
                    f,
                    "page-level command {name} must appear before the first @s"
                )
            }
            ParseErrorKind::CodeBeforeSection => {
                f.write_str("a section must be defined with @s before a code block")
            }
            ParseErrorKind::EmptyBlockName => f.write_str("code block name is empty"),
            ParseErrorKind::MalformedQuotedName { name } => {
                write!(f, "malformed quoted code block name: {name}")
            }
            ParseErrorKind::InvalidModifier { name } => {
                write!(f, "invalid code block modifier: {name}")
            }
            ParseErrorKind::UnsupportedModifier { name } => {
                write!(f, "code block modifier {name} is not supported yet")
            }
            ParseErrorKind::DuplicateModifier { name } => {
                write!(f, "duplicate code block modifier: {name}")
            }
            ParseErrorKind::ConflictingModifiers => {
                f.write_str("a code block cannot use both += and :=")
            }
            ParseErrorKind::DuplicateDefinition { name } => {
                write!(f, "redefinition of {{{name}}}; use := to redefine it")
            }
            ParseErrorKind::MalformedClosingDelimiter { text } => {
                write!(f, "a code block must be closed by --- alone, not {text}")
            }
            ParseErrorKind::UnclosedBlock { name } => {
                write!(f, "code block {{{name}}} is never closed")
            }
        }
    }
}

impl std::error::Error for ParseError {}

// Parse error collection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseErrors {
    errors: Vec<ParseError>,
}

impl ParseErrors {
    pub fn as_slice(&self) -> &[ParseError] {
        &self.errors
    }

    pub fn into_vec(self) -> Vec<ParseError> {
        self.errors
    }
}

impl fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.errors.first() {
            write!(f, "{first}")?;
            if self.errors.len() > 1 {
                write!(f, " (and {} more parse errors)", self.errors.len() - 1)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ParseErrors {}

// Single-file parse entry points
pub fn parse_bytes(
    file: impl Into<String>,
    source: &[u8],
) -> Result<Program, ParseErrors> {
    let file = file.into();
    match std::str::from_utf8(source) {
        Ok(source) => parse_source(file, source),
        Err(error) => Err(invalid_utf8_error(file, source, error.valid_up_to())),
    }
}

pub fn parse_str(file: impl Into<String>, source: &str) -> Result<Program, ParseErrors> {
    parse_source(file.into(), source)
}

fn parse_source(file: String, source: &str) -> Result<Program, ParseErrors> {
    let chapter = parse_chapter_source(file.clone(), source, Vec::new(), 1, 0, None)?;
    let title = chapter.title.clone();
    let title_origin = chapter.title_origin.clone();
    let commands = chapter.commands.clone();
    Ok(Program {
        kind: ProgramKind::Chapter,
        origin: SourceOrigin::new(&file, 1),
        title,
        title_origin,
        commands,
        chapters: vec![chapter],
        file,
        text: source.to_owned(),
    })
}

// Parse a loaded book chapter
pub(crate) fn parse_chapter_bytes(
    file: String,
    source: &[u8],
    inherited_commands: Vec<Command>,
    major_number: usize,
    minor_number: usize,
    book: BookChapter,
) -> Result<Chapter, ParseErrors> {
    match std::str::from_utf8(source) {
        Ok(source) => parse_chapter_source(
            file,
            source,
            inherited_commands,
            major_number,
            minor_number,
            Some(book),
        ),
        Err(error) => Err(invalid_utf8_error(file, source, error.valid_up_to())),
    }
}

// Parse one chapter from located text
fn parse_chapter_source(
    file: String,
    source: &str,
    inherited_commands: Vec<Command>,
    major_number: usize,
    minor_number: usize,
    book: Option<BookChapter>,
) -> Result<Chapter, ParseErrors> {
    let lines = source
        .split('\n')
        .enumerate()
        .map(|(index, text)| {
            let text = text.strip_suffix('\r').unwrap_or(text);
            Line::new(text, &file, index + 1)
        })
        .collect();

    Parser::new(
        file,
        lines,
        inherited_commands,
        major_number,
        minor_number,
        book,
    )
    .parse()
}

// Report invalid UTF-8
fn invalid_utf8_error(file: String, source: &[u8], valid_up_to: usize) -> ParseErrors {
    let line = source[..valid_up_to]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    ParseErrors {
        errors: vec![ParseError {
            origin: SourceOrigin::new(file, line),
            kind: ParseErrorKind::InvalidUtf8 { valid_up_to },
        }],
    }
}

// Parser state and recognition
// Parser state
struct Parser {
    lines: Vec<Line>,
    chapter: Chapter,
    inherited_commands: Vec<Command>,
    current_section: Option<Section>,
    current_block: Option<Block>,
    defined_names: HashSet<String>,
    errors: Vec<ParseError>,
}

impl Parser {
    fn new(
        file: String,
        lines: Vec<Line>,
        inherited_commands: Vec<Command>,
        major_number: usize,
        minor_number: usize,
        book: Option<BookChapter>,
    ) -> Self {
        Self {
            chapter: Chapter {
                origin: SourceOrigin::new(&file, 1),
                title: String::new(),
                title_origin: None,
                commands: Vec::new(),
                sections: Vec::new(),
                file: file.clone(),
                major_number,
                minor_number,
                book,
            },
            lines,
            inherited_commands,
            current_section: None,
            current_block: None,
            defined_names: HashSet::new(),
            errors: Vec::new(),
        }
    }
}

// Parser implementation
impl Parser {
    // Parse and dispatch source lines
    fn parse(mut self) -> Result<Chapter, ParseErrors> {
        let mut index = 0;
        while index < self.lines.len() {
            if self.retain_literal_prose(&mut index) {
                continue;
            }
            let line = self.lines[index].clone();
            self.parse_line(line);
            index += 1;
        }
        self.finish()
    }

    fn parse_line(&mut self, line: Line) {
        if self.current_block.as_ref().is_some_and(Block::is_code) {
            if line.text == "---" {
                self.close_code_block(line);
            } else if resembles_delimiter(&line.text) {
                self.error(
                    line.origin.clone(),
                    ParseErrorKind::MalformedClosingDelimiter {
                        text: line.text.clone(),
                    },
                );
                self.close_code_block(line);
            } else if let Some(block) = &mut self.current_block {
                block.lines.push(line);
            }
            return;
        }

        if line.text.trim().starts_with("//") {
            return;
        }

        if let Some((name, arguments)) = command_parts(&line.text) {
            match name {
                "@title" => {
                    self.chapter.title = arguments.to_owned();
                    self.chapter.title_origin = Some(line.origin);
                    return;
                }
                "@s" => {
                    self.begin_section(line.origin, arguments.to_owned());
                    return;
                }
                "@code_type" => {
                    self.add_command(line.origin, CommandKind::CodeType, name, arguments);
                    return;
                }
                "@comment_type" => {
                    self.add_command(
                        line.origin,
                        CommandKind::CommentType,
                        name,
                        arguments,
                    );
                    return;
                }
                "@colorscheme" => {
                    self.add_command(
                        line.origin,
                        CommandKind::ColorScheme,
                        name,
                        arguments,
                    );
                    return;
                }
                name if is_deferred_command(name) => {
                    self.error(
                        line.origin,
                        ParseErrorKind::UnsupportedCommand {
                            name: name.to_owned(),
                        },
                    );
                    return;
                }
                _ => {}
            }
        }

        if let Some(opening) = opening_text(&line.text).map(str::to_owned) {
            self.begin_code_block(line, opening);
        } else if let Some(block) = &mut self.current_block {
            block.lines.push(line);
        }
    }

    // Protect literal prose structures
    fn retain_literal_prose(&mut self, index: &mut usize) -> bool {
        let range = if let Some(close) = self.prose_fence_close(*index) {
            *index..close + 1
        } else if let Some(end) = self.prose_table_end(*index) {
            *index..end
        } else {
            return false;
        };

        self.current_block
            .as_mut()
            .expect("literal prose belongs to the current prose block")
            .lines
            .extend_from_slice(&self.lines[range.clone()]);
        *index = range.end;
        true
    }

    fn prose_fence_close(&self, start: usize) -> Option<usize> {
        let block = self.current_block.as_ref()?;
        if block.is_code() {
            return None;
        }
        let opening = fence_opening(&self.lines[start].text)?;
        (start + 1..self.lines.len())
            .find(|line| fence_closes(&self.lines[*line].text, &opening))
    }

    fn prose_table_end(&self, start: usize) -> Option<usize> {
        let block = self.current_block.as_ref()?;
        if block.is_code() {
            return None;
        }
        table_candidate(
            &self.lines.get(start)?.text,
            &self.lines.get(start + 1)?.text,
        )
        .then_some(())?;

        let mut end = start + 2;
        while end < self.lines.len()
            && !self.lines[end].text.trim().is_empty()
            && table_row(&self.lines[end].text).is_some()
        {
            end += 1;
        }
        Some(end)
    }

    // Store a configuration command
    fn add_command(
        &mut self,
        origin: SourceOrigin,
        kind: CommandKind,
        name: &str,
        arguments: &str,
    ) {
        if arguments.is_empty() {
            self.error(
                origin,
                ParseErrorKind::MissingCommandArguments {
                    name: name.to_owned(),
                },
            );
            return;
        }

        if kind == CommandKind::ColorScheme && self.current_section.is_some() {
            self.error(
                origin,
                ParseErrorKind::PageCommandAfterSection {
                    name: name.to_owned(),
                },
            );
            return;
        }

        let command = Command {
            kind,
            name: name.to_owned(),
            arguments: if arguments == "none" {
                String::new()
            } else {
                arguments.to_owned()
            },
            origin,
        };

        if let Some(section) = &mut self.current_section {
            section.commands.push(command);
        } else {
            self.chapter.commands.push(command);
        }
    }

    // Begin a semantic section
    fn begin_section(&mut self, origin: SourceOrigin, title: String) {
        self.close_section();
        let number = self.chapter.sections.len() + 1;
        let mut commands = self.inherited_commands.clone();
        commands.extend(self.chapter.commands.clone());
        self.current_section = Some(Section {
            origin: origin.clone(),
            title,
            commands,
            blocks: Vec::new(),
            number,
        });
        self.current_block = Some(Block {
            origin,
            kind: BlockKind::Prose,
            lines: Vec::new(),
        });
    }

    // Enter and leave a code block
    fn begin_code_block(&mut self, line: Line, opening: String) {
        if self.current_section.is_none() {
            self.error(line.origin, ParseErrorKind::CodeBeforeSection);
            return;
        }

        let code = parse_block_opening(&opening, &line.origin, &mut self.errors);
        let source_name = if code.quoted_name {
            format!("\"{}\"", code.name)
        } else {
            code.name.clone()
        };
        let changes_existing = code.modifiers.contains(&Modifier::Additive)
            || code.modifiers.contains(&Modifier::Redefinition);
        if !self.defined_names.insert(source_name.clone()) && !changes_existing {
            self.error(
                line.origin.clone(),
                ParseErrorKind::DuplicateDefinition { name: source_name },
            );
        }

        let section = self
            .current_section
            .as_mut()
            .expect("section checked above");
        if let Some(block) = self.current_block.take() {
            section.blocks.push(block);
        }

        let code_type = effective_command(&section.commands, CommandKind::CodeType);
        let comment_string =
            effective_command(&section.commands, CommandKind::CommentType);
        self.current_block = Some(Block {
            origin: line.origin,
            kind: BlockKind::Code(CodeBlock {
                code_type,
                comment_string,
                ..code
            }),
            lines: Vec::new(),
        });
    }

    fn close_code_block(&mut self, line: Line) {
        let section = self
            .current_section
            .as_mut()
            .expect("a code block always belongs to a section");
        if let Some(block) = self.current_block.take() {
            section.blocks.push(block);
        }
        self.current_block = Some(Block {
            origin: line.origin,
            kind: BlockKind::Prose,
            lines: Vec::new(),
        });
    }

    // Close and finish parser state
    fn close_section(&mut self) {
        let Some(mut section) = self.current_section.take() else {
            return;
        };
        if let Some(block) = self.current_block.take() {
            if block.is_code() {
                let name = block
                    .code()
                    .map(|code| code.name.clone())
                    .unwrap_or_default();
                self.error(block.origin, ParseErrorKind::UnclosedBlock { name });
            } else if block.lines.iter().any(|line| !line.text.trim().is_empty()) {
                section.blocks.push(block);
            }
        }
        self.chapter.sections.push(section);
    }

    fn finish(mut self) -> Result<Chapter, ParseErrors> {
        self.close_section();

        if !self.errors.is_empty() {
            return Err(ParseErrors {
                errors: self.errors,
            });
        }

        if self.chapter.title.is_empty()
            && let Some(book) = &self.chapter.book
        {
            self.chapter.title = book.navigation_label.clone();
            self.chapter.title_origin = Some(book.label_origin.clone());
        }
        Ok(self.chapter)
    }

    fn error(&mut self, origin: SourceOrigin, kind: ParseErrorKind) {
        self.errors.push(ParseError { origin, kind });
    }
}

// Find the effective command
fn effective_command(commands: &[Command], kind: CommandKind) -> String {
    commands
        .iter()
        .rev()
        .find(|command| command.kind == kind)
        .map(|command| command.arguments.clone())
        .unwrap_or_default()
}

// Command recognition
pub(crate) fn command_parts(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with('@') {
        return None;
    }
    let boundary = line.find(char::is_whitespace).unwrap_or(line.len());
    Some((&line[..boundary], line[boundary..].trim()))
}

pub(crate) fn is_deferred_command(name: &str) -> bool {
    matches!(
        name,
        "@book"
            | "@include"
            | "@change"
            | "@change_end"
            | "@replace"
            | "@with"
            | "@end"
            | "@compiler"
            | "@error_format"
            | "@add_css"
            | "@overwrite_css"
    )
}

// Code delimiter recognition
fn opening_text(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("---")?;
    rest.starts_with([' ', '\t']).then_some(rest.trim())
}

fn resembles_delimiter(line: &str) -> bool {
    line.strip_prefix("---")
        .is_some_and(|rest| rest.starts_with([' ', '\t']))
}

// Read a block opening
fn parse_block_opening(
    opening: &str,
    origin: &SourceOrigin,
    errors: &mut Vec<ParseError>,
) -> CodeBlock {
    let mut name = opening.trim();
    let mut modifier_text = None;

    if let Some(index) = separated_modifier_index(name) {
        modifier_text = Some(name[index + 3..].trim());
        name = name[..index].trim();
    } else if let Some(prefix) = name.strip_suffix("---")
        && prefix.ends_with([' ', '\t'])
    {
        name = prefix.trim_end();
    }

    let mut modifiers = Vec::new();
    if let Some(prefix) = compact_modifier_prefix(name, "+=") {
        push_modifier(&mut modifiers, Modifier::Additive, "+=", origin, errors);
        name = prefix;
    } else if let Some(prefix) = compact_modifier_prefix(name, ":=") {
        push_modifier(&mut modifiers, Modifier::Redefinition, ":=", origin, errors);
        name = prefix;
    }

    if let Some(modifier_text) = modifier_text {
        for modifier in modifier_text.split_whitespace() {
            match modifier {
                "+=" => push_modifier(
                    &mut modifiers,
                    Modifier::Additive,
                    modifier,
                    origin,
                    errors,
                ),
                ":=" => push_modifier(
                    &mut modifiers,
                    Modifier::Redefinition,
                    modifier,
                    origin,
                    errors,
                ),
                "noWeave" => push_modifier(
                    &mut modifiers,
                    Modifier::NoWeave,
                    modifier,
                    origin,
                    errors,
                ),
                "noHeader" => push_modifier(
                    &mut modifiers,
                    Modifier::NoHeader,
                    modifier,
                    origin,
                    errors,
                ),
                "noTangle" => errors.push(ParseError {
                    origin: origin.clone(),
                    kind: ParseErrorKind::UnsupportedModifier {
                        name: modifier.to_owned(),
                    },
                }),
                _ => errors.push(ParseError {
                    origin: origin.clone(),
                    kind: ParseErrorKind::InvalidModifier {
                        name: modifier.to_owned(),
                    },
                }),
            }
        }
    }

    if modifiers.contains(&Modifier::Additive)
        && modifiers.contains(&Modifier::Redefinition)
    {
        errors.push(ParseError {
            origin: origin.clone(),
            kind: ParseErrorKind::ConflictingModifiers,
        });
    }

    let (name, quoted_name) = parse_block_name(name, origin, errors);
    CodeBlock {
        name,
        quoted_name,
        modifiers,
        code_type: String::new(),
        comment_string: String::new(),
    }
}

// Recognize and record block modifiers
fn separated_modifier_index(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    text.match_indices("---")
        .map(|(index, _)| index)
        .filter(|index| {
            *index > 0
                && index + 3 < bytes.len()
                && matches!(bytes[index - 1], b' ' | b'\t')
                && matches!(bytes[index + 3], b' ' | b'\t')
        })
        .last()
}

fn compact_modifier_prefix<'a>(name: &'a str, modifier: &str) -> Option<&'a str> {
    let prefix = name.strip_suffix(modifier)?;
    prefix.ends_with([' ', '\t']).then(|| prefix.trim_end())
}

fn push_modifier(
    modifiers: &mut Vec<Modifier>,
    modifier: Modifier,
    name: &str,
    origin: &SourceOrigin,
    errors: &mut Vec<ParseError>,
) {
    if modifiers.contains(&modifier) {
        errors.push(ParseError {
            origin: origin.clone(),
            kind: ParseErrorKind::DuplicateModifier {
                name: name.to_owned(),
            },
        });
    } else {
        modifiers.push(modifier);
    }
}

// Read a block name
fn parse_block_name(
    name: &str,
    origin: &SourceOrigin,
    errors: &mut Vec<ParseError>,
) -> (String, bool) {
    if name.is_empty() {
        errors.push(ParseError {
            origin: origin.clone(),
            kind: ParseErrorKind::EmptyBlockName,
        });
        return (String::new(), false);
    }

    let begins_quote = name.starts_with('"');
    let ends_quote = name.ends_with('"');
    if begins_quote != ends_quote || (begins_quote && name.len() < 2) {
        errors.push(ParseError {
            origin: origin.clone(),
            kind: ParseErrorKind::MalformedQuotedName {
                name: name.to_owned(),
            },
        });
        return (name.to_owned(), false);
    }

    if begins_quote {
        let inner = &name[1..name.len() - 1];
        if inner.is_empty() {
            errors.push(ParseError {
                origin: origin.clone(),
                kind: ParseErrorKind::EmptyBlockName,
            });
        }
        (inner.to_owned(), true)
    } else {
        (name.to_owned(), false)
    }
}
