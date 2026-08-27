// src/latex.rs
//! Deterministic LaTeX generation from parsed `.lit` programs.

// LaTeX imports
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::identifier::{
    IdentifierRole, analyze_resolved as analyze_resolved_identifiers,
    rust_identifier_offsets,
};
use crate::inline::TableAlignment;
use crate::output::PlannedOutput;
use crate::parser::{Block, BlockKind, Chapter, Modifier, Program, SourceOrigin};
use crate::prose::{
    FencedProse, InlineElement, InlineSource, ProseBlock, ProseList, ProseTable,
    TableCell, inline_elements, prose_block,
};
use crate::resolver::{BlockLookup, ResolveErrors, ResolvedProgram, resolve};
use crate::util::{block_reference, leading_whitespace};
use crate::woven::{
    LocatedIdentifierIndex, LocatedMiniIndex, LocatedMiniOccurrence, SectionLocation,
    WeaveIndex, WeaveLayout, WeaveSection, collect_locations, locate_identifiers,
    locate_mini_identifiers,
};

const STYLE_PATH: &str = "litweb-latex/litweb.sty";
const STYLE_BYTES: &[u8] = include_bytes!("../assets/latex/litweb.sty");

// LaTeX public results
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexOptions {
    pub identifier_index: bool,
    pub font_size: LatexFontSize,
    pub chapter_opening: LatexChapterOpening,
}

impl Default for LatexOptions {
    fn default() -> Self {
        Self {
            identifier_index: true,
            font_size: LatexFontSize::default(),
            chapter_opening: LatexChapterOpening::default(),
        }
    }
}

/// A base font size supported by LaTeX's standard document classes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LatexFontSize {
    /// Request 10-point base text.
    TenPoint,
    /// Request the standard 11-point class option.
    ElevenPoint,
    /// Use Litweb's 12-point default.
    #[default]
    TwelvePoint,
}

impl LatexFontSize {
    fn class_option(self) -> Option<&'static str> {
        match self {
            Self::TenPoint => None,
            Self::ElevenPoint => Some("11pt"),
            Self::TwelvePoint => Some("12pt"),
        }
    }
}

/// The page side on which major chapters of a LaTeX book begin.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LatexChapterOpening {
    /// Begin each major chapter on a right-hand odd page.
    #[default]
    Right,
    /// Begin each major chapter on a left-hand even page.
    Left,
}

impl LatexChapterOpening {
    fn style_command(self) -> &'static str {
        match self {
            Self::Right => "\\LitwebChaptersOpenRight\n",
            Self::Left => "\\LitwebChaptersOpenLeft\n",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexPlan {
    pub outputs: Vec<PlannedOutput>,
}

// Plan LaTeX
pub fn plan_latex(program: &Program) -> Result<LatexPlan, LatexPlanError> {
    plan_latex_with_options(program, LatexOptions::default())
}

pub fn plan_latex_with_options(
    program: &Program,
    options: LatexOptions,
) -> Result<LatexPlan, LatexPlanError> {
    let resolved = resolve(program).map_err(LatexPlanError::Resolution)?;
    let layout = WeaveLayout::new(program);
    let index = collect_locations(program, &resolved, &layout);
    let analyzed_identifiers = options
        .identifier_index
        .then(|| analyze_resolved_identifiers(program, &resolved));
    let identifiers = if let Some(analyzed) = &analyzed_identifiers {
        locate_identifiers(analyzed, &layout)
    } else {
        LocatedIdentifierIndex::default()
    };
    let mini_index = if let Some(analyzed) = &analyzed_identifiers {
        locate_mini_identifiers(analyzed, &layout)
    } else {
        LocatedMiniIndex::default()
    };
    let output_path = latex_output_name(program).map_err(|error| {
        LatexPlanError::Latex(LatexErrors {
            errors: vec![error],
        })
    })?;
    let (document, errors) = render_document(
        program,
        &resolved,
        &layout,
        &index,
        &identifiers,
        &mini_index,
        &options,
    );
    if !errors.is_empty() {
        return Err(LatexPlanError::Latex(LatexErrors { errors }));
    }

    Ok(LatexPlan {
        outputs: vec![
            PlannedOutput {
                relative_path: output_path,
                bytes: document.into_bytes(),
                origin: program.origin.clone(),
            },
            PlannedOutput {
                relative_path: PathBuf::from(STYLE_PATH),
                bytes: STYLE_BYTES.to_vec(),
                origin: program.origin.clone(),
            },
        ],
    })
}

fn latex_output_name(program: &Program) -> Result<PathBuf, LatexError> {
    let path = Path::new(&program.file);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| LatexError {
            origin: program.origin.clone(),
            kind: LatexErrorKind::InvalidOutputName,
        })?;
    Ok(PathBuf::from(format!("{stem}.tex")))
}

// Render a LaTeX document
fn render_document(
    program: &Program,
    resolved: &ResolvedProgram,
    layout: &WeaveLayout,
    index: &WeaveIndex,
    identifiers: &LocatedIdentifierIndex,
    mini_index: &LocatedMiniIndex,
    options: &LatexOptions,
) -> (String, Vec<LatexError>) {
    if program.is_book() {
        return render_book_document(
            program,
            resolved,
            layout,
            index,
            identifiers,
            mini_index,
            options,
        );
    }

    let mut errors = Vec::new();
    let mut output = String::new();
    push_document_class(&mut output, "article", options.font_size);
    output.push_str("\\input{litweb-latex/litweb.sty}\n\\title{");
    push_latex_text(&mut output, &program.title);
    output.push_str("}\n\\author{}\n\\date{}\n");
    render_mini_index_registry(&mut output, mini_index, Some(0));
    output.push_str("\\begin{document}\n\\maketitle\n");
    let mini_index = (!mini_index.meanings.is_empty()).then_some(mini_index);
    if mini_index.is_some() {
        output.push_str("\\LitwebMiniIndexBegin\n");
    }

    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        render_chapter_sections(
            &mut output,
            chapter,
            &layout.chapters[chapter_index],
            chapter_index,
            LatexReferences { resolved, index },
            mini_index,
            &mut errors,
        );
    }
    if mini_index.is_some() {
        output.push_str("\\LitwebMiniIndexEnd\n");
    }
    render_identifier_index(&mut output, identifiers, Some(0), false);
    output.push_str("\\end{document}\n");
    (output, errors)
}

