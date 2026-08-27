// src/main.rs
// Main imports
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use litweb::book::{BookErrors, is_book_manifest, load_book_bytes};
use litweb::config::{Action, HELP_TEXT, RunConfig, RunMode, WeaveFormat, parse_args};
use litweb::latex::{LatexOptions, LatexPlanError, plan_latex_with_options};
use litweb::output::{OutputError, PlannedOutput, write_planned_outputs_protecting};
use litweb::parser::{ParseErrors, parse_bytes};
use litweb::tangler::{
    PlanError, TangleOptions, TangleWarning, plan_tangle_with_options,
};
use litweb::weaver::{WeaveOptions, WeavePlanError, plan_weave_with_options};

// Main function
fn main() -> ExitCode {
    match parse_args(env::args_os().skip(1)) {
        Ok(Action::Help) => {
            print!("{HELP_TEXT}");
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            println!("Litweb version {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Action::Run(config)) => match run_litweb(config) {
            Ok(warnings) => {
                for warning in warnings {
                    eprintln!("{warning}");
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                error.report();
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

// Operational run errors
enum RunError {
    Read { path: PathBuf, error: io::ErrorKind },
    Parse(ParseErrors),
    Book(BookErrors),
    Tangle(PlanError),
    Weave(WeavePlanError),
    Latex(LatexPlanError),
    Output(OutputError),
}

// Report an operational error
impl RunError {
    fn report(&self) {
        match self {
            Self::Read { path, error } => {
                eprintln!("{}: error: cannot read input: {error}", path.display());
            }
            Self::Parse(errors) => {
                for error in errors.as_slice() {
                    eprintln!("{error}");
                }
            }
            Self::Book(errors) => {
                for error in errors.as_slice() {
                    eprintln!("{error}");
                }
            }
            Self::Tangle(PlanError::Resolution(errors))
            | Self::Weave(WeavePlanError::Resolution(errors))
            | Self::Latex(LatexPlanError::Resolution(errors)) => {
                for error in errors.as_slice() {
                    eprintln!("{error}");
                }
            }
            Self::Tangle(PlanError::Expansion(errors)) => {
                for error in errors.as_slice() {
                    eprintln!("{error}");
                }
            }
            Self::Weave(WeavePlanError::Weave(errors)) => {
                for error in errors.as_slice() {
                    eprintln!("{error}");
                }
            }
            Self::Latex(LatexPlanError::Latex(errors)) => {
                for error in errors.as_slice() {
                    eprintln!("{error}");
                }
            }
            Self::Output(error) => eprintln!("{error}"),
        }
    }
}

// Run Litweb
fn run_litweb(config: RunConfig) -> Result<Vec<TangleWarning>, RunError> {
    // Load one source or a book
    let source = fs::read(&config.input).map_err(|error| RunError::Read {
        path: config.input.clone(),
        error: error.kind(),
    })?;
    let program = if is_book_manifest(&source) {
        load_book_bytes(&config.input, &source).map_err(RunError::Book)?
    } else {
        parse_bytes(config.input.to_string_lossy(), &source).map_err(RunError::Parse)?
    };

    // Plan the requested outputs
    let mut outputs = Vec::<PlannedOutput>::new();
    let mut warnings = Vec::new();

    if config.mode != RunMode::WeaveOnly {
        let plan = plan_tangle_with_options(
            &program,
            TangleOptions {
                headers: config.tangle_headers,
                block_separators: config.tangle_block_separators,
            },
        )
        .map_err(RunError::Tangle)?;
        outputs.extend(plan.outputs);
        warnings = plan.warnings;
    }
    if config.mode != RunMode::TangleOnly {
        match config.weave_format {
            WeaveFormat::Html => {
                let plan = plan_weave_with_options(
                    &program,
                    WeaveOptions {
                        identifier_index: config.identifier_index,
                        color_scheme: config.color_scheme.clone(),
                    },
                )
                .map_err(RunError::Weave)?;
                outputs.extend(plan.outputs);
            }
            WeaveFormat::Latex => {
                let plan = plan_latex_with_options(
                    &program,
                    LatexOptions {
                        identifier_index: config.identifier_index,
                        font_size: config.latex_font_size,
                        chapter_opening: config.latex_chapter_opening,
                    },
                )
                .map_err(RunError::Latex)?;
                outputs.extend(plan.outputs);
            }
        }
    }

    // Protect inputs and write outputs
    let mut inputs = vec![config.input.clone()];
    if program.is_book() {
        let directory = config
            .input
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        inputs.extend(program.chapters.iter().map(|chapter| {
            directory.join(
                &chapter
                    .book
                    .as_ref()
                    .expect("a loaded book chapter has book metadata")
                    .source_path,
            )
        }));
    }
    write_planned_outputs_protecting(&outputs, &config.output_directory, &inputs)
        .map_err(RunError::Output)?;
    Ok(warnings)
}
