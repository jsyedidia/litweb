use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const DOCUMENTED_GREETING: &[u8] = include_bytes!("fixtures/documented/greeting.lit");

const HELP: &str = "\
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

fn lw(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lw"))
        .args(args)
        .output()
        .expect("lw should run")
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "litweb-cli-{label}-{}-{number}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_is_available_in_long_and_short_forms() {
    for option in ["--help", "-h"] {
        let output = lw(&[option]);
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), HELP);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn version_is_available_in_long_and_short_forms() {
    for option in ["--version", "-v"] {
        let output = lw(&[option]);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "Litweb version 0.9.1\n"
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn documented_greeting_command_has_the_exact_visible_result() {
    let directory = TestDirectory::new("documented-greeting");
    let input = directory.path().join("greeting.lit");
    fs::write(&input, DOCUMENTED_GREETING).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_lw"))
        .current_dir(directory.path())
        .args(["-t", "-odir", "generated", "greeting.lit"])
        .output()
        .expect("lw should run");

    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(&input).unwrap(), DOCUMENTED_GREETING);

    let generated = directory.path().join("generated");
    assert_eq!(
        fs::read(generated.join("hello.txt")).unwrap(),
        b"Good morning\nfrom Litweb\n"
    );
    assert_eq!(fs::read_dir(&generated).unwrap().count(), 1);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn a_missing_input_is_an_error() {
    let output = lw(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: no input file provided\n"
    );
}

#[test]
fn an_unsupported_option_is_an_error() {
    let output = lw(&["--unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: unsupported option: --unknown\n"
    );
}

#[test]
fn an_output_directory_value_is_required() {
    for arguments in [&["--out-dir"][..], &["--out-dir", "-t", "input.lit"][..]] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: option --out-dir requires a value\n"
        );
    }
}

#[test]
fn options_cannot_be_repeated() {
    for (arguments, message) in [
        (
            &["-t", "--tangle", "input.lit"][..],
            "error: option --tangle was provided more than once\n",
        ),
        (
            &["-odir", "one", "--out-dir", "two", "input.lit"][..],
            "error: option --out-dir was provided more than once\n",
        ),
        (
            &["-w", "--weave", "input.lit"][..],
            "error: option --weave was provided more than once\n",
        ),
        (
            &["--format", "html", "--format", "latex", "input.lit"][..],
            "error: option --format was provided more than once\n",
        ),
        (
            &[
                "--format",
                "latex",
                "--font-size",
                "11pt",
                "--font-size",
                "12pt",
                "input.lit",
            ][..],
            "error: option --font-size was provided more than once\n",
        ),
        (
            &[
                "--format",
                "latex",
                "--chapter-opening",
                "left",
                "--chapter-opening",
                "right",
                "input.lit",
            ][..],
            "error: option --chapter-opening was provided more than once\n",
        ),
        (
            &["--no-index", "--no-index", "input.lit"][..],
            "error: option --no-index was provided more than once\n",
        ),
        (
            &[
                "--colorscheme",
                "litweb",
                "--colorscheme",
                "dark",
                "input.lit",
            ][..],
            "error: option --colorscheme was provided more than once\n",
        ),
        (
            &["--no-highlight", "--no-highlight", "input.lit"][..],
            "error: option --no-highlight was provided more than once\n",
        ),
        (
            &["--no-headers", "--no-headers", "input.lit"][..],
            "error: option --no-headers was provided more than once\n",
        ),
        (
            &[
                "--no-block-separators",
                "--no-block-separators",
                "input.lit",
            ][..],
            "error: option --no-block-separators was provided more than once\n",
        ),
    ] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(String::from_utf8(output.stderr).unwrap(), message);
    }
}

#[test]
fn colorscheme_value_is_required_and_highlight_options_conflict() {
    for arguments in [
        &["--colorscheme"][..],
        &["--colorscheme", "--weave", "input.lit"][..],
    ] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: option --colorscheme requires a value\n"
        );
    }

    for arguments in [
        &["--colorscheme", "litweb", "--no-highlight", "input.lit"][..],
        &["--no-highlight", "--colorscheme", "litweb", "input.lit"][..],
    ] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: options --colorscheme and --no-highlight cannot be used together\n"
        );
    }
}

#[test]
fn format_value_is_required_known_and_compatible() {
    for arguments in [&["--format"][..], &["--format", "--weave", "input.lit"][..]] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: option --format requires a value\n"
        );
    }

    let output = lw(&["--format", "pdf", "input.lit"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: unknown woven format pdf; expected html or latex\n"
    );

    for (arguments, option) in [
        (
            &["--format", "latex", "--colorscheme", "dark", "input.lit"][..],
            "--colorscheme",
        ),
        (
            &["--no-highlight", "--format", "latex", "input.lit"][..],
            "--no-highlight",
        ),
    ] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("error: option {option} cannot be used with --format latex\n")
        );
    }

    for arguments in [
        &["--format", "latex", "--font-size"][..],
        &["--format", "latex", "--font-size", "--weave", "input.lit"][..],
    ] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: option --font-size requires a value\n"
        );
    }

    let output = lw(&["--format", "latex", "--font-size", "13pt", "input.lit"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: unknown LaTeX font size 13pt; expected 10pt, 11pt, or 12pt\n"
    );

    let output = lw(&["--font-size", "11pt", "input.lit"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: option --font-size requires --format latex\n"
    );

    for arguments in [
        &["--format", "latex", "--chapter-opening"][..],
        &[
            "--format",
            "latex",
            "--chapter-opening",
            "--weave",
            "input.lit",
        ][..],
    ] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: option --chapter-opening requires a value\n"
        );
    }

    let output = lw(&[
        "--format",
        "latex",
        "--chapter-opening",
        "middle",
        "input.lit",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: unknown LaTeX chapter opening middle; expected right or left\n"
    );

    let output = lw(&["--chapter-opening", "left", "input.lit"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: option --chapter-opening requires --format latex\n"
    );
}

#[test]
fn tangle_and_weave_options_conflict() {
    for arguments in [
        &["-t", "-w", "input.lit"][..],
        &["--weave", "--tangle", "input.lit"][..],
    ] {
        let output = lw(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: options --tangle and --weave cannot be used together\n"
        );
    }
}
