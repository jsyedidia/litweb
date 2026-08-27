use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use litweb::parser::{Program, parse_str};
use litweb::resolver::{
    ChangeKind, InvalidPathReason, ResolveError, ResolveErrorKind, resolve,
};
use litweb::tangler::{
    ExpansionError, ExpansionErrorKind, PlanError, TangleOptions, TanglePlan,
    TangleWarningKind, plan_tangle, plan_tangle_with_options, write_outputs,
};

const MINIMAL: &str = include_str!("fixtures/tangle/success/minimal/input.lit");
const NESTED: &str = include_str!("fixtures/tangle/success/nested_expansion/input.lit");
const MODIFIERS: &str = include_str!("fixtures/tangle/success/modifiers/input.lit");
const MULTIPLE_ROOTS: &str =
    include_str!("fixtures/tangle/success/multiple_roots/input.lit");
const NO_HEADER: &str = include_str!("fixtures/tangle/success/no_header/input.lit");
const DOCUMENTED_GREETING: &str = include_str!("fixtures/documented/greeting.lit");

type ExpectedOutput<'a> = (&'a str, &'a [u8]);
type SuccessCase<'a> = (&'a str, &'a str, &'a [ExpectedOutput<'a>]);

fn program(file: &str, source: &str) -> Program {
    parse_str(file, source).expect("fixture should parse")
}

fn plan(file: &str, source: &str) -> TanglePlan {
    plan_tangle(&program(file, source)).expect("fixture should tangle")
}

fn plan_with_options(file: &str, source: &str, options: TangleOptions) -> TanglePlan {
    plan_tangle_with_options(&program(file, source), options)
        .expect("fixture should tangle")
}

fn one_resolution_error(file: &str, source: &str) -> ResolveError {
    let error = plan_tangle(&program(file, source)).expect_err("resolution should fail");
    let PlanError::Resolution(errors) = error else {
        panic!("expected resolution error, got {error:?}");
    };
    assert_eq!(errors.as_slice().len(), 1, "{errors:?}");
    errors.into_vec().pop().unwrap()
}

fn one_expansion_error(file: &str, source: &str) -> ExpansionError {
    let error = plan_tangle(&program(file, source)).expect_err("expansion should fail");
    let PlanError::Expansion(errors) = error else {
        panic!("expected expansion error, got {error:?}");
    };
    assert_eq!(errors.as_slice().len(), 1, "{errors:?}");
    errors.into_vec().pop().unwrap()
}

#[test]
fn reviewed_tangle_goldens_match_byte_for_byte_in_source_order() {
    let cases: [SuccessCase<'_>; 5] = [
        (
            "minimal.lit",
            MINIMAL,
            &[(
                "minimal.txt",
                include_bytes!("fixtures/tangle/success/minimal/minimal.txt"),
            )],
        ),
        (
            "nested_expansion.lit",
            NESTED,
            &[(
                "nested.py",
                include_bytes!("fixtures/tangle/success/nested_expansion/nested.py"),
            )],
        ),
        (
            "modifiers.lit",
            MODIFIERS,
            &[(
                "modifiers.txt",
                include_bytes!("fixtures/tangle/success/modifiers/modifiers.txt"),
            )],
        ),
        (
            "multiple_roots.lit",
            MULTIPLE_ROOTS,
            &[
                (
                    "alpha.txt",
                    include_bytes!(
                        "fixtures/tangle/success/multiple_roots/expected/alpha.txt"
                    ),
                ),
                (
                    "nested/beta.txt",
                    include_bytes!(
                        "fixtures/tangle/success/multiple_roots/expected/nested/beta.txt"
                    ),
                ),
                (
                    "Makefile",
                    include_bytes!(
                        "fixtures/tangle/success/multiple_roots/expected/Makefile"
                    ),
                ),
            ],
        ),
        (
            "no_header.lit",
            NO_HEADER,
            &[(
                "clean.rs",
                include_bytes!("fixtures/tangle/success/no_header/clean.rs"),
            )],
        ),
    ];

    for (file, source, expected) in cases {
        let result = plan(file, source);
        assert!(result.warnings.is_empty());
        assert_eq!(result.outputs.len(), expected.len());
        for (output, (path, bytes)) in result.outputs.iter().zip(expected) {
            assert_eq!(output.relative_path, Path::new(path));
            assert_eq!(&output.bytes, bytes, "{file}: {path}");
        }
    }
}

#[test]
fn prose_tables_do_not_change_tangled_output() {
    let source = "\
@s Explanation
Source | Meaning
--- | ---
--- example.txt | literal delimiter
@s Example | literal command

--- output.txt
kept
---
";
    let result = plan("table.lit", source);

    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0].bytes, b"kept\n");
}

