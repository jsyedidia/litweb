use std::ffi::OsString;
use std::path::PathBuf;

use litweb::config::{Action, CliError, RunConfig, RunMode, WeaveFormat, parse_args};
use litweb::latex::{LatexChapterOpening, LatexFontSize};

fn strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(|value| OsString::from(*value)).collect()
}

fn run_config(arguments: Vec<OsString>) -> RunConfig {
    match parse_args(arguments).unwrap() {
        Action::Run(config) => config,
        action => panic!("expected a run action, got {action:?}"),
    }
}

#[test]
fn documented_run_configurations_are_exact() {
    assert_eq!(
        run_config(strings(&["greeting.lit"])),
        RunConfig {
            input: PathBuf::from("greeting.lit"),
            output_directory: PathBuf::from("."),
            mode: RunMode::Default,
            weave_format: WeaveFormat::Html,
            tangle_headers: true,
            tangle_block_separators: true,
            identifier_index: true,
            latex_font_size: LatexFontSize::TwelvePoint,
            latex_chapter_opening: LatexChapterOpening::Right,
            color_scheme: None,
        }
    );
    assert_eq!(
        run_config(strings(&["-t", "-odir", "generated", "greeting.lit"])),
        RunConfig {
            input: PathBuf::from("greeting.lit"),
            output_directory: PathBuf::from("generated"),
            mode: RunMode::TangleOnly,
            weave_format: WeaveFormat::Html,
            tangle_headers: true,
            tangle_block_separators: true,
            identifier_index: true,
            latex_font_size: LatexFontSize::TwelvePoint,
            latex_chapter_opening: LatexChapterOpening::Right,
            color_scheme: None,
        }
    );
    assert_eq!(
        run_config(strings(&["-w", "--no-highlight", "greeting.lit"])),
        RunConfig {
            input: PathBuf::from("greeting.lit"),
            output_directory: PathBuf::from("."),
            mode: RunMode::WeaveOnly,
            weave_format: WeaveFormat::Html,
            tangle_headers: true,
            tangle_block_separators: true,
            identifier_index: true,
            latex_font_size: LatexFontSize::TwelvePoint,
            latex_chapter_opening: LatexChapterOpening::Right,
            color_scheme: Some(PathBuf::from("none")),
        }
    );

    let config = run_config(strings(&["--colorscheme", "dark", "greeting.lit"]));
    assert_eq!(config.color_scheme, Some(PathBuf::from("dark")));

    for (argument, expected) in [
        ("10pt", LatexFontSize::TenPoint),
        ("11pt", LatexFontSize::ElevenPoint),
        ("12pt", LatexFontSize::TwelvePoint),
    ] {
        let config = run_config(strings(&[
            "--format",
            "latex",
            "--font-size",
            argument,
            "greeting.lit",
        ]));
        assert_eq!(config.weave_format, WeaveFormat::Latex);
        assert_eq!(config.latex_font_size, expected);
    }

    for (argument, expected) in [
        ("right", LatexChapterOpening::Right),
        ("left", LatexChapterOpening::Left),
    ] {
        let config = run_config(strings(&[
            "--format",
            "latex",
            "--chapter-opening",
            argument,
            "greeting.lit",
        ]));
        assert_eq!(config.weave_format, WeaveFormat::Latex);
        assert_eq!(config.latex_chapter_opening, expected);
    }
}

#[test]
fn a_second_positional_input_is_rejected() {
    assert_eq!(
        parse_args(strings(&["first.lit", "second.lit"])),
        Err(CliError::MultipleInputs)
    );
}

#[test]
fn end_of_options_makes_later_arguments_positional() {
    let config = run_config(strings(&["-t", "--", "-draft.lit"]));
    assert_eq!(config.input, PathBuf::from("-draft.lit"));
    assert_eq!(config.mode, RunMode::TangleOnly);

    let config = run_config(strings(&["--", "--weave"]));
    assert_eq!(config.input, PathBuf::from("--weave"));
    assert_eq!(config.mode, RunMode::Default);

    let config = run_config(strings(&["input.lit", "--"]));
    assert_eq!(config.input, PathBuf::from("input.lit"));
}

#[test]
fn end_of_options_preserves_value_and_input_requirements() {
    assert_eq!(parse_args(strings(&["--"])), Err(CliError::MissingInput));
    assert_eq!(
        parse_args(strings(&["input.lit", "--", "--weave"])),
        Err(CliError::MultipleInputs)
    );

    for option in [
        "--out-dir",
        "--chapter-opening",
        "--colorscheme",
        "--font-size",
        "--format",
    ] {
        assert_eq!(
            parse_args(strings(&[option, "--", "input.lit"])),
            Err(CliError::MissingOptionValue(option))
        );
    }
}