fn push_document_class(output: &mut String, class: &str, font_size: LatexFontSize) {
    output.push_str("\\documentclass");
    if let Some(option) = font_size.class_option() {
        output.push('[');
        output.push_str(option);
        output.push(']');
    }
    output.push('{');
    output.push_str(class);
    output.push_str("}\n");
}

// Render a LaTeX book
fn render_book_document(
    program: &Program,
    resolved: &ResolvedProgram,
    layout: &WeaveLayout,
    index: &WeaveIndex,
    identifiers: &LocatedIdentifierIndex,
    mini_index: &LocatedMiniIndex,
    options: &LatexOptions,
) -> (String, Vec<LatexError>) {
    let book = program.book().expect("the caller selected a book");
    let mut errors = Vec::new();
    let mut output = String::new();
    push_document_class(&mut output, "book", options.font_size);
    output.push_str("\\input{litweb-latex/litweb.sty}\n");
    output.push_str(options.chapter_opening.style_command());
    output.push_str("\\title{");
    push_latex_text(&mut output, &program.title);
    output.push_str("}\n\\author{}\n\\date{}\n");
    render_mini_index_registry(&mut output, mini_index, None);
    output.push_str("\\begin{document}\n\\frontmatter\n\\maketitle\n");

    let introduction = Block {
        origin: program.origin.clone(),
        kind: BlockKind::Prose,
        lines: book.introduction_lines.clone(),
    };
    render_prose(
        &mut output,
        &introduction,
        None,
        resolved,
        index,
        None,
        &mut errors,
    );
    output
        .push_str("\\tableofcontents\n\\LitwebPrepareLiterateMainMatter\n\\mainmatter\n");

    let mini_index = (!mini_index.meanings.is_empty()).then_some(mini_index);
    let mut major_chapter_active = false;
    let mut mini_index_active = false;
    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        if chapter.minor_number == 0 {
            if major_chapter_active {
                if mini_index_active {
                    output.push_str("\\LitwebMiniIndexEnd\n");
                    mini_index_active = false;
                }
                output.push_str("\\LitwebLiterateChapterEnd\n");
            }
            render_book_chapter_heading(&mut output, chapter, true);
            if mini_index.is_some() {
                output.push_str("\\LitwebMiniIndexBegin\n");
                mini_index_active = true;
            }
            major_chapter_active = true;
        } else {
            render_book_chapter_heading(&mut output, chapter, false);
        }
        render_chapter_sections(
            &mut output,
            chapter,
            &layout.chapters[chapter_index],
            chapter_index,
            LatexReferences { resolved, index },
            mini_index.filter(|_| mini_index_active),
            &mut errors,
        );
    }
    if major_chapter_active {
        if mini_index_active {
            output.push_str("\\LitwebMiniIndexEnd\n");
        }
        output.push_str("\\LitwebLiterateChapterEnd\n");
    }

    if !identifiers.entries.is_empty() {
        output.push_str("\\backmatter\n");
    }
    render_identifier_index(&mut output, identifiers, None, true);
    output.push_str("\\end{document}\n");
    (output, errors)
}