#[test]
fn comparison_options_suppress_only_generated_framing() {
    let source = "\
@comment_type // %s
@s Comparison output
--- output.rs
fn main() {
    @{Body}
    @{Ending}
}
---
--- Body
println!(\"hello\");
---
--- Ending
return;
---
";
    let expected_default = concat!(
        "// output.rs\n",
        "fn main() {\n",
        "    // Body\n",
        "    println!(\"hello\");\n",
        "\n",
        "    // Ending\n",
        "    return;\n",
        "}\n",
    );
    let expected_without_headers = concat!(
        "fn main() {\n",
        "    println!(\"hello\");\n",
        "\n",
        "    return;\n",
        "}\n",
    );
    let expected_without_separators = concat!(
        "// output.rs\n",
        "fn main() {\n",
        "    // Body\n",
        "    println!(\"hello\");\n",
        "    // Ending\n",
        "    return;\n",
        "}\n",
    );
    let expected_without_framing = concat!(
        "fn main() {\n",
        "    println!(\"hello\");\n",
        "    return;\n",
        "}\n",
    );

    for (options, expected) in [
        (TangleOptions::default(), expected_default),
        (
            TangleOptions {
                headers: false,
                block_separators: true,
            },
            expected_without_headers,
        ),
        (
            TangleOptions {
                headers: true,
                block_separators: false,
            },
            expected_without_separators,
        ),
        (
            TangleOptions {
                headers: false,
                block_separators: false,
            },
            expected_without_framing,
        ),
    ] {
        let result = plan_with_options("comparison.lit", source, options);
        assert!(result.warnings.is_empty());
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.outputs[0].bytes, expected.as_bytes());
    }

    let authored_source = concat!(
        "@s Authored lines\n",
        "--- output.txt\n",
        "// authored comment\n",
        "    \n",
        "text\n",
        "---\n",
    );
    let authored = plan_with_options(
        "authored.lit",
        authored_source,
        TangleOptions {
            headers: false,
            block_separators: false,
        },
    );
    assert_eq!(
        authored.outputs[0].bytes,
        b"// authored comment\n    \ntext\n"
    );
}

#[test]
fn indented_expansions_leave_empty_lines_empty() {
    let source = concat!(
        "@comment_type // %s\n",
        "@s Empty child lines\n",
        "--- output.rs --- noHeader\n",
        "fn main() {\n",
        "    @{Child}\n",
        "}\n",
        "---\n",
        "--- Child\n",
        "first();\n",
        "\n",
        "  \n",
        "  @{Nested}\n",
        "last();\n",
        "---\n",
        "--- Nested\n",
        "nested_one();\n",
        "\n",
        "nested_two();\n",
        "---\n",
    );
    let with_headers = concat!(
        "fn main() {\n",
        "    // Child\n",
        "    first();\n",
        "\n",
        "      \n",
        "      // Nested\n",
        "      nested_one();\n",
        "\n",
        "      nested_two();\n",
        "    last();\n",
        "}\n",
    );
    let without_headers = concat!(
        "fn main() {\n",
        "    first();\n",
        "\n",
        "      \n",
        "      nested_one();\n",
        "\n",
        "      nested_two();\n",
        "    last();\n",
        "}\n",
    );

    for options in [
        TangleOptions::default(),
        TangleOptions {
            headers: false,
            block_separators: true,
        },
        TangleOptions {
            headers: true,
            block_separators: false,
        },
        TangleOptions {
            headers: false,
            block_separators: false,
        },
    ] {
        let expected = if options.headers {
            with_headers
        } else {
            without_headers
        };
        let result = plan_with_options("empty-lines.lit", source, options);
        assert!(result.warnings.is_empty());
        assert_eq!(result.outputs[0].bytes, expected.as_bytes());
    }
}

#[test]
fn headers_follow_authored_indentation_through_nested_and_empty_blocks() {
    let source = "\
@comment_type // %s
@s Header indentation
--- output.rs --- noHeader
@{Preindented}
    @{Nested}
  @{Empty}
---
--- Preindented

    preindented();
---
--- Nested
  @{Leaf}
---
--- Leaf
leaf();
---
--- Empty
---
";
    let result = plan_with_options(
        "headers.lit",
        source,
        TangleOptions {
            headers: true,
            block_separators: false,
        },
    );

    assert_eq!(
        result.outputs[0].bytes,
        concat!(
            "    // Preindented\n",
            "\n",
            "    preindented();\n",
            "      // Nested\n",
            "      // Leaf\n",
            "      leaf();\n",
            "  // Empty\n",
        )
        .as_bytes()
    );
}

