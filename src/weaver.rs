// src/weaver.rs
//! Deterministic HTML generation from parsed `.lit` programs.

// Weaver imports
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::identifier::{
    IdentifierRole, analyze_resolved as analyze_resolved_identifiers,
};
use crate::inline::TableAlignment;
use crate::output::PlannedOutput;
use crate::parser::{
    Block, BlockKind, BookMetadata, Chapter, Command, CommandKind, Modifier, Program,
    Section, SourceOrigin,
};
use crate::prose::{
    FencedProse, InlineElement, InlineSource, ProseBlock, ProseList, ProseTable,
    TableCell, inline_elements, prose_block,
};
use crate::resolver::{BlockLookup, ResolveErrors, ResolvedProgram, resolve};
use crate::util::{block_reference, leading_whitespace};
use crate::woven::{
    LocatedIdentifierIndex, LocatedIdentifierLocation, SectionLocation, WeaveIndex,
    WeaveLayout, WeaveSection, collect_locations, locate_identifiers,
};

// Weaver public results
// Weave options
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaveOptions {
    pub identifier_index: bool,
    pub color_scheme: Option<PathBuf>,
}

impl Default for WeaveOptions {
    fn default() -> Self {
        Self {
            identifier_index: true,
            color_scheme: None,
        }
    }
}

// Weave plan
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeavePlan {
    pub outputs: Vec<PlannedOutput>,
}

// Weave plan failures
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeavePlanError {
    Resolution(ResolveErrors),
    Weave(WeaveErrors),
}

impl fmt::Display for WeavePlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolution(errors) => write!(f, "{errors}"),
            Self::Weave(errors) => write!(f, "{errors}"),
        }
    }
}

impl std::error::Error for WeavePlanError {}

// Weave errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaveError {
    pub origin: SourceOrigin,
    pub kind: WeaveErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeaveErrorKind {
    InvalidOutputName,
    InvalidChapterLink {
        target: String,
    },
    ContentsPathCollision {
        path: PathBuf,
    },
    IdentifierIndexPathCollision {
        path: PathBuf,
    },
    MathAssetPathCollision {
        page: PathBuf,
        asset: PathBuf,
    },
    HighlightAssetPathCollision {
        page: PathBuf,
        asset: PathBuf,
    },
    UnknownColorScheme {
        name: String,
    },
    ReadColorScheme {
        path: PathBuf,
        kind: io::ErrorKind,
    },
    ColorSchemeInvalidUtf8 {
        path: PathBuf,
        valid_up_to: usize,
    },
    UndefinedBlock {
        name: String,
    },
    AmbiguousBlock {
        name: String,
        candidates: Vec<SourceOrigin>,
    },
}

impl fmt::Display for WeaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: error: ", self.origin)?;
        match &self.kind {
            WeaveErrorKind::InvalidOutputName => {
                f.write_str("the input path has no usable HTML output name")
            }
            WeaveErrorKind::ContentsPathCollision { path } => write!(
                f,
                "chapter output collides with the contents page: {}",
                path.display()
            ),
            WeaveErrorKind::IdentifierIndexPathCollision { path } => write!(
                f,
                "the identifier index output {} conflicts with another book page",
                path.display()
            ),
            WeaveErrorKind::MathAssetPathCollision { page, asset } => write!(
                f,
                "the book page {} conflicts with the math asset path {}",
                page.display(),
                asset.display()
            ),
            WeaveErrorKind::HighlightAssetPathCollision { page, asset } => write!(
                f,
                "the book page {} conflicts with the syntax-highlighting asset path {}",
                page.display(),
                asset.display()
            ),
            WeaveErrorKind::UnknownColorScheme { name } => write!(
                f,
                concat!(
                    "unknown color scheme {}; expected litweb, prism-default, dark, ",
                    "funky, okaidia, twilight, coy, solarized-light, tomorrow-night, ",
                    "none, or a .css file",
                ),
                name
            ),
            WeaveErrorKind::ReadColorScheme { path, kind } => {
                write!(f, "cannot read color scheme {}: {kind}", path.display())
            }
            WeaveErrorKind::ColorSchemeInvalidUtf8 { path, valid_up_to } => write!(
                f,
                "color scheme {} is not UTF-8 at byte {valid_up_to}",
                path.display()
            ),
            WeaveErrorKind::InvalidChapterLink { target } => write!(
                f,
                "chapter link {target} does not name a declared chapter in this book"
            ),
            WeaveErrorKind::UndefinedBlock { name } => {
                write!(f, "code block {{{name}}} is not defined")
            }
            WeaveErrorKind::AmbiguousBlock { name, candidates } => write!(
                f,
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

impl std::error::Error for WeaveError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaveErrors {
    errors: Vec<WeaveError>,
}

impl WeaveErrors {
    pub fn as_slice(&self) -> &[WeaveError] {
        &self.errors
    }

    pub fn into_vec(self) -> Vec<WeaveError> {
        self.errors
    }
}

impl fmt::Display for WeaveErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.errors.first() {
            write!(f, "{first}")?;
            if self.errors.len() > 1 {
                write!(f, " (and {} more weave errors)", self.errors.len() - 1)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for WeaveErrors {}

// Weaver planning
// Page render context
struct WeaveData<'a> {
    resolved: &'a ResolvedProgram,
    layout: &'a WeaveLayout,
    index: &'a WeaveIndex,
}

struct RenderContext<'a> {
    current_chapter: Option<usize>,
    current_path: &'a Path,
    single_page: bool,
    highlighting: bool,
    book_paths: Option<&'a BookHtmlPaths>,
}

// Choose page color schemes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltInColorScheme {
    Litweb,
    PrismDefault,
    Dark,
    Funky,
    Okaidia,
    Twilight,
    Coy,
    SolarizedLight,
    TomorrowNight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PageColorScheme {
    Disabled,
    BuiltIn(BuiltInColorScheme),
    Custom(String),
}

impl PageColorScheme {
    fn css(&self) -> Option<&str> {
        match self {
            Self::Disabled => None,
            Self::BuiltIn(scheme) => Some(scheme.css()),
            Self::Custom(css) => Some(css),
        }
    }
}

fn last_command(commands: &[Command], kind: CommandKind) -> Option<&Command> {
    commands.iter().rev().find(|command| command.kind == kind)
}

fn page_color_scheme(
    program: &Program,
    chapter: Option<&Chapter>,
    command_line: Option<&Path>,
    errors: &mut Vec<WeaveError>,
) -> PageColorScheme {
    let result = if let Some(value) = command_line {
        resolve_color_scheme(value, None, &program.origin)
    } else {
        let command = chapter
            .and_then(|chapter| last_command(&chapter.commands, CommandKind::ColorScheme))
            .or_else(|| last_command(&program.commands, CommandKind::ColorScheme));
        match command {
            Some(command) if command.arguments.is_empty() => {
                Ok(PageColorScheme::Disabled)
            }
            Some(command) => {
                let source = Path::new(&command.origin.file);
                resolve_color_scheme(
                    Path::new(&command.arguments),
                    source.parent(),
                    &command.origin,
                )
            }
            None => Ok(PageColorScheme::BuiltIn(BuiltInColorScheme::Litweb)),
        }
    };

    match result {
        Ok(scheme) => scheme,
        Err(error) => {
            errors.push(error);
            PageColorScheme::Disabled
        }
    }
}

fn resolve_color_scheme(
    value: &Path,
    source_directory: Option<&Path>,
    origin: &SourceOrigin,
) -> Result<PageColorScheme, WeaveError> {
    if value
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case("none"))
    {
        return Ok(PageColorScheme::Disabled);
    }
    if value
        .extension()
        .is_some_and(|extension| extension == "css")
    {
        let path = if value.is_absolute() {
            value.to_owned()
        } else {
            source_directory
                .unwrap_or_else(|| Path::new(""))
                .join(value)
        };
        let bytes = fs::read(&path).map_err(|error| WeaveError {
            origin: origin.clone(),
            kind: WeaveErrorKind::ReadColorScheme {
                path: path.clone(),
                kind: error.kind(),
            },
        })?;
        let css = String::from_utf8(bytes).map_err(|error| WeaveError {
            origin: origin.clone(),
            kind: WeaveErrorKind::ColorSchemeInvalidUtf8 {
                path,
                valid_up_to: error.utf8_error().valid_up_to(),
            },
        })?;
        return Ok(PageColorScheme::Custom(css));
    }

    let Some(name) = value.to_str() else {
        return Err(WeaveError {
            origin: origin.clone(),
            kind: WeaveErrorKind::UnknownColorScheme {
                name: value.to_string_lossy().into_owned(),
            },
        });
    };
    let scheme = match name.to_ascii_lowercase().as_str() {
        "litweb" => BuiltInColorScheme::Litweb,
        "prism-default" => BuiltInColorScheme::PrismDefault,
        "dark" => BuiltInColorScheme::Dark,
        "funky" => BuiltInColorScheme::Funky,
        "okaidia" => BuiltInColorScheme::Okaidia,
        "twilight" => BuiltInColorScheme::Twilight,
        "coy" => BuiltInColorScheme::Coy,
        "solarized-light" => BuiltInColorScheme::SolarizedLight,
        "tomorrow-night" => BuiltInColorScheme::TomorrowNight,
        _ => {
            return Err(WeaveError {
                origin: origin.clone(),
                kind: WeaveErrorKind::UnknownColorScheme {
                    name: name.to_owned(),
                },
            });
        }
    };
    Ok(PageColorScheme::BuiltIn(scheme))
}

// Plan woven output
pub fn plan_weave(program: &Program) -> Result<WeavePlan, WeavePlanError> {
    plan_weave_with_options(program, WeaveOptions::default())
}

pub fn plan_weave_with_options(
    program: &Program,
    options: WeaveOptions,
) -> Result<WeavePlan, WeavePlanError> {
    let resolved = resolve(program).map_err(WeavePlanError::Resolution)?;
    let layout = WeaveLayout::new(program);
    let index = collect_locations(program, &resolved, &layout);
    let data = WeaveData {
        resolved: &resolved,
        layout: &layout,
        index: &index,
    };
    let mut errors = Vec::new();
    let identifiers = if options.identifier_index {
        locate_identifiers(&analyze_resolved_identifiers(program, &resolved), &layout)
    } else {
        LocatedIdentifierIndex::default()
    };
    let mut outputs = if let Some(book) = program.book() {
        plan_book_weave(
            program,
            book,
            &data,
            &identifiers,
            options.color_scheme.as_deref(),
            &mut errors,
        )
    } else {
        let output_name = html_output_name(program).map_err(|error| {
            WeavePlanError::Weave(WeaveErrors {
                errors: vec![error],
            })
        })?;
        let color_scheme = page_color_scheme(
            program,
            None,
            options.color_scheme.as_deref(),
            &mut errors,
        );
        let html = render_document(
            program,
            &data,
            &identifiers,
            &output_name,
            &color_scheme,
            &mut errors,
        );
        vec![PlannedOutput {
            relative_path: output_name,
            bytes: html.into_bytes(),
            origin: program.origin.clone(),
        }]
    };
    let uses_math = planned_pages_contain_math(&outputs);
    let uses_highlighting = planned_pages_contain_highlighting(&outputs);
    if uses_math {
        outputs.extend(plan_math_assets(program, &outputs, &mut errors));
    }
    if uses_highlighting {
        outputs.extend(plan_highlight_assets(program, &outputs, &mut errors));
    }

    if !errors.is_empty() {
        return Err(WeavePlanError::Weave(WeaveErrors { errors }));
    }

    Ok(WeavePlan { outputs })
}

// Plan book pages
fn plan_book_weave(
    program: &Program,
    book: &BookMetadata,
    data: &WeaveData<'_>,
    identifiers: &LocatedIdentifierIndex,
    command_line_scheme: Option<&Path>,
    errors: &mut Vec<WeaveError>,
) -> Vec<PlannedOutput> {
    let Some(paths) = book_html_paths(program, errors) else {
        return Vec::new();
    };
    let identifier_path =
        book_identifier_index_path(program, &paths, identifiers, errors);
    let contents_color_scheme =
        page_color_scheme(program, None, command_line_scheme, errors);
    let mut outputs = Vec::with_capacity(program.chapters.len() + 2);
    outputs.push(PlannedOutput {
        relative_path: paths.contents.clone(),
        bytes: render_contents_document(
            program,
            book,
            &paths,
            data,
            identifier_path.as_deref(),
            &contents_color_scheme,
            errors,
        )
        .into_bytes(),
        origin: program.origin.clone(),
    });
    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        let color_scheme =
            page_color_scheme(program, Some(chapter), command_line_scheme, errors);
        outputs.push(PlannedOutput {
            relative_path: paths.chapters[chapter_index].clone(),
            bytes: render_chapter_document(
                program,
                chapter_index,
                data,
                &paths,
                identifier_path.as_deref(),
                &color_scheme,
                errors,
            )
            .into_bytes(),
            origin: chapter.origin.clone(),
        });
    }
    if let Some(path) = identifier_path {
        outputs.push(PlannedOutput {
            relative_path: path.clone(),
            bytes: render_book_identifier_index(program, &paths, identifiers, &path)
                .into_bytes(),
            origin: program.origin.clone(),
        });
    }
    outputs
}

