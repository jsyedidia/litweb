// src/book.rs
//! Pure book-manifest parsing and explicit chapter loading.

// Book imports
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::inline::{fenced_line_mask, table_line_mask};
use crate::parser::{
    BookChapter, BookMetadata, Command, CommandKind, Line, ParseErrors, Program,
    ProgramKind, SourceOrigin, command_parts, is_deferred_command, parse_chapter_bytes,
};
use crate::resolver::{InvalidPathReason, validate_relative_path};

// Book manifest model
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookManifest {
    pub origin: SourceOrigin,
    pub title: String,
    pub title_origin: SourceOrigin,
    pub commands: Vec<Command>,
    pub introduction: String,
    pub introduction_lines: Vec<Line>,
    pub chapters: Vec<ManifestChapter>,
    pub file: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestChapter {
    pub navigation_label: String,
    pub origin: SourceOrigin,
    pub source_path: PathBuf,
    pub major_number: usize,
    pub minor_number: usize,
}

// Book error model
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookError {
    pub origin: SourceOrigin,
    pub kind: BookErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookErrorKind {
    ReadManifest {
        path: PathBuf,
        kind: io::ErrorKind,
    },
    InvalidUtf8 {
        valid_up_to: usize,
    },
    MissingBookMarker,
    DuplicateBookMarker,
    BookMarkerArguments,
    MissingTitle,
    DuplicateTitle,
    EmptyTitle,
    NoChapters,
    MissingCommandArguments {
        name: String,
    },
    UnsupportedCommand {
        name: String,
    },
    MalformedChapterEntry {
        text: String,
    },
    EmptyChapterLabel,
    EmptyChapterPath,
    MinorBeforeMajor,
    InvalidManifestPath {
        path: PathBuf,
    },
    InvalidChapterPath {
        path: PathBuf,
        reason: InvalidPathReason,
    },
    DuplicateChapterPath {
        path: PathBuf,
    },
    ReadChapter {
        path: PathBuf,
        kind: io::ErrorKind,
    },
    ChapterParse {
        path: PathBuf,
        errors: ParseErrors,
    },
}

// Render one book error
impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: error: ", self.origin)?;
        match &self.kind {
            BookErrorKind::ReadManifest { path, kind } => {
                write!(f, "cannot read book manifest {}: {kind}", path.display())
            }
            BookErrorKind::InvalidUtf8 { valid_up_to } => {
                write!(f, "book manifest is not UTF-8 at byte {valid_up_to}")
            }
            BookErrorKind::MissingBookMarker => {
                f.write_str("book manifest has no @book marker")
            }
            BookErrorKind::DuplicateBookMarker => {
                f.write_str("book manifest has more than one @book marker")
            }
            BookErrorKind::BookMarkerArguments => {
                f.write_str("the @book marker does not accept arguments")
            }
            BookErrorKind::MissingTitle => f.write_str("book manifest has no @title"),
            BookErrorKind::DuplicateTitle => {
                f.write_str("book manifest has more than one @title")
            }
            BookErrorKind::EmptyTitle => f.write_str("book title is empty"),
            BookErrorKind::NoChapters => f.write_str("book manifest has no chapters"),
            BookErrorKind::MissingCommandArguments { name } => {
                write!(f, "command {name} requires arguments")
            }
            BookErrorKind::UnsupportedCommand { name } => {
                write!(f, "command {name} is not supported in a book manifest")
            }
            BookErrorKind::MalformedChapterEntry { text } => {
                write!(f, "malformed chapter entry: {text}")
            }
            BookErrorKind::EmptyChapterLabel => {
                f.write_str("chapter navigation label is empty")
            }
            BookErrorKind::EmptyChapterPath => f.write_str("chapter path is empty"),
            BookErrorKind::MinorBeforeMajor => {
                f.write_str("a minor chapter must follow a major chapter")
            }
            BookErrorKind::InvalidManifestPath { path } => {
                write!(
                    f,
                    "book manifest must have a usable .lit filename: {}",
                    path.display()
                )
            }
            BookErrorKind::InvalidChapterPath { path, reason } => {
                write!(f, "invalid chapter path {}: {reason}", path.display())
            }
            BookErrorKind::DuplicateChapterPath { path } => {
                write!(
                    f,
                    "chapter path is listed more than once: {}",
                    path.display()
                )
            }
            BookErrorKind::ReadChapter { path, kind } => {
                write!(f, "cannot read chapter {}: {kind}", path.display())
            }
            BookErrorKind::ChapterParse { path, errors } => {
                write!(f, "cannot parse chapter {}: {errors}", path.display())
            }
        }
    }
}

impl std::error::Error for BookError {}

// Book error collection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookErrors {
    errors: Vec<BookError>,
}