#[test]
fn separators_only_fill_unwritten_boundaries_between_adjacent_references() {
    let source = "\
@s Sibling separators
--- output.txt
@{First}
@{Second}

@{Third}
@{Ends blank}
@{After blank}
tail
@{Final}

---
--- First
first
---
--- Second
second
---
--- Third
third
---
--- Ends blank
ends blank

---
--- After blank
after blank
---
--- Final
final
---
";
    let result = plan("separators.lit", source);

    assert_eq!(
        result.outputs[0].bytes,
        concat!(
            "first\n",
            "\n",
            "second\n",
            "\n",
            "third\n",
            "\n",
            "ends blank\n",
            "\n",
            "after blank\n",
            "tail\n",
            "final\n",
            "\n",
        )
        .as_bytes()
    );
}

#[test]
fn documented_greeting_resolution_applies_changes_in_source_order() {
    let parsed = program("greeting.lit", DOCUMENTED_GREETING);
    let resolved = resolve(&parsed).unwrap();

    assert_eq!(resolved.blocks.len(), 2);
    assert_eq!(resolved.blocks[0].name, "hello.txt");
    assert_eq!(resolved.blocks[0].origin.line, 5);
    assert_eq!(resolved.blocks[0].lines[0].text, "@{Greeting line}");
    assert_eq!(resolved.blocks[1].name, "Greeting line");
    assert_eq!(resolved.blocks[1].origin.line, 11);
    assert_eq!(
        resolved.blocks[1]
            .lines
            .iter()
            .map(|line| (line.text.as_str(), line.origin.line))
            .collect::<Vec<_>>(),
        [("Good morning", 20), ("from Litweb", 24)]
    );
    assert_eq!(resolved.roots.len(), 1);
    assert_eq!(resolved.roots[0].block, 0);
    assert_eq!(resolved.roots[0].path, Path::new("hello.txt"));
    assert_eq!(resolved.roots[0].origin.line, 5);

    let result = plan_tangle(&parsed).unwrap();
    assert!(result.warnings.is_empty());
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0].relative_path, Path::new("hello.txt"));
    assert_eq!(result.outputs[0].origin, resolved.roots[0].origin);
    assert_eq!(result.outputs[0].bytes, b"Good morning\nfrom Litweb\n");
}

#[test]
fn missing_changes_and_incompatible_modifiers_are_source_located() {
    let cases = [
        (
            "missing-addition.lit",
            include_str!("fixtures/tangle/failure/missing_addition.lit"),
            2,
            ResolveErrorKind::MissingChangeTarget {
                name: "Missing".to_owned(),
                change: ChangeKind::Addition,
            },
        ),
        (
            "missing-redefinition.lit",
            include_str!("fixtures/tangle/failure/missing_redefinition.lit"),
            2,
            ResolveErrorKind::MissingChangeTarget {
                name: "Missing".to_owned(),
                change: ChangeKind::Redefinition,
            },
        ),
        (
            "incompatible.lit",
            include_str!("fixtures/tangle/failure/incompatible_modifier.lit"),
            5,
            ResolveErrorKind::IncompatibleModifier {
                name: "Block".to_owned(),
                modifier: "noHeader".to_owned(),
            },
        ),
    ];

    for (file, source, line, kind) in cases {
        let error = one_resolution_error(file, source);
        assert_eq!(error.origin.file, file);
        assert_eq!(error.origin.line, line);
        assert_eq!(error.kind, kind);
    }
}