fn render_book_chapter_heading(
    output: &mut String,
    chapter: &Chapter,
    literate_boundary: bool,
) {
    let metadata = chapter
        .book
        .as_ref()
        .expect("a loaded book chapter has book metadata");
    output.push_str(if literate_boundary {
        "\\LitwebLiterateChapter["
    } else if chapter.minor_number == 0 {
        "\\chapter["
    } else {
        "\\section["
    });
    push_latex_text(output, &metadata.navigation_label);
    output.push_str("]{");
    push_latex_text(output, &chapter.title);
    output.push_str("}\n");
}

// Render LaTeX presentation sections
#[derive(Clone, Copy)]
struct LatexReferences<'a> {
    resolved: &'a ResolvedProgram,
    index: &'a WeaveIndex,
}

fn render_chapter_sections(
    output: &mut String,
    chapter: &Chapter,
    sections: &[WeaveSection],
    chapter_index: usize,
    references: LatexReferences<'_>,
    mini_index: Option<&LocatedMiniIndex>,
    errors: &mut Vec<LatexError>,
) {
    for section in sections {
        let source = &chapter.sections[section.source_section];
        output.push_str("\\LitwebSection{");
        output.push_str(&latex_anchor(&section.location));
        output.push_str("}{");
        let _ = write!(output, "{}", section.location.section);
        output.push('}');
        if section.source_section_start && !source.title.is_empty() {
            output.push_str("\\LitwebSectionTitle{");
            push_latex_text(output, &source.title);
            if !source.title.ends_with(['.', '?', '!']) {
                output.push('.');
            }
            output.push_str("}\n");
        } else {
            output.push('\n');
        }

        if let Some(prose) = section.prose {
            render_prose(
                output,
                &source.blocks[prose],
                Some(chapter_index),
                references.resolved,
                references.index,
                mini_index,
                errors,
            );
        }
        if let Some(code) = section.code {
            render_code_block(
                output,
                &source.blocks[code],
                &section.location,
                references.resolved,
                references.index,
                mini_index,
                errors,
            );
        }
    }
}

fn latex_anchor(location: &SectionLocation) -> String {
    format!("litweb-{}-{}", location.chapter + 1, location.section)
}

fn render_location_link(
    output: &mut String,
    location: &SectionLocation,
    current_chapter: Option<usize>,
) {
    output.push_str("\\hyperlink{");
    output.push_str(&latex_anchor(location));
    output.push_str("}{");
    push_latex_text(output, &location.label(current_chapter));
    output.push('}');
}

fn render_mini_index_registry(
    output: &mut String,
    index: &LocatedMiniIndex,
    current_chapter: Option<usize>,
) {
    for meaning in &index.meanings {
        output.push_str("\\LitwebDeclareMiniIndexMeaning{");
        let _ = write!(output, "{}", meaning.marker);
        output.push_str("}{\\LitwebMiniIndexName{");
        push_latex_inline_code(output, &meaning.name);
        output.push_str("}: ");
        push_latex_text(output, meaning.kind.label());
        output.push_str(", ");
        render_location_link(output, &meaning.definition, current_chapter);
        output.push_str(".}\n");
    }
}

// Render LaTeX prose
fn render_prose(
    output: &mut String,
    block: &Block,
    requesting_chapter: Option<usize>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    mini_index: Option<&LocatedMiniIndex>,
    errors: &mut Vec<LatexError>,
) {
    let mut mini_markers = MiniMarkerCursor::new(mini_index);
    let mut line = 0;
    while let Some(element) = prose_block(&block.lines, line) {
        line = element.end();
        match element {
            ProseBlock::Paragraph { start, end } => {
                let mut source = InlineSource::new();
                for line in start..end {
                    source.push_line(&block.lines[line].text, &block.lines[line].origin);
                }
                output.push_str("\\par ");
                render_inline_elements(
                    output,
                    &inline_elements(&source),
                    requesting_chapter,
                    resolved,
                    index,
                    &mut mini_markers,
                    errors,
                );
                output.push_str("\n\n");
            }
            ProseBlock::List { list, .. } => {
                render_prose_list(
                    output,
                    block,
                    &list,
                    requesting_chapter,
                    LatexReferences { resolved, index },
                    &mut mini_markers,
                    errors,
                );
            }
            ProseBlock::Fence(fence) => render_fenced_prose(output, block, &fence),
            ProseBlock::Table(table) => {
                render_prose_table(
                    output,
                    block,
                    &table,
                    requesting_chapter,
                    LatexReferences { resolved, index },
                    &mut mini_markers,
                    errors,
                );
            }
            ProseBlock::DisplayMath { source, .. } => {
                output.push_str("\\[\n");
                output.push_str(&source);
                output.push_str("\n\\]\n");
            }
        }
    }
}

