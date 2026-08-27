// src/identifier.rs
//! Fragment-tolerant target-language identifier analysis.

// Identifier imports
use std::collections::{HashMap, HashSet};

use unicode_ident::{is_xid_continue, is_xid_start};

use crate::inline::{fenced_line_mask, source_code_spans};
use crate::parser::{
    Block, BlockKind, Command, CommandKind, Line, Modifier, Program, SourceOrigin,
};
use crate::resolver::{BlockLookup, ResolvedProgram, resolve};
use crate::util::{block_reference, leading_whitespace};

// Identifier model and collection
// Identifier index model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdentifierRole {
    Definition,
    Use,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdentifierNamespace {
    Ordinary,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdentifierKind {
    Struct,
    Enum,
    Union,
    Trait,
    TypeAlias,
    Function,
    Constant,
    Static,
    Module,
    Macro,
    Field,
    Variant,
    GenericParameter,
    Parameter,
    LocalVariable,
    Import,
}

impl IdentifierKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Union => "union",
            Self::Trait => "trait",
            Self::TypeAlias => "type alias",
            Self::Function => "function",
            Self::Constant => "constant",
            Self::Static => "static variable",
            Self::Module => "module",
            Self::Macro => "macro",
            Self::Field => "field",
            Self::Variant => "variant",
            Self::GenericParameter => "generic parameter",
            Self::Parameter => "parameter",
            Self::LocalVariable => "local variable",
            Self::Import => "import",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentifierSite {
    pub chapter: usize,
    pub section: usize,
    pub block: usize,
    pub origin: SourceOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierOccurrence {
    pub role: IdentifierRole,
    pub namespace: IdentifierNamespace,
    pub site: IdentifierSite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierEntry {
    pub name: String,
    pub occurrences: Vec<IdentifierOccurrence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierMeaning {
    pub id: usize,
    pub name: String,
    pub namespace: IdentifierNamespace,
    pub kind: IdentifierKind,
    pub definition: IdentifierSite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierMeaningOccurrence {
    pub meaning: usize,
    pub role: IdentifierRole,
    pub site: IdentifierSite,
    pub column: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentifierIndex {
    entries: Vec<IdentifierEntry>,
    meanings: Vec<IdentifierMeaning>,
    meaning_occurrences: Vec<IdentifierMeaningOccurrence>,
}

impl IdentifierIndex {
    pub fn entries(&self) -> &[IdentifierEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn meanings(&self) -> &[IdentifierMeaning] {
        &self.meanings
    }

    pub fn meaning_occurrences(&self) -> &[IdentifierMeaningOccurrence] {
        &self.meaning_occurrences
    }
}

// Private identifier records
struct Candidate {
    name: String,
    role: IdentifierRole,
    namespace: IdentifierNamespace,
    type_candidate: bool,
    use_shape: UseShape,
    definition_kind: Option<IdentifierKind>,
    site: IdentifierSite,
    column: usize,
}

struct ClassifiedWord {
    name: String,
    offset: usize,
    role: IdentifierRole,
    namespace: IdentifierNamespace,
    type_candidate: bool,
    use_shape: UseShape,
    definition_kind: Option<IdentifierKind>,
}

#[derive(Debug)]
struct RustToken {
    kind: RustTokenKind,
    offset: usize,
}

#[derive(Debug)]
enum RustTokenKind {
    Word { name: String, raw: bool },
    Symbol(&'static str),
    Opaque,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum UseShape {
    #[default]
    None,
    Field,
    Method,
    Macro,
    Ambiguous,
}

impl UseShape {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, value) | (value, Self::None) => value,
            (left, right) if left == right => left,
            _ => Self::Ambiguous,
        }
    }
}

#[derive(Clone)]
struct CodeLineSource {
    site: IdentifierSite,
    eligible: bool,
}

struct FragmentLine {
    output_start: usize,
    text_start: usize,
    text_end: usize,
    source: Option<CodeLineSource>,
}

struct SourceToken {
    source: CodeLineSource,
    column: usize,
}

struct ContextFrame {
    block: usize,
    next_line: usize,
    indentation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SourceTokenKey {
    site: IdentifierSite,
    column: usize,
    name: String,
}

#[derive(Default)]
struct TokenObservations {
    by_source: HashMap<SourceTokenKey, usize>,
    tokens: Vec<ObservedToken>,
}

struct ObservedToken {
    name: String,
    site: IdentifierSite,
    column: usize,
    evidence: TokenEvidence,
}

#[derive(Default)]
struct TokenEvidence {
    ordinary_definition: bool,
    type_definition: bool,
    ordinary_use: bool,
    type_use: bool,
    possible_type_use: bool,
    use_shape: UseShape,
    definition_kind: Option<IdentifierKind>,
    conflicting_definition_kinds: bool,
}

// Analyze program identifiers
pub fn analyze(program: &Program) -> IdentifierIndex {
    match resolve(program) {
        Ok(resolved) => analyze_with_context(program, Some(&resolved)),
        Err(_) => analyze_with_context(program, None),
    }
}

pub(crate) fn analyze_resolved(
    program: &Program,
    resolved: &ResolvedProgram,
) -> IdentifierIndex {
    analyze_with_context(program, Some(resolved))
}

fn analyze_with_context(
    program: &Program,
    resolved: Option<&ResolvedProgram>,
) -> IdentifierIndex {
    let line_sources = code_line_sources(program);
    let mut observations = TokenObservations::default();
    let contextualized = resolved.map_or_else(HashSet::new, |resolved| {
        collect_contextual_observations(resolved, &line_sources, &mut observations)
    });
    collect_local_observations(
        program,
        &line_sources,
        &contextualized,
        &mut observations,
    );

    let mut declared_types = vec![HashSet::new(); program.chapters.len()];
    let mut ordinary_definitions = vec![HashSet::new(); program.chapters.len()];
    let mut candidates =
        observations.into_candidates(&mut declared_types, &mut ordinary_definitions);
    collect_prose_candidates(program, &mut candidates);
    candidates.sort_by(|left, right| {
        left.site
            .chapter
            .cmp(&right.site.chapter)
            .then_with(|| left.site.section.cmp(&right.site.section))
            .then_with(|| left.site.block.cmp(&right.site.block))
            .then_with(|| left.site.origin.file.cmp(&right.site.origin.file))
            .then_with(|| left.site.origin.line.cmp(&right.site.origin.line))
            .then_with(|| left.column.cmp(&right.column))
    });

    finish_index(candidates, &declared_types, &ordinary_definitions)
}

fn is_rust(code_type: &str) -> bool {
    code_type.split_whitespace().next() == Some("rust")
}

// Inventory code-line sources
fn code_line_sources(program: &Program) -> HashMap<SourceOrigin, CodeLineSource> {
    let mut sources = HashMap::new();
    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        for (section_index, section) in chapter.sections.iter().enumerate() {
            for (block_index, block) in section.blocks.iter().enumerate() {
                let BlockKind::Code(code) = &block.kind else {
                    continue;
                };
                let eligible = is_rust(&code.code_type)
                    && !code.modifiers.contains(&Modifier::NoWeave);
                for line in &block.lines {
                    sources.entry(line.origin.clone()).or_insert_with(|| {
                        CodeLineSource {
                            site: IdentifierSite {
                                chapter: chapter_index,
                                section: section_index,
                                block: block_index,
                                origin: line.origin.clone(),
                            },
                            eligible,
                        }
                    });
                }
            }
        }
    }
    sources
}

// Build contextual Rust fragments
fn collect_contextual_observations(
    resolved: &ResolvedProgram,
    line_sources: &HashMap<SourceOrigin, CodeLineSource>,
    observations: &mut TokenObservations,
) -> HashSet<SourceOrigin> {
    let mut referenced = vec![false; resolved.blocks.len()];
    for block in &resolved.blocks {
        for (line_index, line) in block.lines.iter().enumerate() {
            let Some(name) = block_reference(&line.text) else {
                continue;
            };
            if let BlockLookup::Found(target) =
                resolved.lookup(block.line_chapters[line_index], name)
            {
                referenced[target] = true;
            }
        }
    }

    let mut covered_blocks = vec![false; resolved.blocks.len()];
    let mut contextualized = HashSet::new();
    for root in (0..resolved.blocks.len()).filter(|index| !referenced[*index]) {
        let fragment = contextual_fragment(
            resolved,
            root,
            line_sources,
            &mut covered_blocks,
            &mut contextualized,
        );
        observations.collect(&fragment, &HashSet::new());
    }
    for root in 0..resolved.blocks.len() {
        if covered_blocks[root] {
            continue;
        }
        let fragment = contextual_fragment(
            resolved,
            root,
            line_sources,
            &mut covered_blocks,
            &mut contextualized,
        );
        observations.collect(&fragment, &HashSet::new());
    }
    contextualized
}

fn contextual_fragment(
    resolved: &ResolvedProgram,
    root: usize,
    line_sources: &HashMap<SourceOrigin, CodeLineSource>,
    covered_blocks: &mut [bool],
    contextualized: &mut HashSet<SourceOrigin>,
) -> SourceFragment {
    let mut fragment = SourceFragment::default();
    let mut active = vec![false; resolved.blocks.len()];
    active[root] = true;
    covered_blocks[root] = true;
    let mut frames = vec![ContextFrame {
        block: root,
        next_line: 0,
        indentation: String::new(),
    }];

    while !frames.is_empty() {
        let depth = frames.len() - 1;
        let block_index = frames[depth].block;
        let block = resolved.block(block_index);
        if frames[depth].next_line == block.lines.len() {
            active[block_index] = false;
            frames.pop();
            continue;
        }

        let line_index = frames[depth].next_line;
        let line = &block.lines[line_index];
        let line_chapter = block.line_chapters[line_index];
        frames[depth].next_line += 1;

        if let Some(name) = block_reference(&line.text) {
            let BlockLookup::Found(target) = resolved.lookup(line_chapter, name) else {
                continue;
            };
            if active[target] {
                continue;
            }
            active[target] = true;
            covered_blocks[target] = true;
            frames.push(ContextFrame {
                block: target,
                next_line: 0,
                indentation: frames[depth].indentation.clone()
                    + leading_whitespace(&line.text),
            });
        } else {
            contextualized.insert(line.origin.clone());
            fragment.push_line(
                &frames[depth].indentation,
                line,
                line_sources.get(&line.origin),
            );
        }
    }
    fragment
}

// Find a prose line's code type
fn effective_code_type<'a>(commands: &'a [Command], origin: &SourceOrigin) -> &'a str {
    let mut value = "";
    for command in commands {
        if command.kind == CommandKind::CodeType
            && (command.origin.file != origin.file || command.origin.line < origin.line)
        {
            value = &command.arguments;
        }
    }
    value
}

// Source fragment
#[derive(Default)]
struct SourceFragment {
    text: String,
    lines: Vec<FragmentLine>,
}

impl SourceFragment {
    fn push_line(
        &mut self,
        indentation: &str,
        line: &Line,
        source: Option<&CodeLineSource>,
    ) {
        let output_start = self.text.len();
        self.text.push_str(indentation);
        let text_start = self.text.len();
        self.text.push_str(&line.text);
        let text_end = self.text.len();
        self.text.push('\n');
        self.lines.push(FragmentLine {
            output_start,
            text_start,
            text_end,
            source: source.cloned(),
        });
    }

    fn push_reference_line(&mut self) {
        self.text.push('\n');
    }

    fn source_at(&self, offset: usize) -> Option<SourceToken> {
        let index = self
            .lines
            .partition_point(|line| line.output_start <= offset)
            .checked_sub(1)?;
        let line = &self.lines[index];
        if offset < line.text_start || offset >= line.text_end {
            return None;
        }
        Some(SourceToken {
            source: line.source.clone()?,
            column: offset - line.text_start,
        })
    }
}

fn visible_code_fragment(
    block: &Block,
    line_sources: &HashMap<SourceOrigin, CodeLineSource>,
) -> SourceFragment {
    let mut fragment = SourceFragment::default();
    for line in &block.lines {
        if block_reference(&line.text).is_some() {
            fragment.push_reference_line();
        } else {
            fragment.push_line("", line, line_sources.get(&line.origin));
        }
    }
    fragment
}

// Collect one fragment
fn collect_local_observations(
    program: &Program,
    line_sources: &HashMap<SourceOrigin, CodeLineSource>,
    contextualized: &HashSet<SourceOrigin>,
    observations: &mut TokenObservations,
) {
    for chapter in &program.chapters {
        for section in &chapter.sections {
            for block in &section.blocks {
                let BlockKind::Code(code) = &block.kind else {
                    continue;
                };
                if is_rust(&code.code_type)
                    && !code.modifiers.contains(&Modifier::NoWeave)
                {
                    observations.collect(
                        &visible_code_fragment(block, line_sources),
                        contextualized,
                    );
                }
            }
        }
    }
}

fn collect_prose_candidates(program: &Program, candidates: &mut Vec<Candidate>) {
    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        for (section_index, section) in chapter.sections.iter().enumerate() {
            for (block_index, block) in section.blocks.iter().enumerate() {
                if !matches!(block.kind, BlockKind::Prose) {
                    continue;
                }
                let prose_lines: Vec<_> =
                    block.lines.iter().map(|line| line.text.as_str()).collect();
                let fenced_lines = fenced_line_mask(&prose_lines);
                for (line, fenced) in block.lines.iter().zip(fenced_lines) {
                    if fenced
                        || !is_rust(effective_code_type(&section.commands, &line.origin))
                    {
                        continue;
                    }
                    let mut column = 0;
                    for span in source_code_spans(&line.text) {
                        for word in classify_rust(&span) {
                            candidates.push(Candidate {
                                name: word.name.clone(),
                                role: IdentifierRole::Use,
                                namespace: word.namespace,
                                type_candidate: word.type_candidate
                                    || word
                                        .name
                                        .chars()
                                        .next()
                                        .is_some_and(char::is_uppercase),
                                use_shape: word.use_shape,
                                definition_kind: None,
                                site: IdentifierSite {
                                    chapter: chapter_index,
                                    section: section_index,
                                    block: block_index,
                                    origin: line.origin.clone(),
                                },
                                column,
                            });
                            column += 1;
                        }
                    }
                }
            }
        }
    }
}

impl TokenObservations {
    fn collect(&mut self, fragment: &SourceFragment, skipped: &HashSet<SourceOrigin>) {
        for word in classify_rust(&fragment.text) {
            let Some(source) = fragment.source_at(word.offset) else {
                continue;
            };
            if !source.source.eligible || skipped.contains(&source.source.site.origin) {
                continue;
            }
            let key = SourceTokenKey {
                site: source.source.site.clone(),
                column: source.column,
                name: word.name.clone(),
            };
            let index = if let Some(index) = self.by_source.get(&key) {
                *index
            } else {
                let index = self.tokens.len();
                self.tokens.push(ObservedToken {
                    name: word.name.clone(),
                    site: source.source.site,
                    column: source.column,
                    evidence: TokenEvidence::default(),
                });
                self.by_source.insert(key, index);
                index
            };
            self.tokens[index].evidence.observe(&word);
        }
    }

    fn into_candidates(
        self,
        declared_types: &mut [HashSet<String>],
        ordinary_definitions: &mut [HashSet<String>],
    ) -> Vec<Candidate> {
        self.tokens
            .into_iter()
            .map(|token| {
                let chapter = token.site.chapter;
                if token.evidence.type_definition {
                    declared_types[chapter].insert(token.name.clone());
                }
                if token.evidence.ordinary_definition {
                    ordinary_definitions[chapter].insert(token.name.clone());
                }

                let role = if token.evidence.type_definition
                    || token.evidence.ordinary_definition
                {
                    IdentifierRole::Definition
                } else {
                    IdentifierRole::Use
                };
                let namespace = if token.evidence.ordinary_definition {
                    IdentifierNamespace::Ordinary
                } else if token.evidence.type_definition {
                    IdentifierNamespace::Type
                } else if token.evidence.ordinary_use {
                    IdentifierNamespace::Ordinary
                } else if token.evidence.type_use {
                    IdentifierNamespace::Type
                } else {
                    IdentifierNamespace::Ordinary
                };
                let type_candidate = namespace == IdentifierNamespace::Ordinary
                    && !token.evidence.ordinary_definition
                    && !token.evidence.ordinary_use
                    && token.evidence.possible_type_use;

                Candidate {
                    name: token.name,
                    role,
                    namespace,
                    type_candidate,
                    use_shape: token.evidence.use_shape,
                    definition_kind: (!token.evidence.conflicting_definition_kinds)
                        .then_some(token.evidence.definition_kind)
                        .flatten(),
                    site: token.site,
                    column: token.column,
                }
            })
            .collect()
    }
}

impl TokenEvidence {
    fn observe(&mut self, word: &ClassifiedWord) {
        self.use_shape = self.use_shape.merge(word.use_shape);
        if let Some(kind) = word.definition_kind {
            if self.definition_kind.is_some_and(|current| current != kind) {
                self.conflicting_definition_kinds = true;
            } else {
                self.definition_kind = Some(kind);
            }
        }
        match (word.role, word.namespace) {
            (IdentifierRole::Definition, IdentifierNamespace::Ordinary) => {
                self.ordinary_definition = true;
            }
            (IdentifierRole::Definition, IdentifierNamespace::Type) => {
                self.type_definition = true;
            }
            (IdentifierRole::Use, IdentifierNamespace::Type) => {
                self.type_use = true;
            }
            (IdentifierRole::Use, IdentifierNamespace::Ordinary)
                if word.type_candidate =>
            {
                self.possible_type_use = true;
            }
            (IdentifierRole::Use, IdentifierNamespace::Ordinary) => {
                self.ordinary_use = true;
            }
        }
    }
}

// Finish identifier index
fn finish_index(
    mut candidates: Vec<Candidate>,
    declared_types: &[HashSet<String>],
    ordinary_definitions: &[HashSet<String>],
) -> IdentifierIndex {
    let all_declared_types = declared_types
        .iter()
        .flat_map(|names| names.iter().cloned())
        .collect::<HashSet<_>>();
    for candidate in &mut candidates {
        if candidate.namespace == IdentifierNamespace::Ordinary
            && candidate.type_candidate
            && !ordinary_definitions[candidate.site.chapter].contains(&candidate.name)
            && (all_declared_types.contains(&candidate.name)
                || candidate
                    .name
                    .chars()
                    .next()
                    .is_some_and(char::is_uppercase))
        {
            candidate.namespace = IdentifierNamespace::Type;
        }
    }

    let (meanings, meaning_occurrences) = collect_meanings(&candidates);
    let mut grouped: HashMap<String, Vec<IdentifierOccurrence>> = HashMap::new();

    for candidate in candidates {
        if candidate.role == IdentifierRole::Use && candidate.name.chars().count() == 1 {
            continue;
        }

        if candidate.namespace == IdentifierNamespace::Type
            && !declared_types[candidate.site.chapter].contains(&candidate.name)
        {
            continue;
        }

        grouped
            .entry(display_name(&candidate.name))
            .or_default()
            .push(IdentifierOccurrence {
                role: candidate.role,
                namespace: candidate.namespace,
                site: candidate.site,
            });
    }

    let mut entries = grouped
        .into_iter()
        .map(|(name, occurrences)| IdentifierEntry { name, occurrences })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    IdentifierIndex {
        entries,
        meanings,
        meaning_occurrences,
    }
}

fn collect_meanings(
    candidates: &[Candidate],
) -> (Vec<IdentifierMeaning>, Vec<IdentifierMeaningOccurrence>) {
    let mut meanings = Vec::new();
    let mut candidate_meanings = vec![None; candidates.len()];
    let mut local_definitions: HashMap<(usize, String, IdentifierNamespace), Vec<usize>> =
        HashMap::new();
    let mut book_definitions: HashMap<(String, IdentifierNamespace), Vec<usize>> =
        HashMap::new();

    for (candidate_index, candidate) in candidates.iter().enumerate() {
        let Some(kind) = candidate.definition_kind else {
            continue;
        };
        let id = meanings.len();
        let name = display_name(&candidate.name);
        meanings.push(IdentifierMeaning {
            id,
            name: name.clone(),
            namespace: candidate.namespace,
            kind,
            definition: candidate.site.clone(),
        });
        candidate_meanings[candidate_index] = Some(id);
        local_definitions
            .entry((candidate.site.chapter, name.clone(), candidate.namespace))
            .or_default()
            .push(id);
        book_definitions
            .entry((name, candidate.namespace))
            .or_default()
            .push(id);
    }

    let mut occurrences = Vec::new();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        let meaning = candidate_meanings[candidate_index].or_else(|| {
            let name = display_name(&candidate.name);
            let local = local_definitions
                .get(&(candidate.site.chapter, name.clone(), candidate.namespace))
                .map(Vec::as_slice)
                .unwrap_or_default();
            select_meaning(
                local,
                &meanings,
                candidate.use_shape,
                candidate.site.chapter,
            )
            .or_else(|| {
                let book = book_definitions
                    .get(&(name, candidate.namespace))
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                select_meaning(
                    book,
                    &meanings,
                    candidate.use_shape,
                    candidate.site.chapter,
                )
            })
        });
        let Some(meaning) = meaning else {
            continue;
        };
        occurrences.push(IdentifierMeaningOccurrence {
            meaning,
            role: if candidate.definition_kind.is_some() {
                IdentifierRole::Definition
            } else {
                IdentifierRole::Use
            },
            site: candidate.site.clone(),
            column: candidate.column,
        });
    }

    (meanings, occurrences)
}

fn select_meaning(
    candidates: &[usize],
    meanings: &[IdentifierMeaning],
    use_shape: UseShape,
    latest_chapter: usize,
) -> Option<usize> {
    let mut eligible = candidates
        .iter()
        .copied()
        .filter(|meaning| meanings[*meaning].definition.chapter <= latest_chapter)
        .filter(|meaning| match use_shape {
            UseShape::None => true,
            UseShape::Field => meanings[*meaning].kind == IdentifierKind::Field,
            UseShape::Method => meanings[*meaning].kind == IdentifierKind::Function,
            UseShape::Macro => meanings[*meaning].kind == IdentifierKind::Macro,
            UseShape::Ambiguous => false,
        });
    let selected = eligible.next()?;
    eligible.next().is_none().then_some(selected)
}

fn display_name(name: &str) -> String {
    if is_keyword(name) {
        format!("r#{name}")
    } else {
        name.to_owned()
    }
}

// Rust identifier analysis
// Lex Rust
fn lex_rust(text: &str) -> Vec<RustToken> {
    let mut tokens = Vec::new();
    let mut position = 0;

    while position < text.len() {
        let rest = &text[position..];
        let character = rest
            .chars()
            .next()
            .expect("position is on a character boundary");

        if character.is_whitespace() {
            position += character.len_utf8();
            continue;
        }

        if rest.starts_with("//") {
            position += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("/*") {
            position += block_comment_len(rest);
            continue;
        }

        if let Some(length) = raw_literal_len(rest) {
            tokens.push(RustToken {
                kind: RustTokenKind::Opaque,
                offset: position,
            });
            position += length;
            continue;
        }
        if let Some(length) = quoted_literal_len(rest) {
            tokens.push(RustToken {
                kind: RustTokenKind::Opaque,
                offset: position,
            });
            position += length;
            continue;
        }
        if character == '\'' {
            tokens.push(RustToken {
                kind: RustTokenKind::Opaque,
                offset: position,
            });
            position += apostrophe_form_len(rest);
            continue;
        }

        if let Some(after_prefix) = rest.strip_prefix("r#")
            && let Some(first) = after_prefix.chars().next()
            && is_identifier_start(first)
        {
            let length = identifier_len(after_prefix);
            tokens.push(RustToken {
                kind: RustTokenKind::Word {
                    name: after_prefix[..length].to_owned(),
                    raw: true,
                },
                offset: position,
            });
            position += 2 + length;
            continue;
        }

        if is_identifier_start(character) {
            let length = identifier_len(rest);
            tokens.push(RustToken {
                kind: RustTokenKind::Word {
                    name: rest[..length].to_owned(),
                    raw: false,
                },
                offset: position,
            });
            position += length;
            continue;
        }

        if character.is_ascii_digit() {
            let length = number_len(rest);
            tokens.push(RustToken {
                kind: RustTokenKind::Opaque,
                offset: position,
            });
            position += length;
            continue;
        }

        let (symbol, length) = rust_symbol(rest);
        tokens.push(RustToken {
            kind: RustTokenKind::Symbol(symbol),
            offset: position,
        });
        position += length;
    }

    tokens
}

// Skip Rust comments and literals
fn block_comment_len(text: &str) -> usize {
    let mut position = 2;
    let mut depth = 1;
    while position < text.len() {
        let rest = &text[position..];
        if rest.starts_with("/*") {
            depth += 1;
            position += 2;
        } else if rest.starts_with("*/") {
            depth -= 1;
            position += 2;
            if depth == 0 {
                return position;
            }
        } else {
            position += rest
                .chars()
                .next()
                .expect("position is on a character boundary")
                .len_utf8();
        }
    }
    text.len()
}

fn raw_literal_len(text: &str) -> Option<usize> {
    let prefix = ["br", "cr", "r"]
        .into_iter()
        .find(|prefix| text.starts_with(prefix))?;
    let mut position = prefix.len();
    while text[position..].starts_with('#') {
        position += 1;
    }
    if !text[position..].starts_with('"') {
        return None;
    }
    let hashes = position - prefix.len();
    position += 1;
    let closing = format!("\"{}", "#".repeat(hashes));
    Some(
        text[position..]
            .find(&closing)
            .map_or(text.len(), |end| position + end + closing.len()),
    )
}

fn quoted_literal_len(text: &str) -> Option<usize> {
    let (prefix, quote) = if text.starts_with("b\"") || text.starts_with("c\"") {
        (1, '"')
    } else if text.starts_with("b'") {
        (1, '\'')
    } else if text.starts_with('"') {
        (0, '"')
    } else {
        return None;
    };
    Some(quoted_len_from(text, prefix, quote))
}

fn quoted_len_from(text: &str, prefix: usize, quote: char) -> usize {
    let mut escaped = false;
    let mut position = prefix + quote.len_utf8();
    while position < text.len() {
        let character = text[position..]
            .chars()
            .next()
            .expect("position is on a character boundary");
        position += character.len_utf8();
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == quote {
            return position;
        }
    }
    text.len()
}

fn apostrophe_form_len(text: &str) -> usize {
    let after = &text[1..];
    let Some(first) = after.chars().next() else {
        return 1;
    };
    if first == '\\' {
        return quoted_len_from(text, 0, '\'');
    }
    if is_identifier_start(first) {
        let word_length = identifier_len(after);
        if after[word_length..].starts_with('\'') {
            return 1 + word_length + 1;
        }
        return 1 + word_length;
    }
    quoted_len_from(text, 0, '\'')
}

// Rust lexical boundaries
fn is_identifier_start(character: char) -> bool {
    character == '_' || is_xid_start(character)
}

fn is_identifier_continue(character: char) -> bool {
    character == '_' || is_xid_continue(character)
}

fn identifier_len(text: &str) -> usize {
    text.char_indices()
        .find_map(|(offset, character)| {
            (offset > 0 && !is_identifier_continue(character)).then_some(offset)
        })
        .unwrap_or(text.len())
}

fn number_len(text: &str) -> usize {
    text.char_indices()
        .find_map(|(offset, character)| {
            (!character.is_ascii_alphanumeric() && character != '_').then_some(offset)
        })
        .filter(|offset| *offset > 0)
        .unwrap_or(text.len())
}

fn rust_symbol(text: &str) -> (&'static str, usize) {
    for symbol in ["::", "->", "=>", "..=", "...", "..", "&&", "||"] {
        if text.starts_with(symbol) {
            return (symbol, symbol.len());
        }
    }
    let character = text.chars().next().expect("symbol text is not empty");
    let symbol = match character {
        '(' => "(",
        ')' => ")",
        '[' => "[",
        ']' => "]",
        '{' => "{",
        '}' => "}",
        '<' => "<",
        '>' => ">",
        ':' => ":",
        ';' => ";",
        ',' => ",",
        '=' => "=",
        '|' => "|",
        '@' => "@",
        '!' => "!",
        '.' => ".",
        '+' => "+",
        '-' => "-",
        '*' => "*",
        '/' => "/",
        '&' => "&",
        _ => "?",
    };
    (symbol, character.len_utf8())
}

// Classify Rust words
fn classify_rust(text: &str) -> Vec<ClassifiedWord> {
    let tokens = lex_rust(text);
    let mut roles = vec![IdentifierRole::Use; tokens.len()];
    let mut namespaces = vec![IdentifierNamespace::Ordinary; tokens.len()];
    let mut type_candidates = vec![false; tokens.len()];
    let mut definition_kinds = vec![None; tokens.len()];

    classify_items(
        &tokens,
        &mut roles,
        &mut namespaces,
        &mut type_candidates,
        &mut definition_kinds,
    );
    classify_bindings(
        &tokens,
        &mut roles,
        &mut namespaces,
        &mut type_candidates,
        &mut definition_kinds,
    );
    classify_type_clues(&tokens, &mut namespaces, &mut type_candidates);
    classify_imports(&tokens, &mut namespaces, &mut definition_kinds);

    tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let RustTokenKind::Word { name, raw } = &token.kind else {
                return None;
            };
            if name == "_" || (!raw && (is_keyword(name) || is_primitive(name))) {
                return None;
            }
            Some(ClassifiedWord {
                name: name.clone(),
                offset: token.offset,
                role: roles[index],
                namespace: namespaces[index],
                type_candidate: type_candidates[index],
                use_shape: classify_use_shape(&tokens, index),
                definition_kind: definition_kinds[index],
            })
        })
        .collect()
}

fn classify_use_shape(tokens: &[RustToken], index: usize) -> UseShape {
    if symbol(tokens, index + 1) == Some("!") {
        return UseShape::Macro;
    }
    if index == 0 || symbol(tokens, index - 1) != Some(".") {
        return UseShape::None;
    }
    if symbol(tokens, index + 1) == Some("(") {
        return UseShape::Method;
    }
    if symbol(tokens, index + 1) == Some("::")
        && symbol(tokens, index + 2) == Some("<")
        && let Some(close) = matching_symbol(tokens, index + 2, "<", ">")
        && symbol(tokens, close + 1) == Some("(")
    {
        return UseShape::Method;
    }
    UseShape::Field
}

pub(crate) fn rust_identifier_offsets(text: &str) -> Vec<usize> {
    classify_rust(text)
        .into_iter()
        .map(|word| word.offset)
        .collect()
}

// Classify Rust items
fn classify_items(
    tokens: &[RustToken],
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    for index in 0..tokens.len() {
        let Some(keyword) = word(tokens, index) else {
            continue;
        };

        if keyword == "macro_rules" {
            if let Some(name) = next_identifier(tokens, index + 1) {
                roles[name] = IdentifierRole::Definition;
                definition_kinds[name] = Some(IdentifierKind::Macro);
            }
            continue;
        }

        let type_item = matches!(keyword, "struct" | "enum" | "union" | "trait" | "type");
        let ordinary_item = matches!(keyword, "fn" | "const" | "static" | "mod");
        if !type_item && !ordinary_item {
            continue;
        }
        if matches!(keyword, "const" | "static") && inside_angle_brackets(tokens, index) {
            continue;
        }

        let Some(name) = next_identifier(tokens, index + 1) else {
            continue;
        };
        roles[name] = IdentifierRole::Definition;
        definition_kinds[name] = Some(match keyword {
            "struct" => IdentifierKind::Struct,
            "enum" => IdentifierKind::Enum,
            "union" => IdentifierKind::Union,
            "trait" => IdentifierKind::Trait,
            "type" => IdentifierKind::TypeAlias,
            "fn" => IdentifierKind::Function,
            "const" => IdentifierKind::Constant,
            "static" => IdentifierKind::Static,
            "mod" => IdentifierKind::Module,
            _ => unreachable!("the item keyword was checked above"),
        });
        if type_item {
            namespaces[name] = IdentifierNamespace::Type;
        }

        let after_generics = mark_generics(
            tokens,
            name + 1,
            roles,
            namespaces,
            type_candidates,
            definition_kinds,
        );

        match keyword {
            "fn" => mark_function(
                tokens,
                after_generics,
                roles,
                namespaces,
                type_candidates,
                definition_kinds,
            ),
            "struct" | "union" => mark_struct(
                tokens,
                after_generics,
                roles,
                namespaces,
                type_candidates,
                definition_kinds,
            ),
            "enum" => mark_enum(
                tokens,
                after_generics,
                roles,
                namespaces,
                type_candidates,
                definition_kinds,
            ),
            "type" => {
                if let Some(equal) =
                    find_symbol_before(tokens, after_generics, "=", &[";"])
                {
                    let end = find_symbol(tokens, equal + 1, ";").unwrap_or(tokens.len());
                    mark_type_range(tokens, equal + 1, end, namespaces, type_candidates);
                }
            }
            "const" | "static" if symbol(tokens, name + 1) == Some(":") => {
                let end = find_any_symbol(tokens, name + 2, &["=", ";"])
                    .unwrap_or(tokens.len());
                mark_type_range(tokens, name + 2, end, namespaces, type_candidates);
            }
            _ => {}
        }
    }
}

// Mark generics and functions
fn mark_generics(
    tokens: &[RustToken],
    start: usize,
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) -> usize {
    if symbol(tokens, start) != Some("<") {
        return start;
    }
    let Some(end) = matching_symbol(tokens, start, "<", ">") else {
        return start;
    };
    for (segment_start, segment_end) in top_level_segments(tokens, start + 1, end, ",") {
        let constant = word(tokens, segment_start) == Some("const");
        let search_start = segment_start + usize::from(constant);
        if let Some(name) = next_identifier_before(tokens, search_start, segment_end) {
            roles[name] = IdentifierRole::Definition;
            definition_kinds[name] = Some(if constant {
                IdentifierKind::Constant
            } else {
                IdentifierKind::GenericParameter
            });
            if !constant {
                namespaces[name] = IdentifierNamespace::Type;
            }
            if let Some(colon) = find_symbol_before(tokens, name + 1, ":", &[","]) {
                mark_type_range(
                    tokens,
                    colon + 1,
                    segment_end,
                    namespaces,
                    type_candidates,
                );
            }
        }
    }
    end + 1
}

fn mark_function(
    tokens: &[RustToken],
    start: usize,
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    let Some(open) = find_symbol_before(tokens, start, "(", &["{", ";"]) else {
        return;
    };
    let Some(close) = matching_symbol(tokens, open, "(", ")") else {
        return;
    };
    mark_parameter_list(
        tokens,
        open + 1,
        close,
        roles,
        namespaces,
        type_candidates,
        definition_kinds,
    );
    if symbol(tokens, close + 1) == Some("->") {
        let end = find_any_symbol(tokens, close + 2, &["{", ";"]).unwrap_or(tokens.len());
        mark_type_range(tokens, close + 2, end, namespaces, type_candidates);
    }
}

fn mark_parameter_list(
    tokens: &[RustToken],
    start: usize,
    end: usize,
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    for (segment_start, segment_end) in top_level_segments(tokens, start, end, ",") {
        if let Some(colon) = top_level_symbol(tokens, segment_start, segment_end, ":") {
            mark_pattern(
                tokens,
                segment_start,
                colon,
                roles,
                definition_kinds,
                IdentifierKind::Parameter,
            );
            mark_type_range(tokens, colon + 1, segment_end, namespaces, type_candidates);
        } else {
            mark_pattern(
                tokens,
                segment_start,
                segment_end,
                roles,
                definition_kinds,
                IdentifierKind::Parameter,
            );
        }
    }
}

// Mark struct and enum contents
fn mark_struct(
    tokens: &[RustToken],
    start: usize,
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    if let Some(open) = find_symbol_before(tokens, start, "{", &[";"])
        && let Some(close) = matching_symbol(tokens, open, "{", "}")
    {
        mark_named_fields(
            tokens,
            open + 1,
            close,
            roles,
            namespaces,
            type_candidates,
            definition_kinds,
        );
    } else if let Some(open) = find_symbol_before(tokens, start, "(", &[";"])
        && let Some(close) = matching_symbol(tokens, open, "(", ")")
    {
        mark_type_range(tokens, open + 1, close, namespaces, type_candidates);
    }
}

fn mark_named_fields(
    tokens: &[RustToken],
    start: usize,
    end: usize,
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    for (field_start, field_end) in top_level_segments(tokens, start, end, ",") {
        let Some(colon) = top_level_symbol(tokens, field_start, field_end, ":") else {
            continue;
        };
        if let Some(name) = previous_identifier(tokens, colon) {
            roles[name] = IdentifierRole::Definition;
            definition_kinds[name] = Some(IdentifierKind::Field);
        }
        mark_type_range(tokens, colon + 1, field_end, namespaces, type_candidates);
    }
}

fn mark_enum(
    tokens: &[RustToken],
    start: usize,
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    let Some(open) = find_symbol_before(tokens, start, "{", &[";"]) else {
        return;
    };
    let Some(close) = matching_symbol(tokens, open, "{", "}") else {
        return;
    };
    for (variant_start, variant_end) in top_level_segments(tokens, open + 1, close, ",") {
        let Some(name) = next_identifier_before(tokens, variant_start, variant_end)
        else {
            continue;
        };
        roles[name] = IdentifierRole::Definition;
        definition_kinds[name] = Some(IdentifierKind::Variant);
        if symbol(tokens, name + 1) == Some("(")
            && let Some(payload_end) = matching_symbol(tokens, name + 1, "(", ")")
        {
            mark_type_range(tokens, name + 2, payload_end, namespaces, type_candidates);
        } else if symbol(tokens, name + 1) == Some("{")
            && let Some(payload_end) = matching_symbol(tokens, name + 1, "{", "}")
        {
            mark_named_fields(
                tokens,
                name + 2,
                payload_end,
                roles,
                namespaces,
                type_candidates,
                definition_kinds,
            );
        }
    }
}

// Classify Rust bindings
fn classify_bindings(
    tokens: &[RustToken],
    roles: &mut [IdentifierRole],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    for index in 0..tokens.len() {
        match word(tokens, index) {
            Some("let") => {
                let end = find_any_word_or_symbol(tokens, index + 1, &["=", ";", "else"])
                    .unwrap_or(tokens.len());
                if let Some(colon) = top_level_symbol(tokens, index + 1, end, ":") {
                    mark_pattern(
                        tokens,
                        index + 1,
                        colon,
                        roles,
                        definition_kinds,
                        IdentifierKind::LocalVariable,
                    );
                    mark_type_range(tokens, colon + 1, end, namespaces, type_candidates);
                } else {
                    mark_pattern(
                        tokens,
                        index + 1,
                        end,
                        roles,
                        definition_kinds,
                        IdentifierKind::LocalVariable,
                    );
                }
            }
            Some("for") => {
                if let Some(end) = loop_pattern_end(tokens, index) {
                    mark_pattern(
                        tokens,
                        index + 1,
                        end,
                        roles,
                        definition_kinds,
                        IdentifierKind::LocalVariable,
                    );
                }
            }
            Some("match") => mark_match_arms(tokens, index + 1, roles, definition_kinds),
            _ => {}
        }

        if symbol(tokens, index) == Some("|")
            && closure_can_start(tokens, index)
            && let Some(close) = closure_parameter_end(tokens, index)
        {
            mark_parameter_list(
                tokens,
                index + 1,
                close,
                roles,
                namespaces,
                type_candidates,
                definition_kinds,
            );
        }
    }
}

fn loop_pattern_end(tokens: &[RustToken], for_index: usize) -> Option<usize> {
    if symbol(tokens, for_index + 1) == Some("<")
        || for_belongs_to_impl_header(tokens, for_index)
    {
        return None;
    }

    let mut depth = Depth::default();
    for index in for_index + 1..tokens.len() {
        if depth.is_zero() {
            if word(tokens, index) == Some("in") {
                return Some(index);
            }
            if matches!(symbol(tokens, index), Some(";" | "}" | "=>")) {
                return None;
            }
        }
        if let Some(value) = symbol(tokens, index) {
            depth.update(value);
        }
    }
    None
}

fn for_belongs_to_impl_header(tokens: &[RustToken], for_index: usize) -> bool {
    for index in (0..for_index).rev() {
        if word(tokens, index) == Some("impl") {
            return true;
        }
        if matches!(symbol(tokens, index), Some("{" | "}" | ";")) {
            return false;
        }
    }
    false
}

fn mark_pattern(
    tokens: &[RustToken],
    start: usize,
    end: usize,
    roles: &mut [IdentifierRole],
    definition_kinds: &mut [Option<IdentifierKind>],
    kind: IdentifierKind,
) {
    for (index, role) in roles.iter_mut().enumerate().take(end).skip(start) {
        let Some(_) = word(tokens, index) else {
            continue;
        };
        if token_is_keyword(tokens, index)
            || symbol(tokens, index.saturating_sub(1)) == Some("::")
            || symbol(tokens, index + 1) == Some("::")
            || (index + 1 < end && matches!(symbol(tokens, index + 1), Some("(" | "{")))
            || (index + 1 < end && symbol(tokens, index + 1) == Some(":"))
        {
            continue;
        }
        *role = IdentifierRole::Definition;
        definition_kinds[index] = Some(kind);
    }
}

fn mark_match_arms(
    tokens: &[RustToken],
    start: usize,
    roles: &mut [IdentifierRole],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    let Some(open) = find_symbol(tokens, start, "{") else {
        return;
    };
    let Some(close) = matching_symbol(tokens, open, "{", "}") else {
        return;
    };
    for (arm_start, arm_end) in top_level_segments(tokens, open + 1, close, ",") {
        let Some(arrow) = top_level_symbol(tokens, arm_start, arm_end, "=>") else {
            continue;
        };
        let pattern_end =
            find_word_before(tokens, arm_start, arrow, "if").unwrap_or(arrow);
        mark_pattern(
            tokens,
            arm_start,
            pattern_end,
            roles,
            definition_kinds,
            IdentifierKind::LocalVariable,
        );
    }
}

fn closure_can_start(tokens: &[RustToken], index: usize) -> bool {
    index == 0
        || matches!(
            symbol(tokens, index - 1),
            Some("=" | "(" | "{" | "[" | "," | "=>" | ";")
        )
        || word(tokens, index - 1) == Some("move")
}

fn closure_parameter_end(tokens: &[RustToken], open: usize) -> Option<usize> {
    let mut depth = Depth::default();
    for index in open + 1..tokens.len() {
        if depth.is_zero() {
            if symbol(tokens, index) == Some("|") {
                return Some(index);
            }
            if matches!(symbol(tokens, index), Some(")" | "]" | "}" | ";" | "=>")) {
                return None;
            }
        }
        if let Some(value) = symbol(tokens, index) {
            depth.update(value);
        }
    }
    None
}

// Classify Rust type clues
fn classify_type_clues(
    tokens: &[RustToken],
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
) {
    for index in 0..tokens.len() {
        if word(tokens, index) == Some("use") {
            let end = find_symbol(tokens, index + 1, ";").unwrap_or(tokens.len());
            for imported in index + 1..end {
                if word(tokens, imported)
                    .and_then(|name| name.chars().next())
                    .is_some_and(char::is_uppercase)
                {
                    namespaces[imported] = IdentifierNamespace::Type;
                    type_candidates[imported] = true;
                }
            }
        }

        if matches!(word(tokens, index), Some("as" | "impl" | "dyn"))
            && let Some(name) = next_identifier(tokens, index + 1)
        {
            namespaces[name] = IdentifierNamespace::Type;
        }

        if word(tokens, index).is_some()
            && (symbol(tokens, index + 1) == Some("::")
                || matches!(symbol(tokens, index + 1), Some("(" | "{")))
        {
            type_candidates[index] = true;
        }

        if symbol(tokens, index) == Some("::")
            && symbol(tokens, index + 1) == Some("<")
            && let Some(end) = matching_symbol(tokens, index + 1, "<", ">")
        {
            mark_type_range(tokens, index + 2, end, namespaces, type_candidates);
        }
    }
}

fn mark_type_range(
    tokens: &[RustToken],
    start: usize,
    end: usize,
    namespaces: &mut [IdentifierNamespace],
    type_candidates: &mut [bool],
) {
    for index in start..end.min(tokens.len()) {
        if word(tokens, index).is_some() && !token_is_keyword(tokens, index) {
            namespaces[index] = IdentifierNamespace::Type;
            type_candidates[index] = true;
        }
    }
}

fn classify_imports(
    tokens: &[RustToken],
    namespaces: &mut [IdentifierNamespace],
    definition_kinds: &mut [Option<IdentifierKind>],
) {
    for start in (0..tokens.len()).filter(|index| word(tokens, *index) == Some("use")) {
        let end = find_symbol(tokens, start + 1, ";").unwrap_or(tokens.len());
        for index in start + 1..end {
            let Some(name) = word(tokens, index) else {
                continue;
            };
            let aliased = word(tokens, index.saturating_sub(1)) == Some("as");
            let path_leaf = symbol(tokens, index + 1) != Some("::")
                && matches!(symbol(tokens, index + 1), Some("," | "}" | ";") | None);
            if !aliased && !path_leaf {
                continue;
            }
            definition_kinds[index] = Some(IdentifierKind::Import);
            if name.chars().next().is_some_and(char::is_uppercase) {
                namespaces[index] = IdentifierNamespace::Type;
            }
        }
    }
}

// Rust analysis support
// Navigate Rust tokens
fn word(tokens: &[RustToken], index: usize) -> Option<&str> {
    match tokens.get(index)?.kind {
        RustTokenKind::Word { ref name, .. } => Some(name),
        RustTokenKind::Symbol(_) | RustTokenKind::Opaque => None,
    }
}

fn symbol(tokens: &[RustToken], index: usize) -> Option<&'static str> {
    match tokens.get(index)?.kind {
        RustTokenKind::Symbol(symbol) => Some(symbol),
        RustTokenKind::Word { .. } | RustTokenKind::Opaque => None,
    }
}

