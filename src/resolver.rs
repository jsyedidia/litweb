// src/resolver.rs
//! Definition resolution and ordered root discovery.

// Resolver imports
use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::parser::{BlockKind, Line, Modifier, Program, SourceOrigin};

// Resolved definitions
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBlock {
    pub chapter: usize,
    pub name: String,
    pub origin: SourceOrigin,
    pub lines: Vec<Line>,
    pub(crate) line_chapters: Vec<usize>,
    pub comment_string: String,
    pub no_header: bool,
}

// Resolved roots
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub block: usize,
    pub path: PathBuf,
    pub origin: SourceOrigin,
}

// Resolved program indexes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProgram {
    pub blocks: Vec<ResolvedBlock>,
    pub roots: Vec<ResolvedRoot>,
    local_lookup: Vec<HashMap<String, usize>>,
    by_name: HashMap<String, Vec<usize>>,
}

// Resolved name lookup
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockLookup {
    Found(usize),
    Missing,
    Ambiguous(Vec<usize>),
}

impl ResolvedProgram {
    pub fn block(&self, index: usize) -> &ResolvedBlock {
        &self.blocks[index]
    }

    pub fn block_named(&self, name: &str) -> Option<(usize, &ResolvedBlock)> {
        match self.by_name.get(name).map(Vec::as_slice) {
            Some([index]) => Some((*index, &self.blocks[*index])),
            _ => None,
        }
    }

    pub fn lookup(&self, chapter: usize, name: &str) -> BlockLookup {
        if let Some(index) = self
            .local_lookup
            .get(chapter)
            .and_then(|lookup| lookup.get(name))
        {
            return BlockLookup::Found(*index);
        }
        match self.by_name.get(name).map(Vec::as_slice) {
            None | Some([]) => BlockLookup::Missing,
            Some([index]) => BlockLookup::Found(*index),
            Some(indices) => BlockLookup::Ambiguous(indices.to_vec()),
        }
    }
}

// Kinds of definition change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Addition,
    Redefinition,
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Addition => f.write_str("add to"),
            Self::Redefinition => f.write_str("redefine"),
        }
    }
}

// Resolution error model
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveError {
    pub origin: SourceOrigin,
    pub kind: ResolveErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveErrorKind {
    DuplicateDefinition {
        name: String,
    },
    MissingChangeTarget {
        name: String,
        change: ChangeKind,
    },
    AmbiguousChangeTarget {
        name: String,
        change: ChangeKind,
        candidates: Vec<SourceOrigin>,
    },
    IncompatibleModifier {
        name: String,
        modifier: String,
    },
    InvalidRootPath {
        path: PathBuf,
        reason: InvalidPathReason,
    },
    DuplicateRootPath {
        path: PathBuf,
    },
}

// Portable path rejection reasons
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidPathReason {
    Empty,
    Absolute,
    ParentTraversal,
    CurrentDirectory,
    TrailingSeparator,
    Backslash,
    WindowsPrefix,
}

impl fmt::Display for InvalidPathReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the path is empty"),
            Self::Absolute => f.write_str("absolute paths are not allowed"),
            Self::ParentTraversal => f.write_str("parent traversal is not allowed"),
            Self::CurrentDirectory => {
                f.write_str("current-directory components are not allowed")
            }
            Self::TrailingSeparator => {
                f.write_str("the path ends with a directory separator")
            }
            Self::Backslash => f.write_str("backslash paths are not portable"),
            Self::WindowsPrefix => f.write_str("Windows path prefixes are not allowed"),
        }
    }
}

// Render one resolution error
impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: error: ", self.origin)?;
        match &self.kind {
            ResolveErrorKind::DuplicateDefinition { name } => {
                write!(f, "duplicate definition of {{{name}}}")
            }
            ResolveErrorKind::MissingChangeTarget { name, change } => {
                write!(f, "cannot {change} {{{name}}} because it is not defined")
            }
            ResolveErrorKind::AmbiguousChangeTarget {
                name,
                change,
                candidates,
            } => {
                write!(
                    f,
                    "cannot {} {{{}}} because it is ambiguous; definitions are at {}",
                    change,
                    name,
                    candidates
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ResolveErrorKind::IncompatibleModifier { name, modifier } => {
                write!(
                    f,
                    "modifier {modifier} does not apply to the change block {{{name}}}"
                )
            }
            ResolveErrorKind::InvalidRootPath { path, reason } => {
                write!(f, "invalid root path {}: {reason}", path.display())
            }
            ResolveErrorKind::DuplicateRootPath { path } => {
                write!(f, "more than one root writes {}", path.display())
            }
        }
    }
}