struct MiniMarkerCursor<'a> {
    index: Option<&'a LocatedMiniIndex>,
    next: HashMap<SourceOrigin, usize>,
    inline_column: HashMap<SourceOrigin, usize>,
}

impl<'a> MiniMarkerCursor<'a> {
    fn new(index: Option<&'a LocatedMiniIndex>) -> Self {
        Self {
            index,
            next: HashMap::new(),
            inline_column: HashMap::new(),
        }
    }

    fn take(
        &mut self,
        origin: &SourceOrigin,
        column: usize,
    ) -> Option<LocatedMiniOccurrence> {
        let occurrences = self.index?.occurrences.get(origin)?;
        let next = self.next.entry(origin.clone()).or_default();
        while occurrences
            .get(*next)
            .is_some_and(|occurrence| occurrence.column < column)
        {
            *next += 1;
        }
        let occurrence = occurrences
            .get(*next)
            .filter(|occurrence| occurrence.column == column)
            .copied()?;
        *next += 1;
        Some(occurrence)
    }

    fn take_inline(&mut self, origin: &SourceOrigin) -> Option<LocatedMiniOccurrence> {
        let column = {
            let column = self.inline_column.entry(origin.clone()).or_default();
            let current = *column;
            *column += 1;
            current
        };
        self.take(origin, column)
    }
}

fn push_latex_marked_inline_code(
    output: &mut String,
    text: &str,
    origin: &SourceOrigin,
    markers: &mut MiniMarkerCursor<'_>,
) {
    let mut start = 0;
    for offset in rust_identifier_offsets(text) {
        push_latex_inline_code(output, &text[start..offset]);
        if let Some(marker) = markers.take_inline(origin) {
            push_mini_marker(output, marker);
        }
        start = offset;
    }
    push_latex_inline_code(output, &text[start..]);
}

fn push_mini_marker(output: &mut String, occurrence: LocatedMiniOccurrence) {
    output.push_str(match occurrence.role {
        IdentifierRole::Definition => "\\LitwebMiniDefinition{",
        IdentifierRole::Use => "\\LitwebMiniUse{",
    });
    let _ = write!(output, "{}", occurrence.marker);
    output.push('}');
}

fn render_prose_list(
    output: &mut String,
    block: &Block,
    list: &ProseList,
    requesting_chapter: Option<usize>,
    references: LatexReferences<'_>,
    mini_markers: &mut MiniMarkerCursor<'_>,
    errors: &mut Vec<LatexError>,
) {
    let environment = if list.ordered_start.is_some() {
        "enumerate"
    } else {
        "itemize"
    };
    let _ = writeln!(output, "\\begin{{{environment}}}");
    if let Some(start) = list.ordered_start
        && start != 1
    {
        let _ = writeln!(output, "\\setcounter{{enumi}}{{{}}}", start - 1);
    }
    for item in &list.items {
        let mut source = InlineSource::new();
        for part in &item.parts {
            let line = &block.lines[part.line];
            source.push_line(&line.text[part.content_start..], &line.origin);
        }
        output.push_str("\\item ");
        render_inline_elements(
            output,
            &inline_elements(&source),
            requesting_chapter,
            references.resolved,
            references.index,
            mini_markers,
            errors,
        );
        output.push('\n');
    }
    let _ = writeln!(output, "\\end{{{environment}}}");
}

fn render_fenced_prose(output: &mut String, block: &Block, fence: &FencedProse) {
    output.push_str("\\begin{LitwebCode}\n");
    for line in fence.content_start..fence.content_end {
        let text = &block.lines[line].text;
        let indentation = text
            .bytes()
            .take(fence.indentation)
            .take_while(|byte| *byte == b' ')
            .count();
        push_latex_verbatim(output, &text[indentation..]);
        output.push('\n');
    }
    output.push_str("\\end{LitwebCode}\n");
}