struct BookHtmlPaths {
    contents: PathBuf,
    chapters: Vec<PathBuf>,
}

fn book_html_paths(
    program: &Program,
    errors: &mut Vec<WeaveError>,
) -> Option<BookHtmlPaths> {
    let contents = match html_output_name(program) {
        Ok(path) => path,
        Err(error) => {
            errors.push(error);
            return None;
        }
    };
    let chapters = program
        .chapters
        .iter()
        .map(|chapter| {
            chapter
                .book
                .as_ref()
                .expect("a loaded book chapter has book metadata")
                .source_path
                .with_extension("html")
        })
        .collect::<Vec<_>>();

    for (chapter, path) in program.chapters.iter().zip(&chapters) {
        if *path == contents {
            let metadata = chapter
                .book
                .as_ref()
                .expect("a loaded book chapter has book metadata");
            errors.push(WeaveError {
                origin: metadata.label_origin.clone(),
                kind: WeaveErrorKind::ContentsPathCollision { path: path.clone() },
            });
        }
    }

    Some(BookHtmlPaths { contents, chapters })
}

// Plan math assets
fn planned_pages_contain_math(outputs: &[PlannedOutput]) -> bool {
    outputs.iter().any(|output| {
        output
            .bytes
            .windows(INLINE_MATH_MARKER.len())
            .any(|window| window == INLINE_MATH_MARKER)
            || output
                .bytes
                .windows(DISPLAY_MATH_MARKER.len())
                .any(|window| window == DISPLAY_MATH_MARKER)
    })
}

fn plan_math_assets(
    program: &Program,
    pages: &[PlannedOutput],
    errors: &mut Vec<WeaveError>,
) -> Vec<PlannedOutput> {
    for page in pages {
        for asset in MATH_ASSETS {
            let asset_path = math_asset_path(asset.path);
            if paths_conflict_as_files(&page.relative_path, &asset_path) {
                errors.push(WeaveError {
                    origin: page.origin.clone(),
                    kind: WeaveErrorKind::MathAssetPathCollision {
                        page: page.relative_path.clone(),
                        asset: asset_path,
                    },
                });
                return Vec::new();
            }
        }
    }

    MATH_ASSETS
        .iter()
        .map(|asset| PlannedOutput {
            relative_path: math_asset_path(asset.path),
            bytes: asset.bytes.to_vec(),
            origin: program.origin.clone(),
        })
        .collect()
}

fn paths_conflict_as_files(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

// Plan highlighting assets
fn planned_pages_contain_highlighting(outputs: &[PlannedOutput]) -> bool {
    outputs.iter().any(|output| {
        output
            .bytes
            .windows(HIGHLIGHT_PAGE_MARKER.len())
            .any(|window| window == HIGHLIGHT_PAGE_MARKER)
    })
}

fn plan_highlight_assets(
    program: &Program,
    pages: &[PlannedOutput],
    errors: &mut Vec<WeaveError>,
) -> Vec<PlannedOutput> {
    for page in pages {
        for asset in HIGHLIGHT_ASSETS {
            let asset_path = highlight_asset_path(asset.path);
            if paths_conflict_as_files(&page.relative_path, &asset_path) {
                errors.push(WeaveError {
                    origin: page.origin.clone(),
                    kind: WeaveErrorKind::HighlightAssetPathCollision {
                        page: page.relative_path.clone(),
                        asset: asset_path,
                    },
                });
                return Vec::new();
            }
        }
    }

    HIGHLIGHT_ASSETS
        .iter()
        .map(|asset| PlannedOutput {
            relative_path: highlight_asset_path(asset.path),
            bytes: asset.bytes.to_vec(),
            origin: program.origin.clone(),
        })
        .collect()
}

// Choose the book identifier-index path
fn book_identifier_index_path(
    program: &Program,
    paths: &BookHtmlPaths,
    identifiers: &LocatedIdentifierIndex,
    errors: &mut Vec<WeaveError>,
) -> Option<PathBuf> {
    if identifiers.entries.is_empty() {
        return None;
    }

    let path = paths.contents.with_file_name("index-identifiers.html");
    let collision = if path == paths.contents {
        Some(program.origin.clone())
    } else {
        program.chapters.iter().zip(&paths.chapters).find_map(
            |(chapter, chapter_path)| {
                let metadata = chapter
                    .book
                    .as_ref()
                    .expect("a loaded book chapter has book metadata");
                (*chapter_path == path).then(|| metadata.label_origin.clone())
            },
        )
    };
    if let Some(origin) = collision {
        errors.push(WeaveError {
            origin,
            kind: WeaveErrorKind::IdentifierIndexPathCollision { path },
        });
        None
    } else {
        Some(path)
    }
}

// Choose a single-page output name

fn html_output_name(program: &Program) -> Result<PathBuf, WeaveError> {
    let path = Path::new(&program.file);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| WeaveError {
            origin: program.origin.clone(),
            kind: WeaveErrorKind::InvalidOutputName,
        })?;
    Ok(PathBuf::from(format!("{stem}.html")))
}