fn token_is_keyword(tokens: &[RustToken], index: usize) -> bool {
    match tokens.get(index).map(|token| &token.kind) {
        Some(RustTokenKind::Word { name, raw: false }) => is_keyword(name),
        _ => false,
    }
}

fn next_identifier(tokens: &[RustToken], start: usize) -> Option<usize> {
    next_identifier_before(tokens, start, tokens.len())
}

fn next_identifier_before(
    tokens: &[RustToken],
    start: usize,
    end: usize,
) -> Option<usize> {
    (start..end).find(|index| {
        matches!(tokens[*index].kind, RustTokenKind::Word { raw: true, .. })
            || word(tokens, *index).is_some_and(|name| !is_keyword(name))
    })
}

fn previous_identifier(tokens: &[RustToken], before: usize) -> Option<usize> {
    (0..before).rev().find(|index| {
        matches!(tokens[*index].kind, RustTokenKind::Word { raw: true, .. })
            || word(tokens, *index).is_some_and(|name| !is_keyword(name))
    })
}

fn find_symbol(tokens: &[RustToken], start: usize, wanted: &str) -> Option<usize> {
    (start..tokens.len()).find(|index| symbol(tokens, *index) == Some(wanted))
}

fn find_symbol_before(
    tokens: &[RustToken],
    start: usize,
    wanted: &str,
    stops: &[&str],
) -> Option<usize> {
    for index in start..tokens.len() {
        if symbol(tokens, index) == Some(wanted) {
            return Some(index);
        }
        if symbol(tokens, index).is_some_and(|value| stops.contains(&value)) {
            return None;
        }
    }
    None
}

