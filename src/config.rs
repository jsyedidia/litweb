// src/config.rs
// Configuration imports
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

use crate::latex::{LatexChapterOpening, LatexFontSize};

// Help text
pub const HELP_TEXT: &str = "\
Litweb: Literate Programming System

Usage: lw [options] [--] <input>

Use -- before an input whose name begins with -.

Options:
--tangle     -t         Only generate code files
--weave      -w         Only generate the selected woven format
--format FORMAT         Select html (default) or latex output
--font-size SIZE        Set LaTeX text to 10pt, 11pt, or 12pt (default)
--chapter-opening SIDE  Start LaTeX book chapters on right (default) or left
--no-headers            Do not add block-name comments to code
--no-block-separators   Do not separate adjacent expanded blocks
--no-index              Do not generate identifier indexes
--colorscheme SCHEME    Select an HTML highlighting color scheme
--no-highlight          Do not syntax-highlight HTML target code
--out-dir    -odir DIR  Put generated files in DIR
--help       -h         Show this help text
--version    -v         Show the version number
";

// Action enum
/// An action selected from command-line arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Print command help without processing an input.
    Help,
    /// Print the package version without processing an input.
    Version,
    /// Process one input with the completed configuration.
    Run(RunConfig),
}

/// The output planners selected for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Plan both tangled code and the selected woven format.
    Default,
    /// Plan only tangled code.
    TangleOnly,
    /// Plan only the selected woven format.
    WeaveOnly,
}

/// The representation produced by the weaving stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaveFormat {
    /// Produce linked offline HTML, the default.
    Html,
    /// Produce one LuaLaTeX document and its packaged style.
    Latex,
}

/// Every choice needed to process one input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    /// The `.lit` chapter or book manifest to read.
    pub input: PathBuf,
    /// The output directory, or `.` when no directory option was supplied.
    pub output_directory: PathBuf,
    /// Which output planners run; `Default` means both.
    pub mode: RunMode,
    /// Which woven representation to produce; HTML is the default.
    pub weave_format: WeaveFormat,
    /// Whether Tangler emits generated block-name comments; normally true.
    pub tangle_headers: bool,
    /// Whether Tangler separates adjacent expanded blocks; normally true.
    pub tangle_block_separators: bool,
    /// Whether woven output includes a target-language identifier index; normally true.
    pub identifier_index: bool,
    /// The base font size for LaTeX output; normally 12 points.
    pub latex_font_size: LatexFontSize,
    /// The page side on which major LaTeX book chapters begin; normally right.
    pub latex_chapter_opening: LatexChapterOpening,
    /// A command-line override; `None` keeps document/default behavior and
    /// `"none"` disables highlighting.
    pub color_scheme: Option<PathBuf>,
}

// CLI error
/// A problem found while converting command-line arguments into an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    MissingInput,
    MultipleInputs,
    MissingOptionValue(&'static str),
    RepeatedOption(&'static str),
    ConflictingOptions,
    ConflictingHighlightOptions,
    UnknownFormat(OsString),
    UnknownFontSize(OsString),
    UnknownChapterOpening(OsString),
    FontSizeRequiresLatex,
    ChapterOpeningRequiresLatex,
    IncompatibleFormatOption(&'static str),
    UnsupportedOption(OsString),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingInput => f.write_str("no input file provided"),
            Self::MultipleInputs => f.write_str("only one input file is supported"),
            Self::MissingOptionValue(option) => {
                write!(f, "option {option} requires a value")
            }
            Self::RepeatedOption(option) => {
                write!(f, "option {option} was provided more than once")
            }
            Self::ConflictingOptions => {
                f.write_str("options --tangle and --weave cannot be used together")
            }
            Self::ConflictingHighlightOptions => f.write_str(
                "options --colorscheme and --no-highlight cannot be used together",
            ),
            Self::UnknownFormat(format) => write!(
                f,
                "unknown woven format {}; expected html or latex",
                format.to_string_lossy()
            ),
            Self::UnknownFontSize(size) => write!(
                f,
                "unknown LaTeX font size {}; expected 10pt, 11pt, or 12pt",
                size.to_string_lossy()
            ),
            Self::FontSizeRequiresLatex => {
                f.write_str("option --font-size requires --format latex")
            }
            Self::UnknownChapterOpening(opening) => write!(
                f,
                "unknown LaTeX chapter opening {}; expected right or left",
                opening.to_string_lossy()
            ),
            Self::ChapterOpeningRequiresLatex => {
                f.write_str("option --chapter-opening requires --format latex")
            }
            Self::IncompatibleFormatOption(option) => {
                write!(f, "option {option} cannot be used with --format latex")
            }
            Self::UnsupportedOption(option) => {
                write!(f, "unsupported option: {}", option.to_string_lossy())
            }
        }
    }
}