// Weaver document rendering
// Render one-file HTML
fn render_document(
    program: &Program,
    data: &WeaveData<'_>,
    identifiers: &LocatedIdentifierIndex,
    output_path: &Path,
    color_scheme: &PageColorScheme,
    errors: &mut Vec<WeaveError>,
) -> String {
    let mut body = String::new();
    body.push_str("<h1>");
    push_escaped_text(&mut body, &program.title);
    body.push_str("</h1>\n");

    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        let context = RenderContext {
            current_chapter: Some(chapter_index),
            current_path: output_path,
            single_page: true,
            highlighting: color_scheme.css().is_some(),
            book_paths: None,
        };
        render_chapter_sections(
            &mut body,
            chapter,
            &data.layout.chapters[chapter_index],
            &context,
            data.resolved,
            data.index,
            errors,
        );
    }

    render_identifier_index(&mut body, identifiers);
    let uses_math = page_contains_math(&body);
    let highlight_scheme = page_contains_highlighting(&body).then_some(color_scheme);
    let mut output = String::new();
    output.push_str("<!doctype html>\n");
    render_html_element_open(&mut output, uses_math, highlight_scheme.is_some());
    output.push_str("<head>\n");
    output.push_str("<meta charset=\"utf-8\">\n");
    output.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n",
    );
    output.push_str("<title>");
    push_escaped_text(&mut output, &program.title);
    output.push_str("</title>\n");
    if uses_math {
        render_math_stylesheet_link(&mut output, output_path);
    }
    output.push_str("<style>\n");
    output.push_str(DEFAULT_STYLE);
    if uses_math {
        output.push_str(MATH_STYLE);
    }
    if !identifiers.entries.is_empty() {
        output.push_str(IDENTIFIER_STYLE);
    }
    if let Some(scheme) = highlight_scheme {
        render_highlight_theme_css(&mut output, scheme);
    }
    output.push_str("</style>\n");
    if uses_math {
        render_math_scripts(&mut output, output_path);
    }
    if highlight_scheme.is_some() {
        render_highlight_scripts(&mut output, output_path);
    }
    output.push_str("</head>\n<body>\n<main>\n");
    output.push_str(&body);
    output.push_str("</main>\n</body>\n</html>\n");
    output
}

// Render the identifier index
fn render_identifier_index(output: &mut String, index: &LocatedIdentifierIndex) {
    if index.entries.is_empty() {
        return;
    }

    output.push_str(concat!(
        "<section class=\"identifier-index\" ",
        "aria-labelledby=\"identifier-index-title\">\n",
        "<h2 id=\"identifier-index-title\">Identifier Index</h2>\n<dl>\n",
    ));
    for entry in &index.entries {
        output.push_str("<dt><code>");
        push_escaped_text(output, &entry.name);
        output.push_str("</code></dt>\n<dd>");
        for (position, occurrence) in entry.locations.iter().enumerate() {
            if position > 0 {
                output.push_str(", ");
            }
            let label = occurrence.location.section.to_string();
            output.push_str("<a");
            if occurrence.role == IdentifierRole::Definition {
                output.push_str(" class=\"identifier-definition\"");
            }
            output.push_str(" href=\"#");
            push_escaped_attribute(output, &occurrence.location.anchor());
            output.push_str("\" aria-label=\"");
            if occurrence.role == IdentifierRole::Definition {
                output.push_str("definition in section ");
            } else {
                output.push_str("section ");
            }
            push_escaped_attribute(output, &label);
            output.push_str("\">");
            push_escaped_text(output, &label);
            output.push_str("</a>");
        }
        output.push_str("</dd>\n");
    }
    output.push_str("</dl>\n</section>\n");
}

// Render a book contents page
fn render_contents_document(
    program: &Program,
    book: &BookMetadata,
    paths: &BookHtmlPaths,
    data: &WeaveData<'_>,
    identifier_path: Option<&Path>,
    color_scheme: &PageColorScheme,
    errors: &mut Vec<WeaveError>,
) -> String {
    let page_style = if identifier_path.is_some() {
        BookPageStyle::IdentifierNavigation
    } else {
        BookPageStyle::Ordinary
    };
    let mut body = String::new();
    body.push_str("<h1>");
    push_escaped_text(&mut body, &program.title);
    body.push_str("</h1>\n");

    let introduction = Block {
        origin: program.origin.clone(),
        kind: BlockKind::Prose,
        lines: book.introduction_lines.clone(),
    };
    let context = RenderContext {
        current_chapter: None,
        current_path: &paths.contents,
        single_page: false,
        highlighting: color_scheme.css().is_some(),
        book_paths: Some(paths),
    };
    render_prose(
        &mut body,
        &introduction,
        &context,
        data.resolved,
        data.index,
        errors,
    );
    render_contents_list(&mut body, program, paths);
    if let Some(identifier_path) = identifier_path {
        render_contents_index_link(&mut body, &paths.contents, identifier_path);
    }
    let uses_math = page_contains_math(&body);
    let highlight_scheme = page_contains_highlighting(&body).then_some(color_scheme);
    let mut output = String::new();
    render_book_head(
        &mut output,
        &program.title,
        page_style,
        &paths.contents,
        uses_math,
        highlight_scheme,
    );
    output.push_str(&body);
    output.push_str("</main>\n</body>\n</html>\n");
    output
}

// Render a book chapter page

fn render_chapter_document(
    program: &Program,
    chapter_index: usize,
    data: &WeaveData<'_>,
    paths: &BookHtmlPaths,
    identifier_path: Option<&Path>,
    color_scheme: &PageColorScheme,
    errors: &mut Vec<WeaveError>,
) -> String {
    let chapter = &program.chapters[chapter_index];
    let document_title = format!("{} — {}", chapter.title, program.title);
    let page_style = if identifier_path.is_some() {
        BookPageStyle::IdentifierNavigation
    } else {
        BookPageStyle::Ordinary
    };
    let mut body = String::new();
    body.push_str("<h1><span class=\"chapter-number\">");
    push_escaped_text(&mut body, &chapter.number());
    body.push_str(".</span> ");
    push_escaped_text(&mut body, &chapter.title);
    body.push_str("</h1>\n");

    render_book_navigation(&mut body, program, paths, chapter_index, identifier_path);
    let context = RenderContext {
        current_chapter: Some(chapter_index),
        current_path: &paths.chapters[chapter_index],
        single_page: false,
        highlighting: color_scheme.css().is_some(),
        book_paths: Some(paths),
    };
    render_chapter_sections(
        &mut body,
        chapter,
        &data.layout.chapters[chapter_index],
        &context,
        data.resolved,
        data.index,
        errors,
    );
    render_book_navigation(&mut body, program, paths, chapter_index, identifier_path);
    let uses_math = page_contains_math(&body);
    let highlight_scheme = page_contains_highlighting(&body).then_some(color_scheme);
    let mut output = String::new();
    render_book_head(
        &mut output,
        &document_title,
        page_style,
        &paths.chapters[chapter_index],
        uses_math,
        highlight_scheme,
    );
    output.push_str(&body);
    output.push_str("</main>\n</body>\n</html>\n");
    output
}

// Render a book identifier index
fn render_book_identifier_index(
    program: &Program,
    paths: &BookHtmlPaths,
    index: &LocatedIdentifierIndex,
    current_path: &Path,
) -> String {
    let mut output = String::new();
    let document_title = format!("Identifier Index — {}", program.title);
    render_book_head(
        &mut output,
        &document_title,
        BookPageStyle::IdentifierIndex,
        current_path,
        false,
        None,
    );
    output.push_str("<h1 id=\"identifier-index-title\">Identifier Index</h1>\n");
    output.push_str("<p class=\"identifier-index-book-title\">");
    push_escaped_text(&mut output, &program.title);
    output.push_str("</p>\n");
    render_identifier_index_navigation(&mut output, paths, current_path);
    output.push_str(
        "<section class=\"identifier-index book-identifier-index\" \
         aria-labelledby=\"identifier-entries-title\">\n\
         <h2 id=\"identifier-entries-title\" class=\"visually-hidden\">\
         Identifier entries</h2>\n<dl>\n",
    );

    for entry in &index.entries {
        output.push_str("<dt><code>");
        push_escaped_text(&mut output, &entry.name);
        output.push_str("</code></dt>\n<dd>\n");
        for (chapter_index, chapter) in program.chapters.iter().enumerate() {
            let locations = entry
                .locations
                .iter()
                .filter(|occurrence| occurrence.location.chapter == chapter_index)
                .collect::<Vec<_>>();
            if locations.is_empty() {
                continue;
            }
            render_identifier_chapter_group(
                &mut output,
                chapter,
                &paths.chapters[chapter_index],
                &locations,
                current_path,
            );
        }
        output.push_str("</dd>\n");
    }

    output.push_str("</dl>\n</section>\n");
    render_identifier_index_navigation(&mut output, paths, current_path);
    output.push_str("</main>\n</body>\n</html>\n");
    output
}