fn render_prose_table(
    output: &mut String,
    block: &Block,
    table: &ProseTable,
    requesting_chapter: Option<usize>,
    references: LatexReferences<'_>,
    mini_markers: &mut MiniMarkerCursor<'_>,
    errors: &mut Vec<LatexError>,
) {
    output.push_str("\\begin{center}\n");
    let table_markers = take_table_markers(block, table, mini_markers);
    for marker in &table_markers {
        output.push_str("\\LitwebMiniBlockUse{");
        let _ = write!(output, "{}", marker.marker);
        output.push('}');
    }
    if !table_markers.is_empty() {
        output.push_str("\\nobreak\n");
    }
    output.push_str("\\begin{tabularx}{\\linewidth}{");
    let mut no_markers = MiniMarkerCursor::new(None);
    for alignment in &table.alignments {
        output.push(match alignment {
            TableAlignment::Left => 'L',
            TableAlignment::Center => 'C',
            TableAlignment::Right => 'R',
        });
    }
    output.push_str("}\n\\toprule\n");
    render_table_row(
        output,
        block,
        &table.header,
        requesting_chapter,
        references,
        &mut no_markers,
        errors,
    );
    output.push_str("\\midrule\n");
    for row in &table.body {
        render_table_row(
            output,
            block,
            row,
            requesting_chapter,
            references,
            &mut no_markers,
            errors,
        );
    }
    output.push_str("\\bottomrule\n\\end{tabularx}\n\\end{center}\n");
}

fn take_table_markers(
    block: &Block,
    table: &ProseTable,
    markers: &mut MiniMarkerCursor<'_>,
) -> Vec<LocatedMiniOccurrence> {
    let mut found = Vec::new();
    for row in std::iter::once(&table.header).chain(table.body.iter()) {
        for cell in row {
            let mut source = InlineSource::new();
            source.push_line(&cell.text, &block.lines[cell.line].origin);
            take_inline_markers(&inline_elements(&source), markers, &mut found);
        }
    }
    found.sort_by_key(|marker| marker.marker);
    found.dedup_by_key(|marker| marker.marker);
    found
}

fn take_inline_markers(
    elements: &[InlineElement],
    markers: &mut MiniMarkerCursor<'_>,
    found: &mut Vec<LocatedMiniOccurrence>,
) {
    for element in elements {
        match element {
            InlineElement::SourceCode { text, origin } => {
                for _ in rust_identifier_offsets(text) {
                    if let Some(marker) = markers.take_inline(origin) {
                        found.push(marker);
                    }
                }
            }
            InlineElement::Strong(contents)
            | InlineElement::Emphasis(contents)
            | InlineElement::Link {
                label: contents, ..
            } => take_inline_markers(contents, markers, found),
            InlineElement::Text(_)
            | InlineElement::Code(_)
            | InlineElement::Math(_)
            | InlineElement::BlockReference { .. } => {}
        }
    }
}

fn render_table_row(
    output: &mut String,
    block: &Block,
    cells: &[TableCell],
    requesting_chapter: Option<usize>,
    references: LatexReferences<'_>,
    mini_markers: &mut MiniMarkerCursor<'_>,
    errors: &mut Vec<LatexError>,
) {
    for (column, cell) in cells.iter().enumerate() {
        if column > 0 {
            output.push_str(" & ");
        }
        let mut source = InlineSource::new();
        source.push_line(&cell.text, &block.lines[cell.line].origin);
        render_inline_elements(
            output,
            &inline_elements(&source),
            requesting_chapter,
            references.resolved,
            references.index,
            mini_markers,
            errors,
        );
    }
    output.push_str(" \\\\\n");
}