impl std::error::Error for CliError {}

// Recognize option-shaped arguments
fn starts_with_hyphen(argument: &OsStr) -> bool {
    argument.to_string_lossy().starts_with('-')
}

// Parse arguments function
/// Converts operating-system arguments into one requested action.
///
/// Before `--`, every argument beginning with an ASCII hyphen is treated as
/// option-shaped even when the complete value is not valid UTF-8. After `--`,
/// all remaining arguments are positional.
pub fn parse_args<I>(args: I) -> Result<Action, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    // Initialize argument state
    let mut args = args.into_iter();
    let mut input = None;
    let mut output_directory = None;
    let mut mode = None;
    let mut weave_format = None;
    let mut no_headers = false;
    let mut no_block_separators = false;
    let mut no_index = false;
    let mut latex_font_size = None;
    let mut latex_chapter_opening = None;
    let mut color_scheme = None;
    let mut no_highlight = false;
    let mut options_ended = false;

    // Scan command-line arguments
    while let Some(arg) = args.next() {
        if options_ended {
            if input.is_some() {
                return Err(CliError::MultipleInputs);
            }
            input = Some(PathBuf::from(arg));
            continue;
        }
        if arg == OsStr::new("--") {
            options_ended = true;
            continue;
        }

        match arg.to_str() {
            Some("--help" | "-h") => return Ok(Action::Help),
            Some("--version" | "-v") => return Ok(Action::Version),
            Some("--format") => {
                if weave_format.is_some() {
                    return Err(CliError::RepeatedOption("--format"));
                }
                let value = args
                    .next()
                    .ok_or(CliError::MissingOptionValue("--format"))?;
                if starts_with_hyphen(&value) {
                    return Err(CliError::MissingOptionValue("--format"));
                }
                weave_format = Some(match value.to_str() {
                    Some("html") => WeaveFormat::Html,
                    Some("latex") => WeaveFormat::Latex,
                    _ => return Err(CliError::UnknownFormat(value)),
                });
            }
            Some("--font-size") => {
                if latex_font_size.is_some() {
                    return Err(CliError::RepeatedOption("--font-size"));
                }
                let value = args
                    .next()
                    .ok_or(CliError::MissingOptionValue("--font-size"))?;
                if starts_with_hyphen(&value) {
                    return Err(CliError::MissingOptionValue("--font-size"));
                }
                latex_font_size = Some(match value.to_str() {
                    Some("10pt") => LatexFontSize::TenPoint,
                    Some("11pt") => LatexFontSize::ElevenPoint,
                    Some("12pt") => LatexFontSize::TwelvePoint,
                    _ => return Err(CliError::UnknownFontSize(value)),
                });
            }
            Some("--chapter-opening") => {
                if latex_chapter_opening.is_some() {
                    return Err(CliError::RepeatedOption("--chapter-opening"));
                }
                let value = args
                    .next()
                    .ok_or(CliError::MissingOptionValue("--chapter-opening"))?;
                if starts_with_hyphen(&value) {
                    return Err(CliError::MissingOptionValue("--chapter-opening"));
                }
                latex_chapter_opening = Some(match value.to_str() {
                    Some("right") => LatexChapterOpening::Right,
                    Some("left") => LatexChapterOpening::Left,
                    _ => return Err(CliError::UnknownChapterOpening(value)),
                });
            }
            Some("--no-headers") => {
                if no_headers {
                    return Err(CliError::RepeatedOption("--no-headers"));
                }
                no_headers = true;
            }
            Some("--no-block-separators") => {
                if no_block_separators {
                    return Err(CliError::RepeatedOption("--no-block-separators"));
                }
                no_block_separators = true;
            }
            Some("--no-highlight") => {
                if no_highlight {
                    return Err(CliError::RepeatedOption("--no-highlight"));
                }
                if color_scheme.is_some() {
                    return Err(CliError::ConflictingHighlightOptions);
                }
                no_highlight = true;
            }
            Some("--colorscheme") => {
                if color_scheme.is_some() {
                    return Err(CliError::RepeatedOption("--colorscheme"));
                }
                if no_highlight {
                    return Err(CliError::ConflictingHighlightOptions);
                }
                let scheme = args
                    .next()
                    .ok_or(CliError::MissingOptionValue("--colorscheme"))?;
                if starts_with_hyphen(&scheme) {
                    return Err(CliError::MissingOptionValue("--colorscheme"));
                }
                color_scheme = Some(scheme);
            }
            Some("--no-index") => {
                if no_index {
                    return Err(CliError::RepeatedOption("--no-index"));
                }
                no_index = true;
            }
            Some("--tangle" | "-t") => match mode {
                None => mode = Some(RunMode::TangleOnly),
                Some(RunMode::TangleOnly) => {
                    return Err(CliError::RepeatedOption("--tangle"));
                }
                Some(RunMode::WeaveOnly | RunMode::Default) => {
                    return Err(CliError::ConflictingOptions);
                }
            },
            Some("--weave" | "-w") => match mode {
                None => mode = Some(RunMode::WeaveOnly),
                Some(RunMode::WeaveOnly) => {
                    return Err(CliError::RepeatedOption("--weave"));
                }
                Some(RunMode::TangleOnly | RunMode::Default) => {
                    return Err(CliError::ConflictingOptions);
                }
            },
            Some("--out-dir" | "-odir") => {
                if output_directory.is_some() {
                    return Err(CliError::RepeatedOption("--out-dir"));
                }
                let directory = args
                    .next()
                    .ok_or(CliError::MissingOptionValue("--out-dir"))?;
                if starts_with_hyphen(&directory) {
                    return Err(CliError::MissingOptionValue("--out-dir"));
                }
                output_directory = Some(directory);
            }
            _ if starts_with_hyphen(&arg) => {
                return Err(CliError::UnsupportedOption(arg));
            }
            _ if input.is_some() => return Err(CliError::MultipleInputs),
            _ => input = Some(PathBuf::from(arg)),
        }
    }

    // Build the run configuration
    let weave_format = weave_format.unwrap_or(WeaveFormat::Html);
    if weave_format == WeaveFormat::Latex {
        if color_scheme.is_some() {
            return Err(CliError::IncompatibleFormatOption("--colorscheme"));
        }
        if no_highlight {
            return Err(CliError::IncompatibleFormatOption("--no-highlight"));
        }
    } else {
        if latex_font_size.is_some() {
            return Err(CliError::FontSizeRequiresLatex);
        }
        if latex_chapter_opening.is_some() {
            return Err(CliError::ChapterOpeningRequiresLatex);
        }
    }

    Ok(Action::Run(RunConfig {
        input: input.ok_or(CliError::MissingInput)?,
        output_directory: output_directory
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
        mode: mode.unwrap_or(RunMode::Default),
        weave_format,
        tangle_headers: !no_headers,
        tangle_block_separators: !no_block_separators,
        identifier_index: !no_index,
        latex_font_size: latex_font_size.unwrap_or_default(),
        latex_chapter_opening: latex_chapter_opening.unwrap_or_default(),
        color_scheme: if no_highlight {
            Some(PathBuf::from("none"))
        } else {
            color_scheme.map(PathBuf::from)
        },
    }))
}