fn render_identifier_chapter_group(
    output: &mut String,
    chapter: &Chapter,
    chapter_path: &Path,
    locations: &[&LocatedIdentifierLocation],
    current_path: &Path,
) {
    let metadata = chapter
        .book
        .as_ref()
        .expect("a loaded book chapter has book metadata");
    output.push_str(
        "<div class=\"identifier-chapter-group\"><a class=\"identifier-chapter\" href=\"",
    );
    push_escaped_attribute(output, &relative_url(current_path, chapter_path));
    output.push_str("\">");
    push_escaped_text(output, &chapter.number());
    output.push_str(". ");
    push_escaped_text(output, &metadata.navigation_label);
    output.push_str("</a>: ");

    for (position, occurrence) in locations.iter().enumerate() {
        if position > 0 {
            output.push_str(", ");
        }
        let location = &occurrence.location;
        output.push_str("<a");
        if occurrence.role == IdentifierRole::Definition {
            output.push_str(" class=\"identifier-definition\"");
        }
        output.push_str(" href=\"");
        push_escaped_attribute(output, &relative_url(current_path, chapter_path));
        output.push('#');
        push_escaped_attribute(output, &location.anchor());
        output.push_str("\" aria-label=\"");
        if occurrence.role == IdentifierRole::Definition {
            output.push_str("definition in ");
        }
        output.push_str("chapter ");
        push_escaped_attribute(output, &chapter.number());
        output.push_str(", section ");
        let section = location.section.to_string();
        push_escaped_attribute(output, &section);
        output.push_str("\">");
        push_escaped_text(output, &section);
        output.push_str("</a>");
    }
    output.push_str("</div>\n");
}

fn render_identifier_index_navigation(
    output: &mut String,
    paths: &BookHtmlPaths,
    current_path: &Path,
) {
    output.push_str("<nav class=\"book-navigation\" aria-label=\"Book navigation\">\n");
    render_contents_navigation_link(output, current_path, &paths.contents);
    output.push_str("</nav>\n");
}

// Open a book HTML document

enum BookPageStyle {
    Ordinary,
    IdentifierNavigation,
    IdentifierIndex,
}

fn render_book_head(
    output: &mut String,
    title: &str,
    page_style: BookPageStyle,
    current_path: &Path,
    uses_math: bool,
    highlight_scheme: Option<&PageColorScheme>,
) {
    output.push_str("<!doctype html>\n");
    render_html_element_open(output, uses_math, highlight_scheme.is_some());
    output.push_str("<head>\n");
    output.push_str("<meta charset=\"utf-8\">\n");
    output.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n",
    );
    output.push_str("<title>");
    push_escaped_text(output, title);
    output.push_str("</title>\n");
    if uses_math {
        render_math_stylesheet_link(output, current_path);
    }
    output.push_str("<style>\n");
    output.push_str(DEFAULT_STYLE);
    output.push_str(BOOK_STYLE);
    if uses_math {
        output.push_str(MATH_STYLE);
    }
    if matches!(
        page_style,
        BookPageStyle::IdentifierNavigation | BookPageStyle::IdentifierIndex
    ) {
        output.push_str(BOOK_IDENTIFIER_STYLE);
    }
    if matches!(page_style, BookPageStyle::IdentifierIndex) {
        output.push_str(IDENTIFIER_STYLE);
    }
    if let Some(scheme) = highlight_scheme {
        render_highlight_theme_css(output, scheme);
    }
    output.push_str("</style>\n");
    if uses_math {
        render_math_scripts(output, current_path);
    }
    if highlight_scheme.is_some() {
        render_highlight_scripts(output, current_path);
    }
    output.push_str("</head>\n<body>\n<main>\n");
}

// Render presentation sections
fn render_chapter_sections(
    output: &mut String,
    chapter: &Chapter,
    sections: &[WeaveSection],
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    for section in sections {
        let source_section = &chapter.sections[section.source_section];
        let _ = writeln!(
            output,
            "<section class=\"section\" id=\"{}\">",
            section.location.anchor()
        );
        let prose = section.prose.map(|block| &source_section.blocks[block]);
        let code = section.code.map(|block| &source_section.blocks[block]);

        if let Some(prose) = prose.filter(|block| prose_starts_with_paragraph(block)) {
            output.push_str("<div class=\"section-opening\">\n");
            render_section_heading(output, section, source_section);
            let next_line = render_first_prose_paragraph(
                output, prose, context, resolved, index, errors,
            );
            output.push_str("</div>\n");
            render_prose_from(output, prose, next_line, context, resolved, index, errors);
            if let Some(code) = code {
                render_code_block(
                    output,
                    code,
                    &section.location,
                    context,
                    resolved,
                    index,
                    errors,
                );
            }
        } else if prose.is_none()
            && let Some(code) = code
        {
            output.push_str(concat!(
                "<div class=\"codeblock codeblock-first\">\n",
                "<div class=\"section-opening\">\n",
            ));
            render_section_heading(output, section, source_section);
            render_code_block_name(
                output,
                code,
                &section.location,
                context,
                resolved,
                index,
                errors,
            );
            output.push_str("</div>\n");
            render_code_block_contents(
                output,
                code,
                &section.location,
                context,
                resolved,
                index,
                errors,
            );
            output.push_str("</div>\n");
        } else {
            render_section_heading(output, section, source_section);
            if let Some(prose) = prose {
                render_prose(output, prose, context, resolved, index, errors);
            }
            if let Some(code) = code {
                render_code_block(
                    output,
                    code,
                    &section.location,
                    context,
                    resolved,
                    index,
                    errors,
                );
            }
        }
        output.push_str("</section>\n");
    }
}

// Render a presentation-section heading
fn render_section_heading(output: &mut String, section: &WeaveSection, source: &Section) {
    if section.source_section_start && !source.title.is_empty() {
        let _ = write!(
            output,
            concat!(
                "<h2 class=\"section-heading\"><span class=\"section-number\">",
                "{}.</span> <span class=\"section-title\">",
            ),
            section.location.section
        );
        push_escaped_text(output, &source.title);
        output.push_str("</span>");
        if !matches!(source.title.chars().last(), Some('.' | '?' | '!')) {
            output.push('.');
        }
        output.push_str("</h2>\n");
    } else {
        let _ = writeln!(
            output,
            concat!(
                "<h2 class=\"section-heading section-heading-untitled\">",
                "<span class=\"section-number\">{}.</span> ",
                "<span class=\"visually-hidden\">Untitled section</span></h2>",
            ),
            section.location.section
        );
    }
}

// Render the book contents list
fn render_contents_list(output: &mut String, program: &Program, paths: &BookHtmlPaths) {
    output.push_str("<nav class=\"book-contents\" aria-label=\"Contents\">\n<ol>\n");
    let mut chapter = 0;
    while chapter < program.chapters.len() {
        let major = &program.chapters[chapter];
        render_contents_link(output, major, &paths.contents, &paths.chapters[chapter]);
        chapter += 1;
        if chapter < program.chapters.len() && program.chapters[chapter].minor_number != 0
        {
            output.push_str("\n<ol>\n");
            while chapter < program.chapters.len()
                && program.chapters[chapter].minor_number != 0
            {
                render_contents_link(
                    output,
                    &program.chapters[chapter],
                    &paths.contents,
                    &paths.chapters[chapter],
                );
                output.push_str("</li>\n");
                chapter += 1;
            }
            output.push_str("</ol>\n");
        }
        output.push_str("</li>\n");
    }
    output.push_str("</ol>\n</nav>\n");
}

fn render_contents_link(
    output: &mut String,
    chapter: &Chapter,
    contents_path: &Path,
    chapter_path: &Path,
) {
    let metadata = chapter
        .book
        .as_ref()
        .expect("a loaded book chapter has book metadata");
    output.push_str("<li><a href=\"");
    push_escaped_attribute(output, &relative_url(contents_path, chapter_path));
    output.push_str("\">");
    push_escaped_text(output, &chapter.number());
    output.push_str(". ");
    push_escaped_text(output, &metadata.navigation_label);
    output.push_str("</a>");
}

