// src/tangler.rs
//! Clean source generation from parsed `.lit` programs.

// Tangler imports
use std::fmt;
use std::path::Path;

pub use crate::output::{
    OutputAction, OutputError, OutputErrorKind, PlannedOutput, write_planned_outputs,
    write_planned_outputs_protecting,
};
use crate::parser::{Program, SourceOrigin};
use crate::resolver::{BlockLookup, ResolveErrors, ResolvedProgram, resolve};
use crate::util::{block_reference, leading_whitespace};

// Tangle warnings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TangleWarning {
    pub origin: SourceOrigin,
    pub kind: TangleWarningKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TangleWarningKind {
    NoRoots,
}

impl fmt::Display for TangleWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            TangleWarningKind::NoRoots => write!(
                f,
                "{}: warning: no file code blocks; no code written",
                self.origin
            ),
        }
    }
}

// Tangle options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TangleOptions {
    pub headers: bool,
    pub block_separators: bool,
}

impl Default for TangleOptions {
    fn default() -> Self {
        Self {
            headers: true,
            block_separators: true,
        }
    }
}

// Tangle plan
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TanglePlan {
    pub outputs: Vec<PlannedOutput>,
    pub warnings: Vec<TangleWarning>,
}

// Tangle plan failures
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    Resolution(ResolveErrors),
    Expansion(ExpansionErrors),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolution(errors) => write!(f, "{errors}"),
            Self::Expansion(errors) => write!(f, "{errors}"),
        }
    }
}

impl std::error::Error for PlanError {}