// Render LaTeX inline elements
fn render_inline_elements(
    output: &mut String,
    elements: &[InlineElement],
    requesting_chapter: Option<usize>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    mini_markers: &mut MiniMarkerCursor<'_>,
    errors: &mut Vec<LatexError>,
) {
    for element in elements {
        match element {
            InlineElement::Text(text) => push_latex_text(output, text),
            InlineElement::Code(code) => {
                output.push_str("\\texttt{");
                push_latex_inline_code(output, code);
                output.push('}');
            }
            InlineElement::SourceCode { text, origin } => {
                output.push_str("\\texttt{");
                push_latex_marked_inline_code(output, text, origin, mini_markers);
                output.push('}');
            }
            InlineElement::Math(source) => {
                output.push_str("\\(");
                output.push_str(source);
                output.push_str("\\)");
            }
            InlineElement::Strong(contents) => {
                output.push_str("\\textbf{");
                render_inline_elements(
                    output,
                    contents,
                    requesting_chapter,
                    resolved,
                    index,
                    mini_markers,
                    errors,
                );
                output.push('}');
            }
            InlineElement::Emphasis(contents) => {
                output.push_str("\\emph{");
                render_inline_elements(
                    output,
                    contents,
                    requesting_chapter,
                    resolved,
                    index,
                    mini_markers,
                    errors,
                );
                output.push('}');
            }
            InlineElement::Link {
                label,
                target,
                active,
            } => {
                if *active {
                    output.push_str("\\href{\\detokenize{");
                    push_latex_url(output, target);
                    output.push_str("}}{");
                    render_inline_elements(
                        output,
                        label,
                        requesting_chapter,
                        resolved,
                        index,
                        mini_markers,
                        errors,
                    );
                    output.push('}');
                } else {
                    output.push('[');
                    render_inline_elements(
                        output,
                        label,
                        requesting_chapter,
                        resolved,
                        index,
                        mini_markers,
                        errors,
                    );
                    output.push_str("](");
                    push_latex_text(output, target);
                    output.push(')');
                }
            }
            InlineElement::BlockReference { name, origin } => {
                let definition = definition_location(
                    name,
                    origin,
                    requesting_chapter,
                    resolved,
                    index,
                    errors,
                );
                render_section_name(output, name, definition, false, requesting_chapter);
            }
        }
    }
}

// Render LaTeX named code
fn render_code_block(
    output: &mut String,
    block: &Block,
    location: &SectionLocation,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    mini_index: Option<&LocatedMiniIndex>,
    errors: &mut Vec<LatexError>,
) {
    let code = block.code().expect("the caller selected a code block");
    let identity = code_identity(block, location, resolved);
    let definition = definition_location(
        &code.name,
        &block.origin,
        Some(location.chapter),
        resolved,
        index,
        errors,
    );
    output.push_str("\\LitwebCodeTitle{");
    render_section_name(
        output,
        &code.name,
        definition,
        identity.is_some_and(|identity| index.roots.contains(&identity)),
        Some(location.chapter),
    );
    output.push(' ');
    output.push_str(if code.modifiers.contains(&Modifier::Additive) {
        "+\\(\\equiv\\)"
    } else if code.modifiers.contains(&Modifier::Redefinition) {
        ":="
    } else {
        "\\(\\equiv\\)"
    });
    output.push_str(if mini_index.is_some() {
        "}\n\\begin{LitwebMarkedCode}\n"
    } else {
        "}\n\\begin{LitwebCode}\n"
    });

    for line in &block.lines {
        if let Some(name) = block_reference(&line.text) {
            if mini_index.is_some() {
                output.push_str("\\LitwebCodeLine{}{}");
            }
            push_latex_verbatim(output, leading_whitespace(&line.text));
            let definition = definition_location(
                name,
                &line.origin,
                Some(location.chapter),
                resolved,
                index,
                errors,
            );
            render_code_section_name(output, name, definition, Some(location.chapter));
            output.push('\n');
        } else {
            if mini_index.is_some() {
                push_latex_marked_verbatim(output, &line.text, &line.origin, mini_index);
            } else {
                push_latex_verbatim(output, &line.text);
            }
            output.push('\n');
        }
    }
    output.push_str(if mini_index.is_some() {
        "\\end{LitwebMarkedCode}\n"
    } else {
        "\\end{LitwebCode}\n"
    });

    if let Some(locations) = identity.and_then(|identity| index.blocks.get(&identity)) {
        render_relationship(
            output,
            CodeRelationship::Addition,
            &locations.additions,
            location,
        );
        render_relationship(
            output,
            CodeRelationship::Replacement,
            &locations.redefinitions,
            location,
        );
        render_relationship(output, CodeRelationship::Use, &locations.uses, location);
    }
}

fn push_latex_marked_verbatim(
    output: &mut String,
    text: &str,
    origin: &SourceOrigin,
    mini_index: Option<&LocatedMiniIndex>,
) {
    output.push_str("\\LitwebCodeLine{");
    if let Some(occurrences) = mini_index.and_then(|index| index.occurrences.get(origin))
    {
        for occurrence in occurrences {
            if occurrence.role == IdentifierRole::Use {
                output.push_str("\\LitwebCodeUse{");
                let _ = write!(output, "{}", occurrence.marker);
                output.push('}');
            }
        }
    }
    output.push_str("}{");
    if let Some(occurrences) = mini_index.and_then(|index| index.occurrences.get(origin))
    {
        for occurrence in occurrences {
            if occurrence.role == IdentifierRole::Definition {
                output.push_str("\\LitwebCodeDefinition{");
                let _ = write!(output, "{}", occurrence.marker);
                output.push('}');
            }
        }
    }
    output.push('}');
    push_latex_verbatim(output, text);
}