// Render the contents identifier-index link
fn render_contents_index_link(
    output: &mut String,
    contents_path: &Path,
    index_path: &Path,
) {
    output.push_str(
        "<nav class=\"book-index-navigation\" aria-label=\"Book indexes\">\n<a href=\"",
    );
    push_escaped_attribute(output, &relative_url(contents_path, index_path));
    output.push_str("\">Identifier Index</a>\n</nav>\n");
}

// Render book navigation

fn render_book_navigation(
    output: &mut String,
    program: &Program,
    paths: &BookHtmlPaths,
    chapter_index: usize,
    identifier_path: Option<&Path>,
) {
    let current = &paths.chapters[chapter_index];
    output.push_str("<nav class=\"book-navigation\" aria-label=\"Book navigation\">\n");
    if let Some(previous) = chapter_index.checked_sub(1) {
        render_navigation_link(
            output,
            current,
            &program.chapters[previous],
            &paths.chapters[previous],
            "prev",
            "Previous: ",
        );
    }
    if let Some(identifier_path) = identifier_path {
        output.push_str("<span class=\"book-navigation-indexes\">\n");
        render_contents_navigation_link(output, current, &paths.contents);
        output.push_str("<a class=\"identifier-index-link\" href=\"");
        push_escaped_attribute(output, &relative_url(current, identifier_path));
        output.push_str("\">Identifier Index</a>\n</span>\n");
    } else {
        render_contents_navigation_link(output, current, &paths.contents);
    }
    if let Some(next) = program.chapters.get(chapter_index + 1) {
        render_navigation_link(
            output,
            current,
            next,
            &paths.chapters[chapter_index + 1],
            "next",
            "Next: ",
        );
    }
    output.push_str("</nav>\n");
}

fn render_contents_navigation_link(
    output: &mut String,
    current: &Path,
    contents_path: &Path,
) {
    output.push_str("<a class=\"contents\" href=\"");
    push_escaped_attribute(output, &relative_url(current, contents_path));
    output.push_str("\">Contents</a>\n");
}

fn render_navigation_link(
    output: &mut String,
    current: &Path,
    chapter: &Chapter,
    chapter_path: &Path,
    relation: &str,
    prefix: &str,
) {
    let metadata = chapter
        .book
        .as_ref()
        .expect("a loaded book chapter has book metadata");
    output.push_str("<a rel=\"");
    push_escaped_attribute(output, relation);
    output.push_str("\" href=\"");
    push_escaped_attribute(output, &relative_url(current, chapter_path));
    output.push_str("\">");
    push_escaped_text(output, prefix);
    push_escaped_text(output, &metadata.navigation_label);
    output.push_str("</a>\n");
}

// Weaver prose code and relationships
// Render a prose block
fn render_prose(
    output: &mut String,
    block: &Block,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    render_prose_from(output, block, 0, context, resolved, index, errors);
}

// Render a section-opening paragraph
fn prose_starts_with_paragraph(block: &Block) -> bool {
    matches!(
        prose_block(&block.lines, 0),
        Some(ProseBlock::Paragraph { .. })
    )
}

fn render_first_prose_paragraph(
    output: &mut String,
    block: &Block,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) -> usize {
    let Some(ProseBlock::Paragraph { start, end }) = prose_block(&block.lines, 0) else {
        unreachable!("the layout selected an opening paragraph");
    };
    render_paragraph(output, block, start..end, context, resolved, index, errors);
    end
}

// Render prose contents

fn render_prose_from(
    output: &mut String,
    block: &Block,
    mut line: usize,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    while let Some(element) = prose_block(&block.lines, line) {
        line = element.end();
        match element {
            ProseBlock::Paragraph { start, end } => {
                render_paragraph(
                    output,
                    block,
                    start..end,
                    context,
                    resolved,
                    index,
                    errors,
                );
            }
            ProseBlock::List { list, .. } => {
                render_prose_list(output, block, &list, context, resolved, index, errors);
            }
            ProseBlock::Fence(fence) => {
                render_fenced_prose(output, block, &fence, context);
            }
            ProseBlock::Table(table) => {
                render_prose_table(
                    output, block, &table, context, resolved, index, errors,
                );
            }
            ProseBlock::DisplayMath { source, .. } => {
                render_math_placeholder(
                    output,
                    &MathFragment {
                        kind: MathKind::Display,
                        source,
                    },
                );
            }
        }
    }
}

fn render_paragraph(
    output: &mut String,
    block: &Block,
    lines: std::ops::Range<usize>,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    output.push_str("<p>");
    let mut source = InlineSource::new();
    for line in lines {
        source.push_line(&block.lines[line].text, &block.lines[line].origin);
    }
    render_inline_source(output, &source, context, resolved, index, errors);
    output.push_str("</p>\n");
}

fn render_prose_list(
    output: &mut String,
    block: &Block,
    list: &ProseList,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    if let Some(start) = list.ordered_start {
        output.push_str("<ol");
        if start != 1 {
            let _ = write!(output, " start=\"{start}\"");
        }
        output.push_str(">\n");
    } else {
        output.push_str("<ul>\n");
    }

    for item in &list.items {
        output.push_str("<li>");
        let mut source = InlineSource::new();
        for part in &item.parts {
            let line = &block.lines[part.line];
            source.push_line(&line.text[part.content_start..], &line.origin);
        }
        render_inline_source(output, &source, context, resolved, index, errors);
        output.push_str("</li>\n");
    }
    output.push_str(if list.ordered_start.is_some() {
        "</ol>\n"
    } else {
        "</ul>\n"
    });
}

fn render_fenced_prose(
    output: &mut String,
    block: &Block,
    fence: &FencedProse,
    context: &RenderContext<'_>,
) {
    let language_name = fence.info.split_whitespace().next();
    let language = language_name
        .filter(|name| {
            !matches!(
                name.to_ascii_lowercase().as_str(),
                "text" | "plain" | "plaintext"
            )
        })
        .and_then(code_language);

    output.push_str("<pre");
    if let Some(language) = &language {
        output.push_str(" class=\"language-");
        push_escaped_attribute(output, &language.primary);
        output.push('"');
    }
    output.push_str("><code");
    if let Some(language) = &language {
        output.push_str(" class=\"language-");
        push_escaped_attribute(output, &language.primary);
        if context.highlighting {
            output
                .push_str("\" data-litweb-highlight=\"pending\" data-litweb-language=\"");
            push_escaped_attribute(output, &language.candidates);
        }
        output.push('"');
    }
    output.push('>');

    for line in fence.content_start..fence.content_end {
        let text = &block.lines[line].text;
        let indentation = text
            .bytes()
            .take(fence.indentation)
            .take_while(|byte| *byte == b' ')
            .count();
        push_escaped_text(output, &text[indentation..]);
        output.push('\n');
    }
    output.push_str("</code></pre>\n");
}

// Render a prose table
fn render_prose_table(
    output: &mut String,
    block: &Block,
    table: &ProseTable,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    output.push_str("<div class=\"table-scroll\">\n<table>\n<thead>\n<tr>\n");
    for (column, cell) in table.header.iter().enumerate() {
        output.push_str("<th scope=\"col\" class=\"table-align-");
        output.push_str(table_alignment_name(table.alignments[column]));
        output.push_str("\">");
        render_table_cell(output, block, cell, context, resolved, index, errors);
        output.push_str("</th>\n");
    }
    output.push_str("</tr>\n</thead>\n");

    if !table.body.is_empty() {
        output.push_str("<tbody>\n");
        for row in &table.body {
            output.push_str("<tr>\n");
            for (column, cell) in row.iter().enumerate() {
                output.push_str("<td class=\"table-align-");
                output.push_str(table_alignment_name(table.alignments[column]));
                output.push_str("\">");
                render_table_cell(output, block, cell, context, resolved, index, errors);
                output.push_str("</td>\n");
            }
            output.push_str("</tr>\n");
        }
        output.push_str("</tbody>\n");
    }
    output.push_str("</table>\n</div>\n");
}

fn render_table_cell(
    output: &mut String,
    block: &Block,
    cell: &TableCell,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    render_inline(
        output,
        &cell.text,
        &block.lines[cell.line].origin,
        context,
        resolved,
        index,
        errors,
    );
}

fn table_alignment_name(alignment: TableAlignment) -> &'static str {
    match alignment {
        TableAlignment::Left => "left",
        TableAlignment::Center => "center",
        TableAlignment::Right => "right",
    }
}