#[test]
fn every_root_path_invariant_is_checked_before_expansion() {
    let cases = [
        (
            "parent.lit",
            include_str!("fixtures/tangle/failure/parent_path.lit"),
            PathBuf::from("../escape"),
            InvalidPathReason::ParentTraversal,
        ),
        (
            "current.lit",
            include_str!("fixtures/tangle/failure/current_path.lit"),
            PathBuf::from("./escape.txt"),
            InvalidPathReason::CurrentDirectory,
        ),
        (
            "absolute.lit",
            include_str!("fixtures/tangle/failure/absolute_path.lit"),
            PathBuf::from("/escape"),
            InvalidPathReason::Absolute,
        ),
        (
            "windows.lit",
            include_str!("fixtures/tangle/failure/windows_path.lit"),
            PathBuf::from("C:escape"),
            InvalidPathReason::WindowsPrefix,
        ),
        (
            "backslash.lit",
            include_str!("fixtures/tangle/failure/backslash_path.lit"),
            PathBuf::from("directory\\escape.txt"),
            InvalidPathReason::Backslash,
        ),
        (
            "trailing.lit",
            include_str!("fixtures/tangle/failure/trailing_separator.lit"),
            PathBuf::from("directory/"),
            InvalidPathReason::TrailingSeparator,
        ),
    ];

    for (file, source, path, reason) in cases {
        let error = one_resolution_error(file, source);
        assert_eq!(error.origin.line, 2);
        assert_eq!(
            error.kind,
            ResolveErrorKind::InvalidRootPath { path, reason }
        );
    }
}

#[test]
fn normalized_definition_and_output_collisions_are_errors() {
    let definition = one_resolution_error(
        "normalized.lit",
        include_str!("fixtures/tangle/failure/normalized_duplicate.lit"),
    );
    assert_eq!(definition.origin.line, 5);
    assert_eq!(
        definition.kind,
        ResolveErrorKind::DuplicateDefinition {
            name: "same.rs".to_owned()
        }
    );

    let output = one_resolution_error(
        "paths.lit",
        include_str!("fixtures/tangle/failure/duplicate_paths.lit"),
    );
    assert_eq!(output.origin.line, 5);
    assert_eq!(
        output.kind,
        ResolveErrorKind::DuplicateRootPath {
            path: PathBuf::from("nested/same.txt")
        }
    );
}

#[test]
fn undefined_uses_and_cycles_report_the_use_line() {
    let cases = [
        (
            "undefined.lit",
            include_str!("fixtures/tangle/failure/undefined_use.lit"),
            3,
            ExpansionErrorKind::UndefinedBlock {
                name: "Missing".to_owned(),
            },
        ),
        (
            "self-cycle.lit",
            include_str!("fixtures/tangle/failure/self_cycle.lit"),
            6,
            ExpansionErrorKind::ReferenceCycle {
                names: vec!["Loop".to_owned(), "Loop".to_owned()],
            },
        ),
        (
            "longer-cycle.lit",
            include_str!("fixtures/tangle/failure/longer_cycle.lit"),
            9,
            ExpansionErrorKind::ReferenceCycle {
                names: vec!["First".to_owned(), "Second".to_owned(), "First".to_owned()],
            },
        ),
    ];

    for (file, source, line, kind) in cases {
        let error = one_expansion_error(file, source);
        assert_eq!(error.origin.file, file);
        assert_eq!(error.origin.line, line);
        assert_eq!(error.kind, kind);
    }
}

#[test]
fn a_program_without_roots_returns_a_structured_warning() {
    let source = "@s No roots\n--- Named block\ntext\n---\n";
    let result = plan("no-roots.lit", source);

    assert!(result.outputs.is_empty());
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].origin.line, 1);
    assert_eq!(result.warnings[0].kind, TangleWarningKind::NoRoots);
}

#[test]
fn root_inference_uses_the_final_component_but_remains_a_filename_convention() {
    let source = "\
@s Roots
--- ordinary.txt
ordinary
---
--- \"Makefile\"
makefile
---
--- notes.v1/Algorithm
not a root
---
--- Algorithm version 1.2
dotted descriptive name
---
";
    let parsed = program("roots.lit", source);
    let resolved = resolve(&parsed).unwrap();

    assert_eq!(
        resolved
            .roots
            .iter()
            .map(|root| root.path.as_path())
            .collect::<Vec<_>>(),
        [
            Path::new("ordinary.txt"),
            Path::new("Makefile"),
            Path::new("Algorithm version 1.2"),
        ]
    );
}

#[test]
fn only_whole_line_uses_expand_and_tabs_accumulate() {
    let source = "\
@s Uses
--- output.txt
inline @{Piece} stays literal
\t@{Piece}
---
--- Piece
expanded
---
";
    let result = plan("uses.lit", source);

    assert_eq!(
        result.outputs[0].bytes,
        b"inline @{Piece} stays literal\n\texpanded\n"
    );
}