fn code_identity(
    block: &Block,
    location: &SectionLocation,
    resolved: &ResolvedProgram,
) -> Option<usize> {
    let code = block.code().expect("the caller selected a code block");
    match resolved.lookup(location.chapter, &code.name) {
        BlockLookup::Found(identity) => Some(identity),
        BlockLookup::Missing | BlockLookup::Ambiguous(_) => None,
    }
}

fn definition_location<'a>(
    name: &str,
    origin: &SourceOrigin,
    requesting_chapter: Option<usize>,
    resolved: &ResolvedProgram,
    index: &'a WeaveIndex,
    errors: &mut Vec<LatexError>,
) -> Option<&'a SectionLocation> {
    let identity = match requesting_chapter {
        Some(chapter) => resolved.lookup(chapter, name),
        None => {
            let candidates = resolved
                .blocks
                .iter()
                .enumerate()
                .filter_map(|(identity, block)| (block.name == name).then_some(identity))
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [] => BlockLookup::Missing,
                [identity] => BlockLookup::Found(*identity),
                _ => BlockLookup::Ambiguous(candidates),
            }
        }
    };
    let identity = match identity {
        BlockLookup::Found(identity) => identity,
        BlockLookup::Missing => {
            errors.push(LatexError {
                origin: origin.clone(),
                kind: LatexErrorKind::UndefinedBlock {
                    name: name.to_owned(),
                },
            });
            return None;
        }
        BlockLookup::Ambiguous(candidates) => {
            errors.push(LatexError {
                origin: origin.clone(),
                kind: LatexErrorKind::AmbiguousBlock {
                    name: name.to_owned(),
                    candidates: candidates
                        .iter()
                        .map(|identity| resolved.block(*identity).origin.clone())
                        .collect(),
                },
            });
            return None;
        }
    };
    index.blocks.get(&identity).and_then(|locations| {
        (!locations.definition_hidden)
            .then_some(locations.definition.as_ref())
            .flatten()
    })
}

fn render_section_name(
    output: &mut String,
    name: &str,
    location: Option<&SectionLocation>,
    root: bool,
    current_chapter: Option<usize>,
) {
    output.push_str("\\(\\langle\\)");
    if root {
        output.push_str("\\textbf{");
    }
    output.push_str("\\texttt{");
    push_latex_inline_code(output, name);
    output.push('}');
    if root {
        output.push('}');
    }
    if let Some(location) = location {
        output.push_str("\\ ");
        render_location_link(output, location, current_chapter);
    }
    output.push_str("\\(\\rangle\\)");
}

fn render_code_section_name(
    output: &mut String,
    name: &str,
    location: Option<&SectionLocation>,
    current_chapter: Option<usize>,
) {
    output.push_str("\\(\\langle\\)\\texttt{");
    push_latex_verbatim(output, name);
    output.push('}');
    if let Some(location) = location {
        output.push_str("\\ \\LitwebCodeLocation{");
        let _ = write!(output, "{}", location.chapter + 1);
        output.push_str("}{");
        let _ = write!(output, "{}", location.section);
        output.push_str("}{");
        push_latex_verbatim(output, &location.label(current_chapter));
        output.push('}');
    }
    output.push_str("\\(\\rangle\\)");
}

#[derive(Clone, Copy)]
enum CodeRelationship {
    Addition,
    Replacement,
    Use,
}

impl CodeRelationship {
    fn sentence_start(self, plural: bool) -> &'static str {
        match (self, plural) {
            (Self::Addition, false) => "See also section ",
            (Self::Addition, true) => "See also sections ",
            (Self::Replacement, false) => "This code is replaced in section ",
            (Self::Replacement, true) => "This code is replaced in sections ",
            (Self::Use, false) => "This code is used in section ",
            (Self::Use, true) => "This code is used in sections ",
        }
    }
}

fn render_relationship(
    output: &mut String,
    relationship: CodeRelationship,
    locations: &[SectionLocation],
    current: &SectionLocation,
) {
    let locations = locations
        .iter()
        .filter(|location| *location != current)
        .collect::<Vec<_>>();
    if locations.is_empty() {
        return;
    }

    output.push_str("\\LitwebSeeAlso{");
    output.push_str(relationship.sentence_start(locations.len() > 1));
    for (number, location) in locations.iter().enumerate() {
        if number > 0 {
            if number + 1 == locations.len() {
                output.push_str(if locations.len() > 2 {
                    ", and "
                } else {
                    " and "
                });
            } else {
                output.push_str(", ");
            }
        }
        render_location_link(output, location, Some(current.chapter));
    }
    output.push_str(".}\n");
}