// Expansion error model
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpansionError {
    pub origin: SourceOrigin,
    pub kind: ExpansionErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpansionErrorKind {
    UndefinedBlock {
        name: String,
    },
    AmbiguousBlock {
        name: String,
        candidates: Vec<SourceOrigin>,
    },
    ReferenceCycle {
        names: Vec<String>,
    },
    CrossChapterReferenceCycle {
        blocks: Vec<CycleBlock>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleBlock {
    pub name: String,
    pub origin: SourceOrigin,
}

// Render one expansion error
impl fmt::Display for ExpansionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: error: ", self.origin)?;
        match &self.kind {
            ExpansionErrorKind::UndefinedBlock { name } => {
                write!(f, "code block {{{name}}} is not defined")
            }
            ExpansionErrorKind::AmbiguousBlock { name, candidates } => {
                write!(
                    f,
                    "code block {{{name}}} is ambiguous; definitions are at {}",
                    candidates
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ExpansionErrorKind::ReferenceCycle { names } => {
                write!(f, "code block reference cycle: {}", names.join(" -> "))
            }
            ExpansionErrorKind::CrossChapterReferenceCycle { blocks } => write!(
                f,
                "code block reference cycle: {}",
                blocks
                    .iter()
                    .map(|block| format!("{} at {}", block.name, block.origin))
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
        }
    }
}

impl std::error::Error for ExpansionError {}

// Expansion error collection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpansionErrors {
    errors: Vec<ExpansionError>,
}

impl ExpansionErrors {
    pub fn as_slice(&self) -> &[ExpansionError] {
        &self.errors
    }

    pub fn into_vec(self) -> Vec<ExpansionError> {
        self.errors
    }
}

impl fmt::Display for ExpansionErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.errors.first() {
            write!(f, "{first}")?;
            if self.errors.len() > 1 {
                write!(f, " (and {} more expansion errors)", self.errors.len() - 1)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ExpansionErrors {}

// Plan tangle function
pub fn plan_tangle(program: &Program) -> Result<TanglePlan, PlanError> {
    plan_tangle_with_options(program, TangleOptions::default())
}

pub fn plan_tangle_with_options(
    program: &Program,
    options: TangleOptions,
) -> Result<TanglePlan, PlanError> {
    let resolved = resolve(program).map_err(PlanError::Resolution)?;
    let mut outputs = Vec::with_capacity(resolved.roots.len());
    let mut errors = Vec::new();

    for root in &resolved.roots {
        let (bytes, mut root_errors) = expand_root(&resolved, root.block, &options);
        errors.append(&mut root_errors);
        outputs.push(PlannedOutput {
            relative_path: root.path.clone(),
            bytes: bytes.into_bytes(),
            origin: root.origin.clone(),
        });
    }

    if !errors.is_empty() {
        return Err(PlanError::Expansion(ExpansionErrors { errors }));
    }

    let warnings = if resolved.roots.is_empty() {
        vec![TangleWarning {
            origin: program.origin.clone(),
            kind: TangleWarningKind::NoRoots,
        }]
    } else {
        Vec::new()
    };
    Ok(TanglePlan { outputs, warnings })
}

// Expansion frames
struct ExpansionFrame {
    block: usize,
    next_line: usize,
    indentation: String,
    entered: bool,
    previous_reference_start: Option<usize>,
}

// First nonblank indentation
#[derive(Clone)]
enum FirstIndentation {
    Unknown,
    Visiting,
    Known(Option<String>),
}

struct FirstIndentationFrame {
    block: usize,
    next_line: usize,
}

fn first_nonblank_indentation(
    resolved: &ResolvedProgram,
    root: usize,
    states: &mut [FirstIndentation],
) -> Option<String> {
    if let FirstIndentation::Known(indentation) = &states[root] {
        return indentation.clone();
    }

    states[root] = FirstIndentation::Visiting;
    let mut frames = vec![FirstIndentationFrame {
        block: root,
        next_line: 0,
    }];

    while !frames.is_empty() {
        let depth = frames.len() - 1;
        let block_index = frames[depth].block;
        let block = resolved.block(block_index);
        if frames[depth].next_line == block.lines.len() {
            states[block_index] = FirstIndentation::Known(None);
            frames.pop();
            continue;
        }

        let line_index = frames[depth].next_line;
        let line = &block.lines[line_index];
        if let Some(name) = block_reference(&line.text) {
            let line_chapter = block.line_chapters[line_index];
            let BlockLookup::Found(target) = resolved.lookup(line_chapter, name) else {
                frames[depth].next_line += 1;
                continue;
            };
            match states[target].clone() {
                FirstIndentation::Unknown => {
                    states[target] = FirstIndentation::Visiting;
                    frames.push(FirstIndentationFrame {
                        block: target,
                        next_line: 0,
                    });
                }
                FirstIndentation::Visiting | FirstIndentation::Known(None) => {
                    frames[depth].next_line += 1;
                }
                FirstIndentation::Known(Some(child_indentation)) => {
                    let mut indentation = leading_whitespace(&line.text).to_owned();
                    indentation.push_str(&child_indentation);
                    states[block_index] = FirstIndentation::Known(Some(indentation));
                    frames.pop();
                }
            }
        } else if line.text.trim().is_empty() {
            frames[depth].next_line += 1;
        } else {
            states[block_index] =
                FirstIndentation::Known(Some(leading_whitespace(&line.text).to_owned()));
            frames.pop();
        }
    }

    let FirstIndentation::Known(indentation) = &states[root] else {
        unreachable!("completed lookahead has a known result");
    };
    indentation.clone()
}

// Block-separator helper
fn ends_with_blank_line(output: &str) -> bool {
    let Some(without_ending) = output.strip_suffix('\n') else {
        return false;
    };
    without_ending
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.chars().all(char::is_whitespace))
}

// Expand one root
fn expand_root(
    resolved: &ResolvedProgram,
    root: usize,
    options: &TangleOptions,
) -> (String, Vec<ExpansionError>) {
    // Initialize root expansion
    let mut output = String::new();
    let mut errors = Vec::new();
    let mut active = Vec::new();
    let mut active_positions = vec![None; resolved.blocks.len()];
    let mut first_indentations = vec![FirstIndentation::Unknown; resolved.blocks.len()];
    let mut frames = vec![ExpansionFrame {
        block: root,
        next_line: 0,
        indentation: String::new(),
        entered: false,
        previous_reference_start: None,
    }];

    while !frames.is_empty() {
        // Enter an expansion frame
        let depth = frames.len() - 1;
        let block_index = frames[depth].block;
        let block = resolved.block(block_index);

        if !frames[depth].entered {
            if options.headers && !block.comment_string.is_empty() && !block.no_header {
                output.push_str(&frames[depth].indentation);
                if let Some(indentation) = first_nonblank_indentation(
                    resolved,
                    block_index,
                    &mut first_indentations,
                ) {
                    output.push_str(&indentation);
                }
                output.push_str(&block.comment_string.replace("%s", &block.name));
                output.push('\n');
            }
            frames[depth].entered = true;
            active_positions[block_index] = Some(active.len());
            active.push(block_index);
        }

        // Finish an expansion frame
        if frames[depth].next_line == block.lines.len() {
            frames.pop();
            let exited = active.pop().expect("entered frame has an active block");
            active_positions[exited] = None;
            continue;
        }

        // Process the next expansion line
        let line_index = frames[depth].next_line;
        let line = block.lines[line_index].clone();
        let line_chapter = block.line_chapters[line_index];
        frames[depth].next_line += 1;
        let indentation = frames[depth].indentation.clone();
        if let Some(name) = block_reference(&line.text) {
            if options.block_separators
                && frames[depth]
                    .previous_reference_start
                    .is_some_and(|start| !ends_with_blank_line(&output[start..]))
            {
                output.push('\n');
            }
            frames[depth].previous_reference_start = Some(output.len());
            let name = name.to_owned();
            let target = match resolved.lookup(line_chapter, &name) {
                BlockLookup::Found(target) => target,
                BlockLookup::Missing => {
                    errors.push(ExpansionError {
                        origin: line.origin,
                        kind: ExpansionErrorKind::UndefinedBlock { name },
                    });
                    continue;
                }
                BlockLookup::Ambiguous(indices) => {
                    errors.push(ExpansionError {
                        origin: line.origin,
                        kind: ExpansionErrorKind::AmbiguousBlock {
                            name,
                            candidates: indices
                                .iter()
                                .map(|index| resolved.block(*index).origin.clone())
                                .collect(),
                        },
                    });
                    continue;
                }
            };

            if let Some(position) = active_positions[target] {
                let mut cycle = active[position..].to_vec();
                cycle.push(target);
                let first_chapter = resolved.block(cycle[0]).chapter;
                let crosses_chapters = cycle
                    .iter()
                    .any(|index| resolved.block(*index).chapter != first_chapter);
                let kind = if crosses_chapters {
                    ExpansionErrorKind::CrossChapterReferenceCycle {
                        blocks: cycle
                            .iter()
                            .map(|index| CycleBlock {
                                name: resolved.block(*index).name.clone(),
                                origin: resolved.block(*index).origin.clone(),
                            })
                            .collect(),
                    }
                } else {
                    ExpansionErrorKind::ReferenceCycle {
                        names: cycle
                            .iter()
                            .map(|index| resolved.block(*index).name.clone())
                            .collect(),
                    }
                };
                errors.push(ExpansionError {
                    origin: line.origin,
                    kind,
                });
                continue;
            }

            frames.push(ExpansionFrame {
                block: target,
                next_line: 0,
                indentation: indentation + leading_whitespace(&line.text),
                entered: false,
                previous_reference_start: None,
            });
        } else {
            frames[depth].previous_reference_start = None;
            if !line.text.is_empty() {
                output.push_str(&indentation);
            }
            output.push_str(&line.text);
            output.push('\n');
        }
    }

    (output, errors)
}

// Write tangled outputs function
pub fn write_outputs(
    plan: &TanglePlan,
    output_directory: &Path,
    input: &Path,
) -> Result<(), OutputError> {
    write_planned_outputs(&plan.outputs, output_directory, input)
}