impl std::error::Error for ResolveError {}

// Resolution error collection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveErrors {
    errors: Vec<ResolveError>,
}

impl ResolveErrors {
    pub fn as_slice(&self) -> &[ResolveError] {
        &self.errors
    }

    pub fn into_vec(self) -> Vec<ResolveError> {
        self.errors
    }
}

impl fmt::Display for ResolveErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.errors.first() {
            write!(f, "{first}")?;
            if self.errors.len() > 1 {
                write!(f, " (and {} more resolution errors)", self.errors.len() - 1)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ResolveErrors {}

// Resolve program
pub fn resolve(program: &Program) -> Result<ResolvedProgram, ResolveErrors> {
    // Initialize resolution indexes
    let mut blocks = Vec::new();
    let mut roots = Vec::new();
    let mut local_lookup = vec![HashMap::new(); program.chapters.len()];
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut root_paths = HashMap::new();
    let mut changes = Vec::new();
    let mut errors = Vec::new();
    let mut error_sequence = 0;

    // Collect definitions roots and changes
    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        for section in &chapter.sections {
            for block in &section.blocks {
                let BlockKind::Code(code) = &block.kind else {
                    continue;
                };
                let addition = code.modifiers.contains(&Modifier::Additive);
                let redefinition = code.modifiers.contains(&Modifier::Redefinition);
                if addition || redefinition {
                    let change = if addition {
                        ChangeKind::Addition
                    } else {
                        ChangeKind::Redefinition
                    };
                    if code.modifiers.contains(&Modifier::NoHeader) {
                        push_ordered_error(
                            &mut errors,
                            &mut error_sequence,
                            chapter_index,
                            ResolveError {
                                origin: block.origin.clone(),
                                kind: ResolveErrorKind::IncompatibleModifier {
                                    name: code.name.clone(),
                                    modifier: "noHeader".to_owned(),
                                },
                            },
                        );
                    }
                    changes.push((chapter_index, block, code, change));
                    continue;
                }

                if local_lookup[chapter_index].contains_key(&code.name) {
                    push_ordered_error(
                        &mut errors,
                        &mut error_sequence,
                        chapter_index,
                        ResolveError {
                            origin: block.origin.clone(),
                            kind: ResolveErrorKind::DuplicateDefinition {
                                name: code.name.clone(),
                            },
                        },
                    );
                    continue;
                }

                let index = blocks.len();
                local_lookup[chapter_index].insert(code.name.clone(), index);
                by_name.entry(code.name.clone()).or_default().push(index);
                blocks.push(ResolvedBlock {
                    chapter: chapter_index,
                    name: code.name.clone(),
                    origin: block.origin.clone(),
                    lines: block.lines.clone(),
                    line_chapters: vec![chapter_index; block.lines.len()],
                    comment_string: code.comment_string.clone(),
                    no_header: code.modifiers.contains(&Modifier::NoHeader),
                });

                if is_root_name(&code.name, code.quoted_name) {
                    match validate_relative_path(&code.name) {
                        Ok(path) => {
                            if root_paths
                                .insert(path.clone(), block.origin.clone())
                                .is_some()
                            {
                                push_ordered_error(
                                    &mut errors,
                                    &mut error_sequence,
                                    chapter_index,
                                    ResolveError {
                                        origin: block.origin.clone(),
                                        kind: ResolveErrorKind::DuplicateRootPath {
                                            path,
                                        },
                                    },
                                );
                            } else {
                                roots.push(ResolvedRoot {
                                    block: index,
                                    path,
                                    origin: block.origin.clone(),
                                });
                            }
                        }
                        Err(reason) => push_ordered_error(
                            &mut errors,
                            &mut error_sequence,
                            chapter_index,
                            ResolveError {
                                origin: block.origin.clone(),
                                kind: ResolveErrorKind::InvalidRootPath {
                                    path: PathBuf::from(&code.name),
                                    reason,
                                },
                            },
                        ),
                    }
                }
            }
        }
    }

    // Apply changes in source order
    for (chapter_index, block, code, change) in changes {
        let index = match lookup_block(&local_lookup, &by_name, chapter_index, &code.name)
        {
            BlockLookup::Found(index) => index,
            BlockLookup::Missing => {
                push_ordered_error(
                    &mut errors,
                    &mut error_sequence,
                    chapter_index,
                    ResolveError {
                        origin: block.origin.clone(),
                        kind: ResolveErrorKind::MissingChangeTarget {
                            name: code.name.clone(),
                            change,
                        },
                    },
                );
                continue;
            }
            BlockLookup::Ambiguous(indices) => {
                push_ordered_error(
                    &mut errors,
                    &mut error_sequence,
                    chapter_index,
                    ResolveError {
                        origin: block.origin.clone(),
                        kind: ResolveErrorKind::AmbiguousChangeTarget {
                            name: code.name.clone(),
                            change,
                            candidates: indices
                                .iter()
                                .map(|index| blocks[*index].origin.clone())
                                .collect(),
                        },
                    },
                );
                continue;
            }
        };
        match change {
            ChangeKind::Addition => {
                blocks[index].lines.extend(block.lines.clone());
                blocks[index]
                    .line_chapters
                    .extend(std::iter::repeat_n(chapter_index, block.lines.len()));
            }
            ChangeKind::Redefinition => {
                blocks[index].lines = block.lines.clone();
                blocks[index].line_chapters = vec![chapter_index; block.lines.len()];
            }
        }
    }

    // Finish resolution
    if errors.is_empty() {
        Ok(ResolvedProgram {
            blocks,
            roots,
            local_lookup,
            by_name,
        })
    } else {
        errors.sort_by_key(|(chapter, line, sequence, _)| (*chapter, *line, *sequence));
        Err(ResolveErrors {
            errors: errors.into_iter().map(|(_, _, _, error)| error).collect(),
        })
    }
}

// Look up a definition during resolution
fn lookup_block(
    local_lookup: &[HashMap<String, usize>],
    by_name: &HashMap<String, Vec<usize>>,
    chapter: usize,
    name: &str,
) -> BlockLookup {
    if let Some(index) = local_lookup
        .get(chapter)
        .and_then(|lookup| lookup.get(name))
    {
        return BlockLookup::Found(*index);
    }
    match by_name.get(name).map(Vec::as_slice) {
        None | Some([]) => BlockLookup::Missing,
        Some([index]) => BlockLookup::Found(*index),
        Some(indices) => BlockLookup::Ambiguous(indices.to_vec()),
    }
}

// Record a resolution error in discovery order
fn push_ordered_error(
    errors: &mut Vec<(usize, usize, usize, ResolveError)>,
    sequence: &mut usize,
    chapter: usize,
    error: ResolveError,
) {
    errors.push((chapter, error.origin.line, *sequence, error));
    *sequence += 1;
}

// Recognize a root name
fn is_root_name(name: &str, quoted: bool) -> bool {
    quoted
        || Path::new(name)
            .file_name()
            .and_then(|filename| Path::new(filename).extension())
            .is_some_and(|extension| !extension.is_empty())
}

// Validate a portable relative path
pub fn validate_relative_path(name: &str) -> Result<PathBuf, InvalidPathReason> {
    if name.is_empty() {
        return Err(InvalidPathReason::Empty);
    }
    if name.ends_with(['/', '\\']) {
        return Err(InvalidPathReason::TrailingSeparator);
    }
    if name.contains('\\') {
        return Err(InvalidPathReason::Backslash);
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(InvalidPathReason::WindowsPrefix);
    }

    let path = Path::new(name);
    if path.is_absolute() {
        return Err(InvalidPathReason::Absolute);
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(component) => normalized.push(component),
            Component::ParentDir => return Err(InvalidPathReason::ParentTraversal),
            Component::CurDir => return Err(InvalidPathReason::CurrentDirectory),
            Component::RootDir => return Err(InvalidPathReason::Absolute),
            Component::Prefix(_) => return Err(InvalidPathReason::WindowsPrefix),
        }
    }
    if normalized.as_os_str().is_empty() {
        Err(InvalidPathReason::Empty)
    } else {
        Ok(normalized)
    }
}