// Render the LaTeX identifier index
fn render_identifier_index(
    output: &mut String,
    index: &LocatedIdentifierIndex,
    current_chapter: Option<usize>,
    book: bool,
) {
    if index.entries.is_empty() {
        return;
    }

    if book {
        output.push_str(concat!(
            "\\chapter*{Identifier Index}\n",
            "\\addcontentsline{toc}{chapter}{Identifier Index}\n",
        ));
    } else {
        output.push_str("\\section*{Identifier Index}\n");
    }
    output.push_str("\\begin{description}\n");
    for entry in &index.entries {
        output.push_str("\\item[\\texttt{");
        push_latex_inline_code(output, &entry.name);
        output.push_str("}] ");
        for (position, occurrence) in entry.locations.iter().enumerate() {
            if position > 0 {
                output.push_str(", ");
            }
            if occurrence.role == IdentifierRole::Definition {
                output.push_str("\\LitwebIdentifierDefinition{");
            }
            render_location_link(output, &occurrence.location, current_chapter);
            if occurrence.role == IdentifierRole::Definition {
                output.push('}');
            }
        }
        output.push('\n');
    }
    output.push_str("\\end{description}\n");
}

// Escape LaTeX contents
fn push_latex_text(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '\\' => output.push_str("\\textbackslash{}"),
            '{' => output.push_str("\\{"),
            '}' => output.push_str("\\}"),
            '#' => output.push_str("\\#"),
            '$' => output.push_str("\\$"),
            '%' => output.push_str("\\%"),
            '&' => output.push_str("\\&"),
            '_' => output.push_str("\\_"),
            '^' => output.push_str("\\textasciicircum{}"),
            '~' => output.push_str("\\textasciitilde{}"),
            character => output.push(character),
        }
    }
}

fn push_latex_inline_code(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            ' ' | '\n' => output.push_str("\\ "),
            '\t' => output.push_str("\\hspace*{2em}"),
            character => push_latex_text(output, &character.to_string()),
        }
    }
}

fn push_latex_verbatim(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '\\' => output.push_str("\\LitwebBackslash{}"),
            '{' => output.push_str("\\LitwebLeftBrace{}"),
            '}' => output.push_str("\\LitwebRightBrace{}"),
            character => output.push(character),
        }
    }
}

fn push_latex_url(output: &mut String, target: &str) {
    for byte in target.bytes() {
        match byte {
            b'%' | b'\\' | b'{' | b'}' => {
                let _ = write!(output, "%{byte:02X}");
            }
            byte => output.push(char::from(byte)),
        }
    }
}

// LaTeX errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LatexPlanError {
    Resolution(ResolveErrors),
    Latex(LatexErrors),
}

impl fmt::Display for LatexPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolution(errors) => write!(formatter, "{errors}"),
            Self::Latex(errors) => write!(formatter, "{errors}"),
        }
    }
}

impl std::error::Error for LatexPlanError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexError {
    pub origin: SourceOrigin,
    pub kind: LatexErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LatexErrorKind {
    InvalidOutputName,
    UndefinedBlock {
        name: String,
    },
    AmbiguousBlock {
        name: String,
        candidates: Vec<SourceOrigin>,
    },
}

impl fmt::Display for LatexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: error: ", self.origin)?;
        match &self.kind {
            LatexErrorKind::InvalidOutputName => {
                formatter.write_str("the input path has no usable LaTeX output name")
            }
            LatexErrorKind::UndefinedBlock { name } => {
                write!(formatter, "code block {{{name}}} is not defined")
            }
            LatexErrorKind::AmbiguousBlock { name, candidates } => write!(
                formatter,
                "code block {{{name}}} is ambiguous; definitions are at {}",
                candidates
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl std::error::Error for LatexError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexErrors {
    errors: Vec<LatexError>,
}

impl LatexErrors {
    pub fn as_slice(&self) -> &[LatexError] {
        &self.errors
    }

    pub fn into_vec(self) -> Vec<LatexError> {
        self.errors
    }
}

impl fmt::Display for LatexErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.errors.first() {
            write!(formatter, "{first}")?;
            if self.errors.len() > 1 {
                write!(
                    formatter,
                    " (and {} more LaTeX errors)",
                    self.errors.len() - 1
                )?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for LatexErrors {}