impl BookErrors {
    pub fn as_slice(&self) -> &[BookError] {
        &self.errors
    }

    pub fn into_vec(self) -> Vec<BookError> {
        self.errors
    }
}

impl fmt::Display for BookErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.errors.first() {
            write!(f, "{first}")?;
            if self.errors.len() > 1 {
                write!(f, " (and {} more book errors)", self.errors.len() - 1)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for BookErrors {}

// Manifest parse entry points
pub fn parse_book_manifest_bytes(
    file: impl Into<String>,
    source: &[u8],
) -> Result<BookManifest, BookErrors> {
    let file = file.into();
    match std::str::from_utf8(source) {
        Ok(source) => parse_book_manifest_source(file, source),
        Err(error) => {
            let valid_up_to = error.valid_up_to();
            let line = source[..valid_up_to]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
                + 1;
            Err(BookErrors {
                errors: vec![BookError {
                    origin: SourceOrigin::new(file, line),
                    kind: BookErrorKind::InvalidUtf8 { valid_up_to },
                }],
            })
        }
    }
}

pub fn parse_book_manifest_str(
    file: impl Into<String>,
    source: &str,
) -> Result<BookManifest, BookErrors> {
    parse_book_manifest_source(file.into(), source)
}

// Parse a book manifest source
fn parse_book_manifest_source(
    file: String,
    source: &str,
) -> Result<BookManifest, BookErrors> {
    // Initialize manifest parsing
    let origin = SourceOrigin::new(&file, 1);
    let mut errors = Vec::new();
    let mut book_marker = None;
    let mut title = None;
    let mut commands = Vec::new();
    let mut introduction = String::new();
    let mut introduction_lines = Vec::new();
    let mut chapters = Vec::new();
    let mut chapter_sequence = ChapterSequence::default();
    let source_lines: Vec<_> = source
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let fenced_lines = fenced_line_mask(&source_lines);
    let table_lines = table_line_mask(&source_lines);

    if !valid_manifest_path(&file) {
        errors.push(BookError {
            origin: origin.clone(),
            kind: BookErrorKind::InvalidManifestPath {
                path: PathBuf::from(&file),
            },
        });
    }

    // Read manifest commands and introduction prose
    for (index, line) in source_lines.iter().copied().enumerate() {
        let line_origin = SourceOrigin::new(&file, index + 1);

        if fenced_lines[index] || table_lines[index] {
            append_introduction_line(
                &mut introduction,
                &mut introduction_lines,
                line,
                line_origin,
            );
            continue;
        }

        if let Some((name, arguments)) = command_parts(line) {
            match name {
                "@book" => {
                    let duplicate = book_marker.replace(index + 1).is_some();
                    if !arguments.is_empty() {
                        errors.push(BookError {
                            origin: line_origin.clone(),
                            kind: BookErrorKind::BookMarkerArguments,
                        });
                    }
                    if duplicate {
                        errors.push(BookError {
                            origin: line_origin,
                            kind: BookErrorKind::DuplicateBookMarker,
                        });
                    }
                    continue;
                }
                "@title" => {
                    if title.is_some() {
                        errors.push(BookError {
                            origin: line_origin,
                            kind: BookErrorKind::DuplicateTitle,
                        });
                    } else if arguments.is_empty() {
                        errors.push(BookError {
                            origin: line_origin.clone(),
                            kind: BookErrorKind::EmptyTitle,
                        });
                        title = Some((String::new(), line_origin));
                    } else {
                        title = Some((arguments.to_owned(), line_origin));
                    }
                    continue;
                }
                "@code_type" | "@comment_type" | "@colorscheme" => {
                    if arguments.is_empty() {
                        errors.push(BookError {
                            origin: line_origin,
                            kind: BookErrorKind::MissingCommandArguments {
                                name: name.to_owned(),
                            },
                        });
                    } else {
                        commands.push(Command {
                            kind: match name {
                                "@code_type" => CommandKind::CodeType,
                                "@comment_type" => CommandKind::CommentType,
                                "@colorscheme" => CommandKind::ColorScheme,
                                _ => {
                                    unreachable!("the match arm selected a known command")
                                }
                            },
                            name: name.to_owned(),
                            arguments: if arguments == "none" {
                                String::new()
                            } else {
                                arguments.to_owned()
                            },
                            origin: line_origin,
                        });
                    }
                    continue;
                }
                "@s" => {
                    errors.push(BookError {
                        origin: line_origin,
                        kind: BookErrorKind::UnsupportedCommand {
                            name: name.to_owned(),
                        },
                    });
                    continue;
                }
                name if is_deferred_command(name) => {
                    errors.push(BookError {
                        origin: line_origin,
                        kind: BookErrorKind::UnsupportedCommand {
                            name: name.to_owned(),
                        },
                    });
                    continue;
                }
                _ => {}
            }
        }

        match chapter_entry(line) {
            EntryLine::Prose => append_introduction_line(
                &mut introduction,
                &mut introduction_lines,
                line,
                line_origin,
            ),
            EntryLine::Malformed => errors.push(BookError {
                origin: line_origin,
                kind: BookErrorKind::MalformedChapterEntry {
                    text: line.to_owned(),
                },
            }),
            EntryLine::Candidate {
                indented,
                label,
                target,
            } => match validate_and_number_chapter(
                indented,
                label,
                target,
                &line_origin,
                &mut chapter_sequence,
                &mut errors,
            ) {
                CandidateResult::Introduction => append_introduction_line(
                    &mut introduction,
                    &mut introduction_lines,
                    line,
                    line_origin,
                ),
                CandidateResult::Chapter(chapter) => chapters.push(chapter),
                CandidateResult::Invalid => {}
            },
        }
    }

    // Finish the manifest
    if book_marker.is_none() {
        errors.push(BookError {
            origin: origin.clone(),
            kind: BookErrorKind::MissingBookMarker,
        });
    }
    if title.is_none() {
        errors.push(BookError {
            origin: origin.clone(),
            kind: BookErrorKind::MissingTitle,
        });
    }
    if chapters.is_empty() {
        errors.push(BookError {
            origin: origin.clone(),
            kind: BookErrorKind::NoChapters,
        });
    }

    if !errors.is_empty() {
        errors.sort_by_key(|error| error.origin.line);
        return Err(BookErrors { errors });
    }

    let (title, title_origin) = title.expect("missing title returned above");
    Ok(BookManifest {
        origin,
        title,
        title_origin,
        commands,
        introduction,
        introduction_lines,
        chapters,
        file,
        text: source.to_owned(),
    })
}

// Append one manifest introduction line
fn append_introduction_line(
    introduction: &mut String,
    lines: &mut Vec<Line>,
    text: &str,
    origin: SourceOrigin,
) {
    introduction.push_str(text);
    introduction.push('\n');
    lines.push(Line {
        origin,
        text: text.to_owned(),
    });
}

// Validate and number one manifest chapter
enum CandidateResult {
    Introduction,
    Chapter(ManifestChapter),
    Invalid,
}

#[derive(Default)]
struct ChapterSequence {
    paths: HashSet<PathBuf>,
    major_number: usize,
    minor_number: usize,
}

fn validate_and_number_chapter(
    indented: bool,
    label: String,
    target: String,
    origin: &SourceOrigin,
    sequence: &mut ChapterSequence,
    errors: &mut Vec<BookError>,
) -> CandidateResult {
    if label.is_empty() {
        errors.push(BookError {
            origin: origin.clone(),
            kind: BookErrorKind::EmptyChapterLabel,
        });
    }
    if target.is_empty() {
        errors.push(BookError {
            origin: origin.clone(),
            kind: BookErrorKind::EmptyChapterPath,
        });
        return CandidateResult::Invalid;
    }

    let target_without_separator = target.trim_end_matches(['/', '\\']);
    let looks_like_chapter = Path::new(&target).extension() == Some(OsStr::new("lit"))
        || Path::new(target_without_separator).extension() == Some(OsStr::new("lit"));
    if !looks_like_chapter {
        return CandidateResult::Introduction;
    }

    let source_path = match validate_relative_path(&target) {
        Ok(path) => path,
        Err(reason) => {
            errors.push(BookError {
                origin: origin.clone(),
                kind: BookErrorKind::InvalidChapterPath {
                    path: PathBuf::from(target),
                    reason,
                },
            });
            return CandidateResult::Invalid;
        }
    };
    if !sequence.paths.insert(source_path.clone()) {
        errors.push(BookError {
            origin: origin.clone(),
            kind: BookErrorKind::DuplicateChapterPath {
                path: source_path.clone(),
            },
        });
    }

    if indented {
        if sequence.major_number == 0 {
            errors.push(BookError {
                origin: origin.clone(),
                kind: BookErrorKind::MinorBeforeMajor,
            });
        }
        sequence.minor_number += 1;
    } else {
        sequence.major_number += 1;
        sequence.minor_number = 0;
    }

    CandidateResult::Chapter(ManifestChapter {
        navigation_label: label,
        origin: origin.clone(),
        source_path,
        major_number: sequence.major_number,
        minor_number: sequence.minor_number,
    })
}

// Check the manifest filename
fn valid_manifest_path(file: &str) -> bool {
    let Some(filename) = Path::new(file).file_name() else {
        return false;
    };
    let filename_path = Path::new(filename);
    filename_path.extension() == Some(OsStr::new("lit"))
        && filename_path
            .file_stem()
            .is_some_and(|stem| !stem.is_empty())
}

// Book entry parser
enum EntryLine {
    Prose,
    Malformed,
    Candidate {
        indented: bool,
        label: String,
        target: String,
    },
}

fn chapter_entry(line: &str) -> EntryLine {
    let without_trailing = line.trim_end_matches([' ', '\t']);
    let content = without_trailing.trim_start_matches([' ', '\t']);
    let indented = content.len() != without_trailing.len();
    if !content.starts_with('[') {
        return EntryLine::Prose;
    }

    let Some(label_end) = content.find(']') else {
        return EntryLine::Malformed;
    };
    if content.as_bytes().get(label_end + 1) != Some(&b'(') {
        return EntryLine::Prose;
    }
    let Some(target_end) = content[label_end + 2..].find(')') else {
        return EntryLine::Malformed;
    };
    let target_end = label_end + 2 + target_end;
    if target_end + 1 != content.len() {
        return EntryLine::Prose;
    }

    EntryLine::Candidate {
        indented,
        label: content[1..label_end].trim().to_owned(),
        target: content[label_end + 2..target_end].trim().to_owned(),
    }
}

// Recognize a book manifest
pub fn is_book_manifest(source: &[u8]) -> bool {
    let exact_marker = |line: &[u8]| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let end = line
            .iter()
            .rposition(|byte| !matches!(byte, b' ' | b'\t'))
            .map_or(0, |index| index + 1);
        &line[..end] == b"@book"
    };
    let Ok(text) = std::str::from_utf8(source) else {
        return source.split(|byte| *byte == b'\n').any(exact_marker);
    };
    let lines: Vec<_> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let fenced_lines = fenced_line_mask(&lines);
    let table_lines = table_line_mask(&lines);
    source
        .split(|byte| *byte == b'\n')
        .zip(fenced_lines.into_iter().zip(table_lines))
        .any(|(line, (fenced, table))| !fenced && !table && exact_marker(line))
}

// Book loading entry points
pub fn load_book(path: impl AsRef<Path>) -> Result<Program, BookErrors> {
    let path = path.as_ref();
    let file = path.to_string_lossy().into_owned();
    let source = fs::read(path).map_err(|error| BookErrors {
        errors: vec![BookError {
            origin: SourceOrigin::new(&file, 1),
            kind: BookErrorKind::ReadManifest {
                path: path.to_path_buf(),
                kind: error.kind(),
            },
        }],
    })?;
    load_book_bytes(path, &source)
}

pub fn load_book_bytes(
    path: impl AsRef<Path>,
    source: &[u8],
) -> Result<Program, BookErrors> {
    let path = path.as_ref();
    let file = path.to_string_lossy().into_owned();
    let manifest = parse_book_manifest_bytes(file, source)?;
    load_manifest(path, manifest)
}

// Load manifest chapters
fn load_manifest(
    manifest_path: &Path,
    manifest: BookManifest,
) -> Result<Program, BookErrors> {
    let directory = manifest_path.parent().unwrap_or_else(|| Path::new(""));
    let mut chapters = Vec::with_capacity(manifest.chapters.len());
    let mut errors = Vec::new();

    for entry in &manifest.chapters {
        let chapter_path = directory.join(&entry.source_path);
        let source = match fs::read(&chapter_path) {
            Ok(source) => source,
            Err(error) => {
                errors.push(BookError {
                    origin: entry.origin.clone(),
                    kind: BookErrorKind::ReadChapter {
                        path: entry.source_path.clone(),
                        kind: error.kind(),
                    },
                });
                continue;
            }
        };
        let book = BookChapter {
            navigation_label: entry.navigation_label.clone(),
            label_origin: entry.origin.clone(),
            source_path: entry.source_path.clone(),
        };
        match parse_chapter_bytes(
            chapter_path.to_string_lossy().into_owned(),
            &source,
            manifest.commands.clone(),
            entry.major_number,
            entry.minor_number,
            book,
        ) {
            Ok(chapter) => chapters.push(chapter),
            Err(parse_errors) => errors.push(BookError {
                origin: entry.origin.clone(),
                kind: BookErrorKind::ChapterParse {
                    path: entry.source_path.clone(),
                    errors: parse_errors,
                },
            }),
        }
    }

    if !errors.is_empty() {
        return Err(BookErrors { errors });
    }

    Ok(Program {
        kind: ProgramKind::Book(BookMetadata {
            introduction: manifest.introduction,
            introduction_lines: manifest.introduction_lines,
        }),
        origin: manifest.origin,
        title: manifest.title,
        title_origin: Some(manifest.title_origin),
        commands: manifest.commands,
        chapters,
        file: manifest.file,
        text: manifest.text,
    })
}