fn find_any_symbol(tokens: &[RustToken], start: usize, wanted: &[&str]) -> Option<usize> {
    (start..tokens.len())
        .find(|index| symbol(tokens, *index).is_some_and(|value| wanted.contains(&value)))
}

fn find_word_before(
    tokens: &[RustToken],
    start: usize,
    end: usize,
    wanted: &str,
) -> Option<usize> {
    (start..end).find(|index| word(tokens, *index) == Some(wanted))
}

fn find_any_word_or_symbol(
    tokens: &[RustToken],
    start: usize,
    wanted: &[&str],
) -> Option<usize> {
    (start..tokens.len()).find(|index| {
        symbol(tokens, *index)
            .or_else(|| word(tokens, *index))
            .is_some_and(|value| wanted.contains(&value))
    })
}

// Navigate balanced token ranges
fn matching_symbol(
    tokens: &[RustToken],
    open_index: usize,
    open: &str,
    close: &str,
) -> Option<usize> {
    let mut depth = 0;
    for index in open_index..tokens.len() {
        match symbol(tokens, index) {
            Some(value) if value == open => depth += 1,
            Some(value) if value == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn inside_angle_brackets(tokens: &[RustToken], before: usize) -> bool {
    let mut depth = 0usize;
    for index in 0..before {
        match symbol(tokens, index) {
            Some("<") => depth += 1,
            Some(">") => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth > 0
}

#[derive(Clone, Copy, Default)]
struct Depth {
    round: usize,
    square: usize,
    brace: usize,
    angle: usize,
}

impl Depth {
    fn update(&mut self, symbol: &str) {
        match symbol {
            "(" => self.round += 1,
            ")" => self.round = self.round.saturating_sub(1),
            "[" => self.square += 1,
            "]" => self.square = self.square.saturating_sub(1),
            "{" => self.brace += 1,
            "}" => self.brace = self.brace.saturating_sub(1),
            "<" => self.angle += 1,
            ">" => self.angle = self.angle.saturating_sub(1),
            _ => {}
        }
    }

    fn is_zero(self) -> bool {
        self.round == 0 && self.square == 0 && self.brace == 0 && self.angle == 0
    }
}

fn top_level_segments(
    tokens: &[RustToken],
    start: usize,
    end: usize,
    separator: &str,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut segment_start = start;
    let mut depth = Depth::default();
    for index in start..end {
        if depth.is_zero() && symbol(tokens, index) == Some(separator) {
            ranges.push((segment_start, index));
            segment_start = index + 1;
        } else if let Some(value) = symbol(tokens, index) {
            depth.update(value);
        }
    }
    if segment_start < end {
        ranges.push((segment_start, end));
    }
    ranges
}

fn top_level_symbol(
    tokens: &[RustToken],
    start: usize,
    end: usize,
    wanted: &str,
) -> Option<usize> {
    let mut depth = Depth::default();
    for index in start..end {
        if depth.is_zero() && symbol(tokens, index) == Some(wanted) {
            return Some(index);
        }
        if let Some(value) = symbol(tokens, index) {
            depth.update(value);
        }
    }
    None
}

// Rust vocabulary
fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "macro_rules"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
            | "abstract"
            | "become"
            | "box"
            | "do"
    )
}

fn is_primitive(word: &str) -> bool {
    matches!(
        word,
        "bool"
            | "char"
            | "str"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    )
}