#[cfg(unix)]
fn non_utf8(first: u8) -> OsString {
    use std::os::unix::ffi::OsStringExt;

    OsString::from_vec(vec![first, 0xff])
}

#[cfg(windows)]
fn non_utf8(first: u8) -> OsString {
    use std::os::windows::ffi::OsStringExt;

    OsString::from_wide(&[u16::from(first), 0xd800])
}

#[cfg(any(unix, windows))]
#[test]
fn non_utf8_option_shape_is_independent_of_full_decoding() {
    let dash_prefixed = non_utf8(b'-');
    assert_eq!(
        parse_args([dash_prefixed.clone()]),
        Err(CliError::UnsupportedOption(dash_prefixed.clone()))
    );

    let config = run_config(vec![OsString::from("--"), dash_prefixed.clone()]);
    assert_eq!(config.input, PathBuf::from(dash_prefixed));

    for option in [
        "--out-dir",
        "--chapter-opening",
        "--colorscheme",
        "--font-size",
        "--format",
    ] {
        assert_eq!(
            parse_args([OsString::from(option), non_utf8(b'-')]),
            Err(CliError::MissingOptionValue(option))
        );
    }

    let format = non_utf8(b'f');
    assert_eq!(
        parse_args([
            OsString::from("--format"),
            format.clone(),
            OsString::from("input.lit"),
        ]),
        Err(CliError::UnknownFormat(format))
    );

    let opening = non_utf8(b'r');
    assert_eq!(
        parse_args([
            OsString::from("--format"),
            OsString::from("latex"),
            OsString::from("--chapter-opening"),
            opening.clone(),
            OsString::from("input.lit"),
        ]),
        Err(CliError::UnknownChapterOpening(opening))
    );
}

#[test]
fn format_values_and_incompatible_options_are_structured() {
    assert_eq!(
        parse_args(strings(&["--format", "pdf", "input.lit"])),
        Err(CliError::UnknownFormat(OsString::from("pdf")))
    );
    assert_eq!(
        parse_args(strings(&[
            "--format",
            "html",
            "--format",
            "latex",
            "input.lit",
        ])),
        Err(CliError::RepeatedOption("--format"))
    );
    assert_eq!(
        parse_args(strings(&[
            "--format",
            "latex",
            "--font-size",
            "13pt",
            "input.lit",
        ])),
        Err(CliError::UnknownFontSize(OsString::from("13pt")))
    );
    assert_eq!(
        parse_args(strings(&[
            "--format",
            "latex",
            "--font-size",
            "11pt",
            "--font-size",
            "12pt",
            "input.lit",
        ])),
        Err(CliError::RepeatedOption("--font-size"))
    );
    assert_eq!(
        parse_args(strings(&["--font-size", "11pt", "input.lit"])),
        Err(CliError::FontSizeRequiresLatex)
    );
    assert_eq!(
        parse_args(strings(&[
            "--format",
            "latex",
            "--chapter-opening",
            "middle",
            "input.lit",
        ])),
        Err(CliError::UnknownChapterOpening(OsString::from("middle")))
    );
    assert_eq!(
        parse_args(strings(&[
            "--format",
            "latex",
            "--chapter-opening",
            "left",
            "--chapter-opening",
            "right",
            "input.lit",
        ])),
        Err(CliError::RepeatedOption("--chapter-opening"))
    );
    assert_eq!(
        parse_args(strings(&["--chapter-opening", "left", "input.lit"])),
        Err(CliError::ChapterOpeningRequiresLatex)
    );
    for option in ["--colorscheme", "--no-highlight"] {
        let arguments = if option == "--colorscheme" {
            strings(&["--format", "latex", option, "dark", "input.lit"])
        } else {
            strings(&["--format", "latex", option, "input.lit"])
        };
        assert_eq!(
            parse_args(arguments),
            Err(CliError::IncompatibleFormatOption(option))
        );
    }
}

#[cfg(any(unix, windows))]
#[test]
fn non_option_non_utf8_values_are_preserved() {
    let input = non_utf8(b'i');
    let config = run_config(vec![input.clone()]);
    assert_eq!(config.input, PathBuf::from(input));

    let output_directory = non_utf8(b'o');
    let config = run_config(vec![
        OsString::from("--out-dir"),
        output_directory.clone(),
        OsString::from("input.lit"),
    ]);
    assert_eq!(config.output_directory, PathBuf::from(output_directory));

    let color_scheme = non_utf8(b'c');
    let config = run_config(vec![
        OsString::from("--colorscheme"),
        color_scheme.clone(),
        OsString::from("input.lit"),
    ]);
    assert_eq!(config.color_scheme, Some(PathBuf::from(color_scheme)));
}