// Recognize inline prose math
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MathKind {
    Inline,
    Display,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MathFragment {
    kind: MathKind,
    source: String,
}

// Render a math placeholder
fn render_math_placeholder(output: &mut String, math: &MathFragment) {
    match math.kind {
        MathKind::Inline => {
            output.push_str("<span class=\"litweb-math litweb-math-inline\">")
        }
        MathKind::Display => {
            output.push_str("<div class=\"litweb-math litweb-math-display\">")
        }
    }
    push_escaped_text(output, &math.source);
    output.push_str(match math.kind {
        MathKind::Inline => "</span>",
        MathKind::Display => "</div>\n",
    });
}

// Scan inline Markdown
fn render_inline(
    output: &mut String,
    text: &str,
    origin: &SourceOrigin,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    let mut source = InlineSource::new();
    source.push_line(text, origin);
    render_inline_source(output, &source, context, resolved, index, errors);
}

fn render_inline_source(
    output: &mut String,
    source: &InlineSource,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    render_inline_elements(
        output,
        &inline_elements(source),
        context,
        resolved,
        index,
        errors,
    );
}

fn render_inline_elements(
    output: &mut String,
    elements: &[InlineElement],
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    for element in elements {
        match element {
            InlineElement::Text(text) => push_escaped_text(output, text),
            InlineElement::Code(code) => {
                output.push_str("<code>");
                push_escaped_text(output, code);
                output.push_str("</code>");
            }
            InlineElement::SourceCode { text: code, .. } => {
                output.push_str("<code class=\"source-code\">");
                push_escaped_text(output, code);
                output.push_str("</code>");
            }
            InlineElement::Math(source) => render_math_placeholder(
                output,
                &MathFragment {
                    kind: MathKind::Inline,
                    source: source.clone(),
                },
            ),
            InlineElement::Strong(contents) => {
                output.push_str("<strong>");
                render_inline_elements(
                    output, contents, context, resolved, index, errors,
                );
                output.push_str("</strong>");
            }
            InlineElement::Emphasis(contents) => {
                output.push_str("<em>");
                render_inline_elements(
                    output, contents, context, resolved, index, errors,
                );
                output.push_str("</em>");
            }
            InlineElement::Link {
                label,
                target,
                active,
                origin,
            } => {
                if *active {
                    output.push_str("<a href=\"");
                    match index.chapter_link(target, context.current_chapter) {
                        Ok(Some(chapter)) => {
                            let paths = context.book_paths.expect("book link paths");
                            let url = relative_url(
                                context.current_path,
                                &paths.chapters[chapter],
                            );
                            push_escaped_attribute(output, &url);
                        }
                        Ok(None) => push_escaped_attribute(output, target),
                        Err(()) => errors.push(WeaveError {
                            origin: origin.clone(),
                            kind: WeaveErrorKind::InvalidChapterLink {
                                target: target.clone(),
                            },
                        }),
                    }
                    output.push_str("\">");
                    render_inline_elements(
                        output, label, context, resolved, index, errors,
                    );
                    output.push_str("</a>");
                } else {
                    output.push('[');
                    render_inline_elements(
                        output, label, context, resolved, index, errors,
                    );
                    output.push_str("](");
                    push_escaped_text(output, target);
                    output.push(')');
                }
            }
            InlineElement::BlockReference { name, origin } => {
                render_code_reference(
                    output, name, origin, context, resolved, index, errors,
                );
            }
        }
    }
}

// Render a prose code reference

fn render_code_reference(
    output: &mut String,
    name: &str,
    origin: &SourceOrigin,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    let location = definition_location(
        name,
        origin,
        context.current_chapter,
        resolved,
        index,
        errors,
    );
    output.push_str("<code class=\"section-reference\">");
    render_section_name(output, name, location, false, context);
    output.push_str("</code>");
}

// Render a section name

fn render_section_name(
    output: &mut String,
    name: &str,
    location: Option<&SectionLocation>,
    root: bool,
    context: &RenderContext<'_>,
) {
    output.push('⟨');
    if root {
        output.push_str("<strong>");
    }
    push_escaped_text(output, name);
    if root {
        output.push_str("</strong>");
    }
    if let Some(location) = location {
        output.push(' ');
        render_location_link(output, location, context);
    }
    output.push('⟩');
}

// Render a code block
fn render_code_block(
    output: &mut String,
    block: &Block,
    location: &SectionLocation,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    output.push_str("<div class=\"codeblock\">\n");
    render_code_block_name(output, block, location, context, resolved, index, errors);
    render_code_block_contents(output, block, location, context, resolved, index, errors);
    output.push_str("</div>\n");
}

// Render a code-block title

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

fn render_code_block_name(
    output: &mut String,
    block: &Block,
    location: &SectionLocation,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
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
    output.push_str("<div class=\"codeblock_name\"><span class=\"section-name\">");
    render_section_name(
        output,
        &code.name,
        definition,
        identity.is_some_and(|identity| index.roots.contains(&identity)),
        context,
    );
    output.push_str("</span> ");
    let (operator, label) = if code.modifiers.contains(&Modifier::Additive) {
        ("+≡", "is continued by")
    } else if code.modifiers.contains(&Modifier::Redefinition) {
        (":=", "is replaced by")
    } else {
        ("≡", "is defined as")
    };
    output.push_str("<span class=\"definition-operator\" aria-label=\"");
    output.push_str(label);
    output.push_str("\">");
    output.push_str(operator);
    output.push_str("</span></div>\n");
}

// Render code-block contents

fn render_code_block_contents(
    output: &mut String,
    block: &Block,
    location: &SectionLocation,
    context: &RenderContext<'_>,
    resolved: &ResolvedProgram,
    index: &WeaveIndex,
    errors: &mut Vec<WeaveError>,
) {
    let code = block.code().expect("the caller selected a code block");
    let identity = code_identity(block, location, resolved);
    let language = code_language(&code.code_type);
    output.push_str("<pre");
    if let Some(language) = &language {
        output.push_str(" class=\"language-");
        push_escaped_attribute(output, &language.primary);
        output.push('"');
    }
    output.push_str("><code");
    if let Some(language) = &language {
        output.push_str(" class=\"language-");
        push_escaped_attribute(output, &language.primary);
        if context.highlighting {
            output
                .push_str("\" data-litweb-highlight=\"pending\" data-litweb-language=\"");
            push_escaped_attribute(output, &language.candidates);
        }
        output.push('"');
    }
    output.push('>');

    for line in &block.lines {
        if let Some(name) = block_reference(&line.text) {
            output.push_str("<span class=\"nocode\">");
            push_escaped_text(output, leading_whitespace(&line.text));
            let definition = definition_location(
                name,
                &line.origin,
                Some(location.chapter),
                resolved,
                index,
                errors,
            );
            render_section_name(output, name, definition, false, context);
            output.push_str("</span>\n");
        } else {
            push_escaped_text(output, &line.text);
            output.push('\n');
        }
    }
    output.push_str("</code></pre>\n");

    if let Some(locations) = identity.and_then(|identity| index.blocks.get(&identity)) {
        render_relationship(
            output,
            CodeRelationship::Addition,
            &locations.additions,
            location,
            context,
        );
        render_relationship(
            output,
            CodeRelationship::Replacement,
            &locations.redefinitions,
            location,
            context,
        );
        render_relationship(
            output,
            CodeRelationship::Use,
            &locations.uses,
            location,
            context,
        );
    }
}

// Choose a code language class

struct CodeLanguage {
    primary: String,
    candidates: String,
}

fn code_language(code_type: &str) -> Option<CodeLanguage> {
    let mut parts = code_type.split_whitespace();
    let name = parts.next()?;
    let extension = parts.next().and_then(|value| value.strip_prefix('.'));
    let primary = normalize_code_language(name)
        .or_else(|| extension.and_then(normalize_code_language))?;
    let mut candidates = primary.clone();
    if let Some(extension) = extension.and_then(normalize_code_language)
        && extension != primary
    {
        candidates.push(' ');
        candidates.push_str(&extension);
    }
    Some(CodeLanguage {
        primary,
        candidates,
    })
}

fn normalize_code_language(value: &str) -> Option<String> {
    let lowercase = value.to_ascii_lowercase();
    let normalized = match lowercase.as_str() {
        "c++" | "objective-c++" => "cpp",
        "c#" => "csharp",
        "f#" => "fsharp",
        "objective-c" => "objectivec",
        "shell" => "bash",
        "html" | "xml" => "markup",
        "tex" => "latex",
        value => value,
    };
    (!normalized.is_empty()
        && normalized.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        }))
    .then(|| normalized.to_owned())
}