#[test]
fn deep_acyclic_expansion_does_not_use_the_process_stack() {
    let depth = 2_000;
    let mut source = String::from("@s Deep\n--- output.txt\n@{Block 0}\n---\n");
    for index in 0..depth {
        source.push_str(&format!("--- Block {index}\n"));
        if index + 1 == depth {
            source.push_str("end\n");
        } else {
            source.push_str(&format!("@{{Block {}}}\n", index + 1));
        }
        source.push_str("---\n");
    }

    let result = plan("deep.lit", &source);
    assert_eq!(result.outputs[0].bytes, b"end\n");
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "litweb-tangle-{label}-{}-{number}",
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

fn lw(arguments: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lw"))
        .args(arguments)
        .output()
        .expect("lw should run")
}

#[test]
fn long_and_short_tangle_only_cli_runs_are_deterministic() {
    let directory = TestDirectory::new("cli-success");
    let input = directory.path().join("input.lit");
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    fs::write(&input, NESTED).unwrap();

    let first_run = lw(&[
        Path::new("--tangle"),
        Path::new("--out-dir"),
        &first,
        &input,
    ]);
    let second_run = lw(&[Path::new("-t"), Path::new("-odir"), &second, &input]);
    assert!(first_run.status.success(), "{:?}", first_run.stderr);
    assert!(second_run.status.success(), "{:?}", second_run.stderr);
    assert!(first_run.stdout.is_empty() && first_run.stderr.is_empty());
    assert!(second_run.stdout.is_empty() && second_run.stderr.is_empty());
    assert_eq!(
        fs::read(first.join("nested.py")).unwrap(),
        fs::read(second.join("nested.py")).unwrap()
    );
    assert_eq!(
        fs::read(first.join("nested.py")).unwrap(),
        include_bytes!("fixtures/tangle/success/nested_expansion/nested.py")
    );
}

#[test]
fn the_cli_can_omit_generated_tangle_framing() {
    let directory = TestDirectory::new("cli-comparison");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(&input, NESTED).unwrap();

    let result = lw(&[
        Path::new("--tangle"),
        Path::new("--no-headers"),
        Path::new("--no-block-separators"),
        Path::new("--out-dir"),
        &output_directory,
        &input,
    ]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stdout.is_empty() && result.stderr.is_empty());
    assert_eq!(
        fs::read(output_directory.join("nested.py")).unwrap(),
        b"def main():\n    if ready:\n        print(\"nested\")\n"
    );
}

#[test]
fn tangle_comparison_options_are_no_ops_in_weave_only_mode() {
    let directory = TestDirectory::new("cli-weave-only-comparison");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(&input, NESTED).unwrap();

    let result = lw(&[
        Path::new("--weave"),
        Path::new("--no-headers"),
        Path::new("--no-block-separators"),
        Path::new("--out-dir"),
        &output_directory,
        &input,
    ]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stdout.is_empty() && result.stderr.is_empty());
    assert!(output_directory.join("input.html").is_file());
    assert!(!output_directory.join("nested.py").exists());
}

#[test]
fn the_cli_writes_every_root_and_creates_nested_directories() {
    let directory = TestDirectory::new("cli-multiple-roots");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(&input, MULTIPLE_ROOTS).unwrap();

    let result = lw(&[
        Path::new("-t"),
        Path::new("-odir"),
        &output_directory,
        &input,
    ]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stdout.is_empty() && result.stderr.is_empty());
    assert_eq!(
        fs::read(output_directory.join("alpha.txt")).unwrap(),
        include_bytes!("fixtures/tangle/success/multiple_roots/expected/alpha.txt")
    );
    assert_eq!(
        fs::read(output_directory.join("nested/beta.txt")).unwrap(),
        include_bytes!("fixtures/tangle/success/multiple_roots/expected/nested/beta.txt")
    );
    assert_eq!(
        fs::read(output_directory.join("Makefile")).unwrap(),
        include_bytes!("fixtures/tangle/success/multiple_roots/expected/Makefile")
    );
    assert_eq!(fs::read_dir(&output_directory).unwrap().count(), 3);
    assert_eq!(
        fs::read_dir(output_directory.join("nested"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn validation_failure_creates_no_output_directory_or_partial_file() {
    let directory = TestDirectory::new("cli-failure");
    let input = directory.path().join("failure.lit");
    let output_directory = directory.path().join("output");
    fs::write(
        &input,
        include_str!("fixtures/tangle/failure/no_partial_output.lit"),
    )
    .unwrap();

    let result = lw(&[
        Path::new("-t"),
        Path::new("-odir"),
        &output_directory,
        &input,
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains(":6: error: code block {Missing} is not defined\n")
    );
    assert!(!output_directory.exists());
}

#[test]
fn every_resolution_and_expansion_failure_has_no_filesystem_effect() {
    let failures = [
        include_str!("fixtures/tangle/failure/missing_addition.lit"),
        include_str!("fixtures/tangle/failure/missing_redefinition.lit"),
        include_str!("fixtures/tangle/failure/undefined_use.lit"),
        include_str!("fixtures/tangle/failure/self_cycle.lit"),
        include_str!("fixtures/tangle/failure/longer_cycle.lit"),
        include_str!("fixtures/tangle/failure/parent_path.lit"),
        include_str!("fixtures/tangle/failure/current_path.lit"),
        include_str!("fixtures/tangle/failure/absolute_path.lit"),
        include_str!("fixtures/tangle/failure/windows_path.lit"),
        include_str!("fixtures/tangle/failure/backslash_path.lit"),
        include_str!("fixtures/tangle/failure/trailing_separator.lit"),
        include_str!("fixtures/tangle/failure/normalized_duplicate.lit"),
        include_str!("fixtures/tangle/failure/duplicate_paths.lit"),
        include_str!("fixtures/tangle/failure/incompatible_modifier.lit"),
        include_str!("fixtures/tangle/failure/no_partial_output.lit"),
    ];
    let directory = TestDirectory::new("all-failures");

    for (index, source) in failures.into_iter().enumerate() {
        let input = directory.path().join(format!("failure-{index}.lit"));
        let output_directory = directory.path().join(format!("output-{index}"));
        fs::write(&input, source).unwrap();
        let result = lw(&[
            Path::new("-t"),
            Path::new("-odir"),
            &output_directory,
            &input,
        ]);

        assert_eq!(result.status.code(), Some(1), "failure {index}");
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains(": error: ")
        );
        assert!(!output_directory.exists(), "failure {index}");
    }
}

#[test]
fn every_parser_failure_has_no_filesystem_effect() {
    let failures = [
        include_bytes!("fixtures/parser/unclosed.lit").as_slice(),
        include_bytes!("fixtures/parser/code_before_section.lit").as_slice(),
        include_bytes!("fixtures/parser/duplicate.lit").as_slice(),
        include_bytes!("fixtures/parser/invalid_modifier.lit").as_slice(),
        include_bytes!("fixtures/parser/conflicting_modifiers.lit").as_slice(),
        include_bytes!("fixtures/parser/duplicate_modifier.lit").as_slice(),
        include_bytes!("fixtures/parser/unsupported_command.lit").as_slice(),
        include_bytes!("fixtures/parser/unsupported_modifier.lit").as_slice(),
        include_bytes!("fixtures/parser/malformed_quote.lit").as_slice(),
        include_bytes!("fixtures/parser/malformed_delimiter.lit").as_slice(),
        include_bytes!("fixtures/parser/missing_arguments.lit").as_slice(),
        b"@s Invalid UTF-8\n\xff\n".as_slice(),
    ];
    let directory = TestDirectory::new("parser-failures");

    for (index, source) in failures.into_iter().enumerate() {
        let input = directory.path().join(format!("failure-{index}.lit"));
        let output_directory = directory.path().join(format!("output-{index}"));
        fs::write(&input, source).unwrap();
        let result = lw(&[
            Path::new("-t"),
            Path::new("-odir"),
            &output_directory,
            &input,
        ]);

        assert_eq!(result.status.code(), Some(1), "failure {index}");
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains(": error: ")
        );
        assert!(!output_directory.exists(), "failure {index}");
    }
}

#[test]
fn a_no_root_cli_run_warns_without_creating_an_output_directory() {
    let directory = TestDirectory::new("no-roots");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(&input, "@s No roots\n--- Named block\ntext\n---\n").unwrap();

    let result = lw(&[
        Path::new("-t"),
        Path::new("-odir"),
        &output_directory,
        &input,
    ]);

    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    assert_eq!(
        String::from_utf8(result.stderr).unwrap(),
        format!(
            "{}:1: warning: no file code blocks; no code written\n",
            input.display()
        )
    );
    assert!(!output_directory.exists());
}

#[test]
fn existing_files_are_replaced_without_temporary_residue() {
    let directory = TestDirectory::new("atomic-replace");
    let destination = directory.path().join("minimal.txt");
    fs::write(&destination, "old contents").unwrap();
    let result = plan("input.lit", MINIMAL);

    write_outputs(
        &result,
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap();
    assert_eq!(
        fs::read(&destination).unwrap(),
        include_bytes!("fixtures/tangle/success/minimal/minimal.txt")
    );
    assert_eq!(
        fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec!["minimal.txt"]
    );
}