// Find a visible definition location
fn definition_location<'a>(
    name: &str,
    origin: &SourceOrigin,
    requesting_chapter: Option<usize>,
    resolved: &ResolvedProgram,
    index: &'a WeaveIndex,
    errors: &mut Vec<WeaveError>,
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
            errors.push(WeaveError {
                origin: origin.clone(),
                kind: WeaveErrorKind::UndefinedBlock {
                    name: name.to_owned(),
                },
            });
            return None;
        }
        BlockLookup::Ambiguous(candidates) => {
            errors.push(WeaveError {
                origin: origin.clone(),
                kind: WeaveErrorKind::AmbiguousBlock {
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

    if let Some(locations) = index.blocks.get(&identity) {
        if locations.definition_hidden {
            return None;
        }
        if let Some(location) = &locations.definition {
            return Some(location);
        }
    }
    None
}

// Render one section-location link

fn render_location_link(
    output: &mut String,
    location: &SectionLocation,
    context: &RenderContext<'_>,
) {
    let label = location.label(context.current_chapter);
    output.push_str("<a href=\"");
    if context.single_page || context.current_chapter == Some(location.chapter) {
        output.push('#');
    } else {
        let target = &context
            .book_paths
            .expect("cross-page links have HTML book paths")
            .chapters[location.chapter];
        push_escaped_attribute(output, &relative_url(context.current_path, target));
        output.push('#');
    }
    push_escaped_attribute(output, &location.anchor());
    output.push_str("\" aria-label=\"section ");
    push_escaped_attribute(output, &label);
    output.push_str("\">");
    push_escaped_text(output, &label);
    output.push_str("</a>");
}

// Code relationship wording

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

// Render relationship locations

fn render_relationship(
    output: &mut String,
    relationship: CodeRelationship,
    locations: &[SectionLocation],
    current: &SectionLocation,
    context: &RenderContext<'_>,
) {
    let locations = locations
        .iter()
        .filter(|location| *location != current)
        .collect::<Vec<_>>();
    if locations.is_empty() {
        return;
    }

    output.push_str("<p class=\"seealso\">");
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
        render_location_link(output, location, context);
    }
    output.push_str(".</p>\n");
}

// Weaver HTML support and styles
// Build a relative page URL
fn relative_url(from_page: &Path, to_page: &Path) -> String {
    let from = path_components(from_page.parent().unwrap_or_else(|| Path::new("")));
    let to = path_components(to_page);
    let mut common = 0;
    while common < from.len() && common < to.len() && from[common] == to[common] {
        common += 1;
    }

    let mut components = Vec::new();
    components.extend(std::iter::repeat_n("..".to_owned(), from.len() - common));
    components.extend(
        to[common..]
            .iter()
            .map(|component| encode_url_component(component)),
    );
    if components.is_empty() {
        ".".to_owned()
    } else {
        components.join("/")
    }
}

fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(component) => {
                Some(component.to_string_lossy().into_owned())
            }
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_owned()),
            Component::RootDir | Component::Prefix(_) => None,
        })
        .collect()
}

// Percent-encode a URL component

fn encode_url_component(component: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in component.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

// Escape HTML text and attributes

fn push_escaped_text(output: &mut String, text: &str) {
    for character in text.chars() {
        push_escaped_char(output, character);
    }
}

fn push_escaped_attribute(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            character => push_escaped_char(output, character),
        }
    }
}

fn push_escaped_char(output: &mut String, character: char) {
    match character {
        '&' => output.push_str("&amp;"),
        '<' => output.push_str("&lt;"),
        '>' => output.push_str("&gt;"),
        character => output.push(character),
    }
}

// Math asset model
const MATH_ASSET_DIRECTORY: &str = "litweb-assets/katex-0.18.1";
const INLINE_MATH_MARKER: &[u8] = b"<span class=\"litweb-math litweb-math-inline\">";
const DISPLAY_MATH_MARKER: &[u8] = b"<div class=\"litweb-math litweb-math-display\">";

struct MathAsset {
    path: &'static str,
    bytes: &'static [u8],
}

const MATH_ASSETS: &[MathAsset] = &[
    MathAsset {
        path: "katex.min.js",
        bytes: include_bytes!("../assets/katex/0.18.1/katex.min.js"),
    },
    MathAsset {
        path: "katex.min.css",
        bytes: include_bytes!("../assets/katex/0.18.1/katex.min.css"),
    },
    MathAsset {
        path: "litweb-math.js",
        bytes: include_bytes!("../assets/katex/0.18.1/litweb-math.js"),
    },
    MathAsset {
        path: "LICENSE",
        bytes: include_bytes!("../assets/katex/0.18.1/LICENSE"),
    },
    MathAsset {
        path: "README.md",
        bytes: include_bytes!("../assets/katex/0.18.1/README.md"),
    },
    MathAsset {
        path: "fonts/KaTeX_AMS-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_AMS-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Caligraphic-Bold.woff2",
        bytes: include_bytes!(
            "../assets/katex/0.18.1/fonts/KaTeX_Caligraphic-Bold.woff2"
        ),
    },
    MathAsset {
        path: "fonts/KaTeX_Caligraphic-Regular.woff2",
        bytes: include_bytes!(
            "../assets/katex/0.18.1/fonts/KaTeX_Caligraphic-Regular.woff2"
        ),
    },
    MathAsset {
        path: "fonts/KaTeX_Fraktur-Bold.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Fraktur-Bold.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Fraktur-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Fraktur-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Main-Bold.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Main-Bold.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Main-BoldItalic.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Main-BoldItalic.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Main-Italic.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Main-Italic.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Main-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Main-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Math-BoldItalic.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Math-BoldItalic.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Math-Italic.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Math-Italic.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_SansSerif-Bold.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_SansSerif-Bold.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_SansSerif-Italic.woff2",
        bytes: include_bytes!(
            "../assets/katex/0.18.1/fonts/KaTeX_SansSerif-Italic.woff2"
        ),
    },
    MathAsset {
        path: "fonts/KaTeX_SansSerif-Regular.woff2",
        bytes: include_bytes!(
            "../assets/katex/0.18.1/fonts/KaTeX_SansSerif-Regular.woff2"
        ),
    },
    MathAsset {
        path: "fonts/KaTeX_Script-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Script-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Size1-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Size1-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Size2-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Size2-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Size3-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Size3-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Size4-Regular.woff2",
        bytes: include_bytes!("../assets/katex/0.18.1/fonts/KaTeX_Size4-Regular.woff2"),
    },
    MathAsset {
        path: "fonts/KaTeX_Typewriter-Regular.woff2",
        bytes: include_bytes!(
            "../assets/katex/0.18.1/fonts/KaTeX_Typewriter-Regular.woff2"
        ),
    },
];

fn math_asset_path(path: &str) -> PathBuf {
    Path::new(MATH_ASSET_DIRECTORY).join(path)
}

// Highlight asset model
const HIGHLIGHT_ASSET_DIRECTORY: &str = "litweb-assets/prism-1.30.0";
const HIGHLIGHT_PAGE_MARKER: &[u8] = b"data-litweb-highlight-ready=\"pending\"";

struct HighlightAsset {
    path: &'static str,
    bytes: &'static [u8],
}

const HIGHLIGHT_ASSETS: &[HighlightAsset] = &[
    HighlightAsset {
        path: "prism-all.min.js",
        bytes: include_bytes!("../assets/prism/1.30.0/prism-all.min.js"),
    },
    HighlightAsset {
        path: "litweb-highlight.js",
        bytes: include_bytes!("../assets/prism/1.30.0/litweb-highlight.js"),
    },
    HighlightAsset {
        path: "LICENSE-Prism",
        bytes: include_bytes!("../assets/prism/1.30.0/LICENSE-Prism"),
    },
    HighlightAsset {
        path: "LICENSE-Primer",
        bytes: include_bytes!("../assets/prism/1.30.0/LICENSE-Primer"),
    },
    HighlightAsset {
        path: "README.md",
        bytes: include_bytes!("../assets/prism/1.30.0/README.md"),
    },
    HighlightAsset {
        path: "themes/litweb.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/litweb.css"),
    },
    HighlightAsset {
        path: "themes/default.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/default.css"),
    },
    HighlightAsset {
        path: "themes/dark.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/dark.css"),
    },
    HighlightAsset {
        path: "themes/funky.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/funky.css"),
    },
    HighlightAsset {
        path: "themes/okaidia.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/okaidia.css"),
    },
    HighlightAsset {
        path: "themes/twilight.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/twilight.css"),
    },
    HighlightAsset {
        path: "themes/coy.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/coy.css"),
    },
    HighlightAsset {
        path: "themes/solarized-light.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/solarized-light.css"),
    },
    HighlightAsset {
        path: "themes/tomorrow-night.css",
        bytes: include_bytes!("../assets/prism/1.30.0/themes/tomorrow-night.css"),
    },
];

fn highlight_asset_path(path: &str) -> PathBuf {
    Path::new(HIGHLIGHT_ASSET_DIRECTORY).join(path)
}

// Render math page assets
fn page_contains_math(body: &str) -> bool {
    body.contains("<span class=\"litweb-math litweb-math-inline\">")
        || body.contains("<div class=\"litweb-math litweb-math-display\">")
}

fn render_html_element_open(
    output: &mut String,
    uses_math: bool,
    uses_highlighting: bool,
) {
    output.push_str("<html lang=\"en\"");
    if uses_math || uses_highlighting {
        output.push_str(" data-litweb-ready=\"pending\"");
    }
    if uses_math {
        output.push_str(" data-litweb-math-ready=\"pending\"");
    }
    if uses_highlighting {
        output.push_str(" data-litweb-highlight-ready=\"pending\"");
    }
    output.push_str(">\n");
}

fn render_math_stylesheet_link(output: &mut String, current_path: &Path) {
    output.push_str("<link rel=\"stylesheet\" href=\"");
    push_escaped_attribute(
        output,
        &relative_url(current_path, &math_asset_path("katex.min.css")),
    );
    output.push_str("\">\n");
}

fn render_math_scripts(output: &mut String, current_path: &Path) {
    for script in ["katex.min.js", "litweb-math.js"] {
        output.push_str("<script defer src=\"");
        push_escaped_attribute(
            output,
            &relative_url(current_path, &math_asset_path(script)),
        );
        output.push_str("\"></script>\n");
    }
}

// Render highlighting page assets
fn page_contains_highlighting(body: &str) -> bool {
    body.contains("data-litweb-highlight=\"pending\"")
}

fn render_highlight_scripts(output: &mut String, current_path: &Path) {
    for script in ["prism-all.min.js", "litweb-highlight.js"] {
        output.push_str("<script defer src=\"");
        push_escaped_attribute(
            output,
            &relative_url(current_path, &highlight_asset_path(script)),
        );
        output.push_str("\"></script>\n");
    }
}

// Highlight theme styles
impl BuiltInColorScheme {
    fn css(self) -> &'static str {
        match self {
            Self::Litweb => include_str!("../assets/prism/1.30.0/themes/litweb.css"),
            Self::PrismDefault => {
                include_str!("../assets/prism/1.30.0/themes/default.css")
            }
            Self::Dark => include_str!("../assets/prism/1.30.0/themes/dark.css"),
            Self::Funky => include_str!("../assets/prism/1.30.0/themes/funky.css"),
            Self::Okaidia => include_str!("../assets/prism/1.30.0/themes/okaidia.css"),
            Self::Twilight => include_str!("../assets/prism/1.30.0/themes/twilight.css"),
            Self::Coy => include_str!("../assets/prism/1.30.0/themes/coy.css"),
            Self::SolarizedLight => {
                include_str!("../assets/prism/1.30.0/themes/solarized-light.css")
            }
            Self::TomorrowNight => {
                include_str!("../assets/prism/1.30.0/themes/tomorrow-night.css")
            }
        }
    }
}

fn render_highlight_theme_css(output: &mut String, scheme: &PageColorScheme) {
    if let Some(css) = scheme.css() {
        push_style_text(output, css);
        output.push('\n');
    }
    if matches!(scheme, PageColorScheme::BuiltIn(_)) {
        output.push_str(BUILT_IN_HIGHLIGHT_LAYOUT_STYLE);
    }
}

const BUILT_IN_HIGHLIGHT_LAYOUT_STYLE: &str = concat!(
    "/* Litweb owns code typography and panel geometry. */\n",
    r#"code[class*="language-"],
pre[class*="language-"] {
    font-family: monospace;
    font-size: inherit;
}
code[class*="language-"] {
    line-height: inherit;
}
pre[class*="language-"] {
    margin: 1em 0;
    padding: 0.8rem 1rem;
    line-height: 1.35;
}
"#,
);

fn push_style_text(output: &mut String, css: &str) {
    let mut position = 0;
    while position < css.len() {
        let rest = &css[position..];
        if rest
            .as_bytes()
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"</style"))
        {
            output.push_str("<\\/style");
            position += 7;
        } else {
            let character = rest
                .chars()
                .next()
                .expect("position is on a character boundary");
            output.push(character);
            position += character.len_utf8();
        }
    }
}

// Default page style
const DEFAULT_STYLE: &str = r#"body {
    color: #222;
    background: #fff;
    font-family: Georgia, serif;
    line-height: 1.5;
    margin: 0;
}
main {
    max-width: 56rem;
    margin: 0 auto;
    padding: clamp(0.75rem, 4vw, 2rem);
}
.section {
    margin: 2.5rem 0;
}
.section-heading {
    font-size: 1rem;
    margin: 0 0 1rem;
}
.section-opening > .section-heading,
.section-opening > p,
.section-opening > .codeblock_name {
    display: inline;
    margin: 0;
}
.section-number,
.chapter-number,
.definition-operator {
    white-space: nowrap;
}
.visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
}
.codeblock {
    margin: 1.25rem 0;
}
.codeblock-first {
    margin-top: 0;
}
.codeblock_name {
    margin-left: min(1rem, 3vw);
    font-family: monospace;
    overflow-wrap: anywhere;
}
.section-opening > .codeblock_name {
    margin-left: 0;
}
.definition-operator {
    font-family: Georgia, serif;
}
code {
    white-space: pre-wrap;
}
pre code {
    white-space: inherit;
}
pre {
    overflow-x: auto;
    border: 1px solid #d8d8d8;
    background: #f7f7f7;
    padding: 0.8rem 1rem;
    line-height: 1.35;
}
.table-scroll {
    overflow-x: auto;
    margin: 1.25rem 0;
}
.table-scroll table {
    border-collapse: collapse;
    width: max-content;
    min-width: 100%;
}
.table-scroll th,
.table-scroll td {
    border: 1px solid #d8d8d8;
    padding: 0.45rem 0.7rem;
    vertical-align: top;
}
.table-scroll th {
    background: #f7f7f7;
}
.table-align-left {
    text-align: left;
}
.table-align-center {
    text-align: center;
}
.table-align-right {
    text-align: right;
}
.nocode {
    color: #555;
    font-style: italic;
}
.seealso {
    margin: 0.2rem 0 0 min(1rem, 3vw);
    font-size: 0.9rem;
    color: #555;
}
.section-name a,
.section-reference a,
.nocode a,
.seealso a {
    text-decoration: none;
}
.section-name a:focus-visible,
.section-reference a:focus-visible,
.nocode a:focus-visible,
.seealso a:focus-visible {
    outline: 2px solid currentColor;
    outline-offset: 2px;
}
a {
    color: #245b8a;
}
"#;

// Math page style
const MATH_STYLE: &str = r#".litweb-math:not(.litweb-math-rendered) {
    font-family: monospace;
    white-space: pre-wrap;
}
.litweb-math-display {
    margin: 1rem 0;
    overflow-x: auto;
    overflow-y: hidden;
    text-align: center;
}
.litweb-math-display .katex-display {
    margin: 0;
}
.litweb-math .katex {
    font-size: 1.1em;
}
.litweb-math-error {
    color: #9b1c1c;
}
.litweb-math-error-message {
    font-family: system-ui, sans-serif;
    font-size: 0.85em;
    font-style: normal;
}
.litweb-math-display .litweb-math-error-message {
    display: block;
    margin-top: 0.35rem;
}
@media print {
    .litweb-math-display {
        break-inside: avoid;
        page-break-inside: avoid;
        overflow-x: visible;
        overflow-y: visible;
    }
}
"#;

// Identifier index style
const IDENTIFIER_STYLE: &str = r#".identifier-index {
    margin: 3rem 0 1rem;
    padding-top: 1rem;
    border-top: 1px solid #d8d8d8;
    font-family: Arial, Helvetica, sans-serif;
    font-variant-numeric: lining-nums tabular-nums;
}
.identifier-index dl {
    display: grid;
    grid-template-columns: minmax(8rem, 14rem) 1fr;
    gap: 0.25rem 1rem;
}
.identifier-index dt,
.identifier-index dd {
    margin: 0;
}
.identifier-index dt {
    overflow-wrap: anywhere;
}
.identifier-index dd {
    font-size: 0.875em;
}
.identifier-index a {
    text-decoration: none;
}
.identifier-index a.identifier-definition {
    text-decoration: underline;
    text-decoration-thickness: 0.12em;
    text-underline-offset: 0.12em;
    text-decoration-skip-ink: none;
}
.identifier-index a:focus-visible {
    outline: 2px solid currentColor;
    outline-offset: 2px;
}
@media (max-width: 36rem) {
    .identifier-index dl {
        grid-template-columns: 1fr;
    }
    .identifier-index dd {
        margin-bottom: 0.5rem;
    }
}
"#;

// Book navigation style

const BOOK_STYLE: &str = r#".book-navigation {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 1rem;
    margin: 1.5rem 0;
}
.book-navigation .contents {
    margin-inline: auto;
}
.book-contents ol {
    list-style: none;
    margin-top: 0.35rem;
    padding-left: 1.5rem;
}
.book-contents > ol {
    padding-left: 0;
}
"#;

// Book identifier-index style
const BOOK_IDENTIFIER_STYLE: &str = r#".book-navigation-indexes {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 0.25rem 1rem;
    margin-inline: auto;
}
.book-navigation-indexes .contents {
    margin-inline: 0;
}
.book-index-navigation {
    margin: 1.5rem 0;
}
.identifier-index-book-title {
    font-style: italic;
}
.identifier-chapter-group + .identifier-chapter-group {
    margin-top: 0.25rem;
}
"#;
