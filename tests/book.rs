use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use litweb::book::{
    BookErrorKind, is_book_manifest, load_book, parse_book_manifest_bytes,
    parse_book_manifest_str,
};
use litweb::identifier::analyze;
use litweb::output::PlannedOutput;
use litweb::parser::{
    BlockKind, BookChapter, BookMetadata, CommandKind, ProgramKind, SourceOrigin,
    parse_str,
};
use litweb::resolver::{ChangeKind, InvalidPathReason, ResolveErrorKind, resolve};
use litweb::tangler::{ExpansionErrorKind, PlanError, plan_tangle};
use litweb::weaver::{
    WeaveErrorKind, WeaveOptions, WeavePlan, WeavePlanError, plan_weave,
    plan_weave_with_options,
};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/book")
        .join(path)
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "litweb-book-{label}-{}-{number}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_book(directory: &TestDirectory, manifest: &str, chapters: &[(&str, &str)]) {
    directory.write("index.lit", manifest);
    for (path, source) in chapters {
        directory.write(path, source);
    }
}

fn lw(arguments: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lw"))
        .args(arguments)
        .output()
        .expect("lw should run")
}

fn plan_weave_without_index(program: &litweb::parser::Program) -> WeavePlan {
    plan_weave_with_options(
        program,
        WeaveOptions {
            identifier_index: false,
            color_scheme: Some(PathBuf::from("none")),
        },
    )
    .unwrap()
}

fn html_outputs(plan: &WeavePlan) -> Vec<&PlannedOutput> {
    plan.outputs
        .iter()
        .filter(|output| {
            output
                .relative_path
                .extension()
                .is_some_and(|ext| ext == "html")
        })
        .collect()
}

#[test]
fn manifest_parser_preserves_exact_grammar_defaults_and_order() {
    let source = "\
@book\n\
@title The Book\n\
@code_type rust .rs\n\
@code_type none\n\
@code_type rust .rs\n\
@colorscheme twilight\n\
Intro with [an embedded chapter](chapters/embedded.lit).\n\
[A guide](guide.html)\n\
@titleish remains prose\n\
[First chapter](chapters/first.lit)\n\
\t[First appendix](chapters/appendix.lit)\n\
[Second chapter](chapters/second.lit)\n\
@comment_type none\n";
    let manifest = parse_book_manifest_str("books/index.lit", source).unwrap();

    assert_eq!(manifest.title, "The Book");
    assert_eq!(manifest.title_origin.line, 2);
    assert_eq!(manifest.commands.len(), 5);
    assert_eq!(manifest.commands[0].kind, CommandKind::CodeType);
    assert_eq!(manifest.commands[0].arguments, "rust .rs");
    assert_eq!(manifest.commands[1].arguments, "");
    assert_eq!(manifest.commands[2].arguments, "rust .rs");
    assert_eq!(manifest.commands[3].kind, CommandKind::ColorScheme);
    assert_eq!(manifest.commands[3].arguments, "twilight");
    assert_eq!(manifest.commands[4].kind, CommandKind::CommentType);
    assert_eq!(manifest.commands[4].arguments, "");
    assert!(manifest.introduction.contains("embedded chapter"));
    assert!(manifest.introduction.contains("[A guide](guide.html)"));
    assert!(manifest.introduction.contains("@titleish remains prose"));
    assert_eq!(
        manifest
            .chapters
            .iter()
            .map(|chapter| (
                chapter.navigation_label.as_str(),
                chapter.source_path.as_path(),
                chapter.major_number,
                chapter.minor_number,
            ))
            .collect::<Vec<_>>(),
        vec![
            ("First chapter", Path::new("chapters/first.lit"), 1, 0,),
            ("First appendix", Path::new("chapters/appendix.lit"), 1, 1,),
            ("Second chapter", Path::new("chapters/second.lit"), 2, 0,),
        ]
    );
}

#[test]
fn greeting_book_example_produces_the_documented_program() {
    let directory = TestDirectory::new("greeting-example");
    directory.write(
        "greetings.lit",
        r#"@book
@title Greetings
@code_type text .txt
@comment_type none

This book contains three short greetings.

[English](english.lit)
    [A friendlier greeting](friendly.lit)
[Spanish](spanish.lit)
"#,
    );
    directory.write(
        "english.lit",
        "@title An English Greeting\n@s Greeting\n--- english.txt\nHello\n---\n",
    );
    directory.write(
        "friendly.lit",
        "@title A Friendly Greeting\n@s Greeting\n--- friendly.txt\nHello, friend!\n---\n",
    );
    directory.write(
        "spanish.lit",
        "@title Un saludo\n@s Greeting\n--- spanish.txt\n¡Hola!\n---\n",
    );

    let program = load_book(directory.path("greetings.lit")).unwrap();
    let book = program.book().unwrap();

    assert_eq!(program.title, "Greetings");
    assert!(matches!(&program.kind, ProgramKind::Book(_)));
    assert!(
        book.introduction
            .contains("This book contains three short greetings.")
    );
    assert_eq!(
        book.introduction_lines
            .iter()
            .find(|line| !line.text.is_empty())
            .unwrap()
            .origin
            .line,
        6
    );
    assert_eq!(program.commands.len(), 2);
    assert_eq!(program.commands[0].kind, CommandKind::CodeType);
    assert_eq!(program.commands[0].arguments, "text .txt");
    assert_eq!(program.commands[1].kind, CommandKind::CommentType);
    assert_eq!(program.commands[1].arguments, "");

    assert_eq!(program.chapters.len(), 3);
    assert_eq!(
        program
            .chapters
            .iter()
            .map(|chapter| (
                chapter.number(),
                chapter.book.as_ref().unwrap().navigation_label.as_str(),
                chapter.book.as_ref().unwrap().source_path.as_path(),
            ))
            .collect::<Vec<_>>(),
        vec![
            ("1".to_owned(), "English", Path::new("english.lit"),),
            (
                "1.1".to_owned(),
                "A friendlier greeting",
                Path::new("friendly.lit"),
            ),
            ("2".to_owned(), "Spanish", Path::new("spanish.lit"),),
        ]
    );

    let english = &program.chapters[0];
    assert_eq!(english.title, "An English Greeting");
    assert_eq!(english.book.as_ref().unwrap().label_origin.line, 8);
    let output = english.sections[0]
        .blocks
        .iter()
        .find_map(|block| block.code())
        .unwrap();
    assert_eq!(output.name, "english.txt");
    assert_eq!(output.code_type, "text .txt");
    assert_eq!(output.comment_string, "");

    let plan = plan_weave_without_index(&program);
    assert_eq!(
        plan.outputs
            .iter()
            .map(|output| output.relative_path.as_path())
            .collect::<Vec<_>>(),
        vec![
            Path::new("greetings.html"),
            Path::new("english.html"),
            Path::new("friendly.html"),
            Path::new("spanish.html"),
        ]
    );
    let html = |path: &str| {
        let output = plan
            .outputs
            .iter()
            .find(|output| output.relative_path == Path::new(path))
            .unwrap();
        String::from_utf8(output.bytes.clone()).unwrap()
    };

    let contents = html("greetings.html");
    assert!(contents.contains("<h1>Greetings</h1>"));
    assert!(contents.contains("This book contains three short greetings."));
    assert!(contents.contains("<a href=\"english.html\">1. English</a>"));
    assert!(
        contents.contains("<a href=\"friendly.html\">1.1. A friendlier greeting</a>")
    );
    assert!(contents.contains("<a href=\"spanish.html\">2. Spanish</a>"));

    let english = html("english.html");
    assert!(english.contains("<title>An English Greeting — Greetings</title>"));
    assert!(english.contains(
        "<h1><span class=\"chapter-number\">1.</span> An English Greeting</h1>"
    ));
    assert!(!english.contains("Previous:"));
    assert!(english.contains("Next: A friendlier greeting"));

    let friendly = html("friendly.html");
    assert!(friendly.contains("<title>A Friendly Greeting — Greetings</title>"));
    assert!(friendly.contains(
        "<h1><span class=\"chapter-number\">1.1.</span> A Friendly Greeting</h1>"
    ));
    assert!(friendly.contains("Previous: English"));
    assert!(friendly.contains("Next: Spanish"));

    let spanish = html("spanish.html");
    assert!(spanish.contains("<title>Un saludo — Greetings</title>"));
    assert!(
        spanish.contains("<h1><span class=\"chapter-number\">2.</span> Un saludo</h1>")
    );
    assert!(spanish.contains("Previous: A friendlier greeting"));
    assert!(!spanish.contains("Next:"));

    for output in &plan.outputs {
        let page = String::from_utf8(output.bytes.clone()).unwrap();
        assert!(!page.contains("Identifier Index"));
    }
}

#[test]
fn manifest_fences_keep_commands_and_chapter_links_in_introduction_prose() {
    let source = r#"@book
@title Fenced Manifest
```text
@title Example Title
[Example chapter](example.lit)
```
[Real chapter](real.lit)
"#;
    let manifest = parse_book_manifest_str("index.lit", source).unwrap();

    assert_eq!(manifest.title, "Fenced Manifest");
    assert_eq!(manifest.chapters.len(), 1);
    assert_eq!(manifest.chapters[0].source_path, Path::new("real.lit"));
    assert!(manifest.introduction.contains("@title Example Title"));
    assert!(
        manifest
            .introduction
            .contains("[Example chapter](example.lit)")
    );
    assert_eq!(
        manifest
            .introduction_lines
            .iter()
            .map(|line| line.origin.line)
            .collect::<Vec<_>>(),
        vec![3, 4, 5, 6, 8]
    );
}

#[test]
fn manifest_tables_keep_commands_and_chapter_links_in_introduction_prose() {
    let source = r#"@book
@title Table Manifest
Example | Meaning
--- | ---
@title Example Title | literal source
[Example chapter](example.lit) | literal source

[Real chapter](real.lit)
"#;
    let manifest = parse_book_manifest_str("index.lit", source).unwrap();

    assert_eq!(manifest.title, "Table Manifest");
    assert_eq!(manifest.chapters.len(), 1);
    assert_eq!(manifest.chapters[0].source_path, Path::new("real.lit"));
    assert!(manifest.introduction.contains("@title Example Title"));
    assert!(
        manifest
            .introduction
            .contains("[Example chapter](example.lit)")
    );
}

#[test]
fn book_classification_requires_an_exact_standalone_marker() {
    assert!(is_book_manifest(b"prose\n@book\t \r\n"));
    assert!(!is_book_manifest(b"```text\n@book\n```\n"));
    assert!(is_book_manifest(b"```text\n@book\n"));
    assert!(!is_book_manifest(b" @book\n"));
    assert!(!is_book_manifest(b"@book argument\n"));
    assert!(!is_book_manifest(b"@bookish\n"));
}

#[test]
fn malformed_manifest_diagnostics_follow_source_order() {
    let source = "\
@book extra\n\
@book\n\
@title\n\
@title Again\n\
\t[Minor](minor.lit)\n\
[ ]()\n\
[Broken](missing.lit\n\
@s Not allowed\n";
    let errors = parse_book_manifest_str("index.lit", source).unwrap_err();
    let actual = errors
        .as_slice()
        .iter()
        .map(|error| (error.origin.line, &error.kind))
        .collect::<Vec<_>>();

    assert_eq!(actual.len(), 9, "{actual:?}");
    assert!(matches!(actual[0], (1, BookErrorKind::BookMarkerArguments)));
    assert!(matches!(actual[1], (2, BookErrorKind::DuplicateBookMarker)));
    assert!(matches!(actual[2], (3, BookErrorKind::EmptyTitle)));
    assert!(matches!(actual[3], (4, BookErrorKind::DuplicateTitle)));
    assert!(matches!(actual[4], (5, BookErrorKind::MinorBeforeMajor)));
    assert!(matches!(actual[5], (6, BookErrorKind::EmptyChapterLabel)));
    assert!(matches!(actual[6], (6, BookErrorKind::EmptyChapterPath)));
    assert!(matches!(
        actual[7],
        (7, BookErrorKind::MalformedChapterEntry { .. })
    ));
    assert!(matches!(
        actual[8],
        (8, BookErrorKind::UnsupportedCommand { .. })
    ));

    let missing =
        parse_book_manifest_str("index.lit", "Introduction only\n").unwrap_err();
    assert!(matches!(
        missing.as_slice()[0].kind,
        BookErrorKind::MissingBookMarker
    ));
    assert!(matches!(
        missing.as_slice()[1].kind,
        BookErrorKind::MissingTitle
    ));
    assert!(matches!(
        missing.as_slice()[2].kind,
        BookErrorKind::NoChapters
    ));
}

#[test]
fn manifest_utf8_and_portable_path_failures_are_structured() {
    let invalid =
        parse_book_manifest_bytes("index.lit", b"@book\n@title B\n\xff").unwrap_err();
    assert_eq!(invalid.as_slice()[0].origin.line, 3);
    assert!(matches!(
        invalid.as_slice()[0].kind,
        BookErrorKind::InvalidUtf8 { valid_up_to: 15 }
    ));

    let cases = [
        ("/absolute.lit", InvalidPathReason::Absolute),
        ("C:drive.lit", InvalidPathReason::WindowsPrefix),
        ("./current.lit", InvalidPathReason::CurrentDirectory),
        ("../parent.lit", InvalidPathReason::ParentTraversal),
        ("nested\\chapter.lit", InvalidPathReason::Backslash),
        ("nested/chapter.lit/", InvalidPathReason::TrailingSeparator),
    ];
    for (target, reason) in cases {
        let source = format!("@book\n@title Paths\n[Chapter]({target})\n");
        let errors = parse_book_manifest_str("index.lit", &source).unwrap_err();
        assert!(
            errors.as_slice().iter().any(|error| {
                matches!(
                    &error.kind,
                    BookErrorKind::InvalidChapterPath {
                        reason: actual,
                        ..
                    } if *actual == reason
                )
            }),
            "{target}: {errors:?}"
        );
    }

    let duplicate = parse_book_manifest_str(
        "index.lit",
        "@book\n@title Duplicate\n[One](same.lit)\n[Two](same.lit)\n",
    )
    .unwrap_err();
    assert!(matches!(
        duplicate.as_slice()[0].kind,
        BookErrorKind::DuplicateChapterPath { .. }
    ));
    let collision = parse_book_manifest_str(
        "index.lit",
        "@book\n@title Collision\n[Contents](index.lit)\n",
    )
    .unwrap();
    assert_eq!(collision.chapters[0].source_path, Path::new("index.lit"));
    let invalid_manifest =
        parse_book_manifest_str("book.txt", "@book\n@title Extension\n[One](one.lit)\n")
            .unwrap_err();
    assert!(matches!(
        invalid_manifest.as_slice()[0].kind,
        BookErrorKind::InvalidManifestPath { .. }
    ));
}

#[test]
fn loader_uses_manifest_relative_paths_and_keeps_book_metadata() {
    let program = load_book(fixture("rust_shape/index.lit")).unwrap();

    assert!(program.is_book());
    assert_eq!(program.title, "Small Rust Book");
    assert_eq!(program.commands.len(), 3);
    let ProgramKind::Book(book) = &program.kind else {
        panic!("loaded program should be a book");
    };
    assert!(book.introduction.contains("This introduction"));
    assert_eq!(program.chapters.len(), 3);
    assert_eq!(program.chapters[0].title, "Library Internals");
    assert_eq!(program.chapters[1].title, "Helpers");
    assert_eq!(program.chapters[2].title, "Application Page");
    assert_eq!(program.chapters[0].number(), "1");
    assert_eq!(program.chapters[1].number(), "1.1");
    assert_eq!(program.chapters[2].number(), "2");
    assert_eq!(
        program.chapters[1].book.as_ref().unwrap().navigation_label,
        "Helpers"
    );

    let first_code = program.chapters[0].sections[0]
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            BlockKind::Code(code) => Some(code),
            BlockKind::Prose => None,
        })
        .unwrap();
    assert_eq!(first_code.code_type, "rust .rs");
    assert_eq!(first_code.comment_string, "// %s");
    let application_root = program.chapters[2].sections[0]
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            BlockKind::Code(code) if code.name == "src/main.rs" => Some(code),
            _ => None,
        })
        .unwrap();
    assert_eq!(application_root.code_type, "rust .rs");
    assert_eq!(application_root.comment_string, "");
}

#[test]
fn html_planner_reports_a_chapter_that_collides_with_the_contents_page() {
    let mut program =
        parse_str("chapter.lit", "@title Chapter\n@s One\nA book chapter.\n").unwrap();
    program.file = "index.lit".to_owned();
    program.kind = ProgramKind::Book(BookMetadata {
        introduction: String::new(),
        introduction_lines: Vec::new(),
    });
    program.chapters[0].book = Some(BookChapter {
        navigation_label: "Chapter".to_owned(),
        label_origin: SourceOrigin::new("index.lit", 3),
        source_path: PathBuf::from("index.lit"),
    });

    let error = plan_weave(&program).unwrap_err();
    assert!(matches!(
        error,
        WeavePlanError::Weave(errors)
            if matches!(
                &errors.as_slice()[0].kind,
                WeaveErrorKind::ContentsPathCollision { path }
                    if path == Path::new("index.html")
            )
    ));
}

#[test]
fn joint_tangling_uses_local_scopes_unique_fallback_and_ordered_roots() {
    let program = load_book(fixture("rust_shape/index.lit")).unwrap();
    let first = plan_tangle(&program).unwrap();
    let second = plan_tangle(&program).unwrap();
    assert_eq!(first, second);

    let expected = [
        ("src/lib.rs", "expected/src/lib.rs"),
        ("src/bin/demo.rs", "expected/src/bin/demo.rs"),
        ("examples/demo.rs", "expected/examples/demo.rs"),
        ("tests/generated.rs", "expected/tests/generated.rs"),
        ("src/main.rs", "expected/src/main.rs"),
    ];
    assert_eq!(first.outputs.len(), expected.len());
    for (output, (path, expected_path)) in first.outputs.iter().zip(expected) {
        assert_eq!(output.relative_path, Path::new(path));
        assert_eq!(
            output.bytes,
            fs::read(fixture(&format!("rust_shape/{expected_path}"))).unwrap()
        );
    }
}

#[test]
fn ambiguous_uses_and_changes_list_candidates_in_manifest_order() {
    let directory = TestDirectory::new("ambiguity");
    write_book(
        &directory,
        "@book\n@title Ambiguity\n[B](b.lit)\n[A](a.lit)\n[C](c.lit)\n",
        &[
            ("a.lit", "@s A\n--- Shared\nfirst\n---\n"),
            ("b.lit", "@s B\n--- Shared\nsecond\n---\n"),
            ("c.lit", "@s C\n--- out.txt\n@{Shared}\n---\n"),
        ],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let PlanError::Expansion(errors) = plan_tangle(&program).unwrap_err() else {
        panic!("ambiguous use should be an expansion error");
    };
    let ExpansionErrorKind::AmbiguousBlock { name, candidates } =
        &errors.as_slice()[0].kind
    else {
        panic!("expected an ambiguous block");
    };
    assert_eq!(name, "Shared");
    assert!(candidates[0].file.ends_with("b.lit"));
    assert!(candidates[1].file.ends_with("a.lit"));

    let WeavePlanError::Weave(errors) = plan_weave(&program).unwrap_err() else {
        panic!("ambiguous use should be a weave error");
    };
    let WeaveErrorKind::AmbiguousBlock { name, candidates } = &errors.as_slice()[0].kind
    else {
        panic!("expected an ambiguous woven block");
    };
    assert_eq!(name, "Shared");
    assert!(candidates[0].file.ends_with("b.lit"));
    assert!(candidates[1].file.ends_with("a.lit"));

    directory.write("c.lit", "@s C\n--- Shared +=\nthird\n---\n");
    let program = load_book(directory.path("index.lit")).unwrap();
    let errors = resolve(&program).unwrap_err();
    let ResolveErrorKind::AmbiguousChangeTarget {
        name,
        change,
        candidates,
    } = &errors.as_slice()[0].kind
    else {
        panic!("expected an ambiguous change target");
    };
    assert_eq!(name, "Shared");
    assert_eq!(*change, ChangeKind::Addition);
    assert!(candidates[0].file.ends_with("b.lit"));
    assert!(candidates[1].file.ends_with("a.lit"));
}

#[test]
fn documented_book_change_uses_the_authoring_chapter_scope() {
    let directory = TestDirectory::new("change-line-scope");
    write_book(
        &directory,
        "@book\n@title Greetings\n[English](english.lit)\n[Spanish](spanish.lit)\n",
        &[
            (
                "english.lit",
                "@s Greeting\n--- hello.txt\nHello\n---\n--- Punctuation\n!\n---\n",
            ),
            (
                "spanish.lit",
                "@s Saludo\n--- Punctuation\n¡\n---\n--- hello.txt +=\n@{Punctuation}\n---\n",
            ),
        ],
    );

    let program = load_book(directory.path("index.lit")).unwrap();
    let plan = plan_tangle(&program).unwrap();
    assert_eq!(plan.outputs[0].bytes, "Hello\n¡\n".as_bytes());
}

#[test]
fn roots_are_book_global_and_cross_chapter_cycles_keep_origins() {
    let directory = TestDirectory::new("root-cycle");
    write_book(
        &directory,
        "@book\n@title Roots\n[A](a.lit)\n[B](b.lit)\n",
        &[
            ("a.lit", "@s A\n--- same.txt\na\n---\n"),
            ("b.lit", "@s B\n--- same.txt\nb\n---\n"),
        ],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let errors = resolve(&program).unwrap_err();
    assert!(matches!(
        errors.as_slice()[0].kind,
        ResolveErrorKind::DuplicateRootPath { .. }
    ));

    directory.write("a.lit", "@s A\n--- out.txt\n@{A}\n---\n--- A\n@{B}\n---\n");
    directory.write("b.lit", "@s B\n--- B\n@{A}\n---\n");
    let program = load_book(directory.path("index.lit")).unwrap();
    let PlanError::Expansion(errors) = plan_tangle(&program).unwrap_err() else {
        panic!("cycle should be an expansion error");
    };
    let ExpansionErrorKind::CrossChapterReferenceCycle { blocks } =
        &errors.as_slice()[0].kind
    else {
        panic!("expected a cross-chapter cycle");
    };
    assert_eq!(
        blocks
            .iter()
            .map(|block| block.name.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "A"]
    );
    assert!(blocks[0].origin.file.ends_with("a.lit"));
    assert!(blocks[1].origin.file.ends_with("b.lit"));
}

#[test]
fn loading_aggregates_chapter_failures_in_manifest_order() {
    let directory = TestDirectory::new("loading-errors");
    directory.write(
        "index.lit",
        "@book\n@title Failures\n[Missing](z-missing.lit)\n[Parse](a-parse.lit)\n[UTF-8](m-utf8.lit)\n[Directory](b-directory.lit)\n",
    );
    directory.write("a-parse.lit", "--- Before section\ntext\n---\n");
    directory.write("m-utf8.lit", b"@s Invalid\n\xff");
    fs::create_dir(directory.path("b-directory.lit")).unwrap();

    let errors = load_book(directory.path("index.lit")).unwrap_err();
    assert_eq!(errors.as_slice().len(), 4);
    assert!(matches!(
        errors.as_slice()[0].kind,
        BookErrorKind::ReadChapter { .. }
    ));
    assert!(matches!(
        errors.as_slice()[1].kind,
        BookErrorKind::ChapterParse { .. }
    ));
    assert!(matches!(
        errors.as_slice()[2].kind,
        BookErrorKind::ChapterParse { .. }
    ));
    assert!(matches!(
        errors.as_slice()[3].kind,
        BookErrorKind::ReadChapter { .. }
    ));
    assert_eq!(
        errors
            .as_slice()
            .iter()
            .map(|error| error.origin.line)
            .collect::<Vec<_>>(),
        vec![3, 4, 5, 6]
    );
}

#[test]
fn invalid_manifest_paths_prevent_all_chapter_reads() {
    let directory = TestDirectory::new("preflight");
    directory.write(
        "index.lit",
        "@book\n@title Preflight\n[Missing](missing.lit)\n[Escape](../escape.lit)\n",
    );

    let errors = load_book(directory.path("index.lit")).unwrap_err();
    assert_eq!(errors.as_slice().len(), 1);
    assert!(matches!(
        errors.as_slice()[0].kind,
        BookErrorKind::InvalidChapterPath {
            reason: InvalidPathReason::ParentTraversal,
            ..
        }
    ));
}

#[test]
fn multi_page_weaving_uses_titles_navigation_and_scoped_cross_links() {
    let program = load_book(fixture("rust_shape/index.lit")).unwrap();
    let first = plan_weave_without_index(&program);
    let second = plan_weave_without_index(&program);
    assert_eq!(first, second);
    let pages = html_outputs(&first);
    assert_eq!(
        pages
            .iter()
            .map(|output| output.relative_path.as_path())
            .collect::<Vec<_>>(),
        vec![
            Path::new("index.html"),
            Path::new("chapters/library.html"),
            Path::new("chapters/helpers.html"),
            Path::new("chapters/application.html"),
        ]
    );
    let expected = [
        include_bytes!("fixtures/book/rust_shape/expected_html/index.html").as_slice(),
        include_bytes!("fixtures/book/rust_shape/expected_html/chapters/library.html")
            .as_slice(),
        include_bytes!("fixtures/book/rust_shape/expected_html/chapters/helpers.html")
            .as_slice(),
        include_bytes!(
            "fixtures/book/rust_shape/expected_html/chapters/application.html"
        )
        .as_slice(),
    ];
    for (output, expected) in pages.iter().zip(expected) {
        assert_eq!(output.bytes, expected, "{}", output.relative_path.display());
    }

    let contents = String::from_utf8(pages[0].bytes.clone()).unwrap();
    assert!(contents.contains("<title>Small Rust Book</title>"));
    assert!(
        contents.contains("<ol>\n<li><a href=\"chapters/helpers.html\">1.1. Helpers</a>")
    );
    let helpers = String::from_utf8(pages[2].bytes.clone()).unwrap();
    assert!(helpers.contains("<title>Helpers — Small Rust Book</title>"));
    assert_eq!(helpers.matches("aria-label=\"Book navigation\"").count(), 2);
    assert!(helpers.contains("Previous: Library"));
    assert!(helpers.contains("Next: Application"));
    assert!(
        helpers.contains("href=\"library.html#1:2\" aria-label=\"section 1.2\">1.2</a>")
    );
    assert!(helpers.contains(
        "<div class=\"section-opening\">\n<h2 class=\"section-heading\"><span class=\"section-number\">1.</span> <span class=\"section-title\">Helper outputs</span>.</h2>\n<p>This chapter uses the shared"
    ));
    assert!(
        helpers.contains(
            "⟨Local variables <a href=\"#1.1:4\" aria-label=\"section 4\">4</a>⟩"
        )
    );
    assert!(helpers.contains(
        "<div class=\"codeblock codeblock-first\">\n<div class=\"section-opening\">\n<h2 class=\"section-heading section-heading-untitled\"><span class=\"section-number\">2.</span> <span class=\"visually-hidden\">Untitled section</span></h2>\n<div class=\"codeblock_name\"><span class=\"section-name\">⟨<strong>examples/demo.rs</strong>"
    ));

    let library = String::from_utf8(pages[1].bytes.clone()).unwrap();
    assert_eq!(library.matches("aria-label=\"Book navigation\"").count(), 2);
    assert!(!library.contains("Previous:"));
    assert!(library.contains("Next: Helpers"));
    assert!(library.contains(
        "<p class=\"seealso\">See also section <a href=\"helpers.html#1.1:5\" aria-label=\"section 1.1.5\">1.1.5</a>.</p>"
    ));
    assert!(library.contains(
        "<p class=\"seealso\">This code is used in sections <a href=\"#1:1\" aria-label=\"section 1\">1</a>, <a href=\"helpers.html#1.1:1\" aria-label=\"section 1.1.1\">1.1.1</a>, and <a href=\"application.html#2:1\" aria-label=\"section 2.1\">2.1</a>.</p>"
    ));
    assert!(helpers.contains(
        "<p class=\"seealso\">This code is used in sections <a href=\"#1.1:1\" aria-label=\"section 1\">1</a>, <a href=\"#1.1:2\" aria-label=\"section 2\">2</a>, and <a href=\"#1.1:3\" aria-label=\"section 3\">3</a>.</p>"
    ));
    let application = String::from_utf8(pages[3].bytes.clone()).unwrap();
    assert_eq!(
        application
            .matches("aria-label=\"Book navigation\"")
            .count(),
        2
    );
    assert!(application.contains("Previous: Helpers"));
    assert!(!application.contains("Next:"));
}

#[test]
fn rust_book_identifier_index_groups_chapters_filters_entries_and_links_navigation() {
    let program = load_book(fixture("identifier_index/index.lit")).unwrap();
    let first = plan_weave(&program).unwrap();
    let second = plan_weave(&program).unwrap();
    assert_eq!(first, second);
    let pages = html_outputs(&first);
    assert_eq!(
        pages
            .iter()
            .map(|output| output.relative_path.as_path())
            .collect::<Vec<_>>(),
        vec![
            Path::new("index.html"),
            Path::new("chapters/definitions.html"),
            Path::new("nested/deep/uses.html"),
            Path::new("other.html"),
            Path::new("index-identifiers.html"),
        ]
    );

    let html = |path: &str| {
        let bytes = first
            .outputs
            .iter()
            .find(|output| output.relative_path == Path::new(path))
            .unwrap()
            .bytes
            .clone();
        String::from_utf8(bytes).unwrap()
    };
    let contents = html("index.html");
    let definitions = html("chapters/definitions.html");
    let uses = html("nested/deep/uses.html");
    let index = html("index-identifiers.html");

    assert!(contents.contains(
        "<nav class=\"book-index-navigation\" aria-label=\"Book indexes\">\n<a href=\"index-identifiers.html\">Identifier Index</a>"
    ));
    assert!(definitions.contains(
        "<a class=\"identifier-index-link\" href=\"../index-identifiers.html\">Identifier Index</a>"
    ));
    assert!(uses.contains(
        "<a class=\"identifier-index-link\" href=\"../../index-identifiers.html\">Identifier Index</a>"
    ));
    assert_eq!(
        index
            .matches("<nav class=\"book-navigation\" aria-label=\"Book navigation\">")
            .count(),
        2
    );
    assert!(index.contains("<a class=\"contents\" href=\"index.html\">Contents</a>"));
    assert!(index.contains(
        "<div class=\"identifier-chapter-group\"><a class=\"identifier-chapter\" href=\"chapters/definitions.html\">1. Definitions</a>:"
    ));
    assert!(index.contains(
        "<div class=\"identifier-chapter-group\"><a class=\"identifier-chapter\" href=\"nested/deep/uses.html\">2. Uses &amp; More</a>:"
    ));

    let shared_start = index.find("<dt><code>shared_name</code></dt>").unwrap();
    let shared_end = index[shared_start..]
        .find("</dd>")
        .map(|offset| shared_start + offset)
        .unwrap();
    let shared = &index[shared_start..shared_end];
    assert!(shared.contains(
        "class=\"identifier-definition\" href=\"chapters/definitions.html#1:1\" aria-label=\"definition in chapter 1, section 1\""
    ));
    assert!(shared.contains("href=\"chapters/definitions.html#1:2\""));
    assert_eq!(
        shared.matches("href=\"nested/deep/uses.html#2:1\"").count(),
        1
    );

    let local_type_start = index.find("<dt><code>LocalType</code></dt>").unwrap();
    let local_type_end = index[local_type_start..]
        .find("</dd>")
        .map(|offset| local_type_start + offset)
        .unwrap();
    let local_type = &index[local_type_start..local_type_end];
    assert!(local_type.contains("chapters/definitions.html#1:1"));
    assert!(
        local_type.contains(
            "class=\"identifier-definition\" href=\"nested/deep/uses.html#2:1\""
        )
    );

    let q_start = index.find("<dt><code>q</code></dt>").unwrap();
    let q_end = index[q_start..]
        .find("</dd>")
        .map(|offset| q_start + offset)
        .unwrap();
    let q = &index[q_start..q_end];
    assert_eq!(q.matches("identifier-definition").count(), 1);
    assert_eq!(
        q.matches("href=\"chapters/definitions.html#1:2\"").count(),
        1
    );

    for absent in ["ExternalType", "c_name", "hidden_name"] {
        assert!(!index.contains(&format!("<dt><code>{absent}</code></dt>")));
    }
    assert!(index.find("<dt><code>LocalType</code>").unwrap() < q_start);
    assert!(q_start < shared_start);

    let disabled = plan_weave_without_index(&program);
    assert_eq!(html_outputs(&disabled).len(), 4);
    for output in html_outputs(&disabled) {
        let html = String::from_utf8(output.bytes.clone()).unwrap();
        assert!(!html.contains("Identifier Index"));
        assert!(!html.contains("book-navigation-indexes"));
    }
}

#[test]
fn ambiguous_cross_chapter_context_keeps_local_identifier_results() {
    let directory = TestDirectory::new("ambiguous-identifier-context");
    write_book(
        &directory,
        "@book\n@title Ambiguous Context\n@code_type rust .rs\n\
         [First](first.lit)\n[Second](second.lit)\n[User](user.lit)\n",
        &[
            (
                "first.lit",
                "@s First\n--- Shared\nfn first_local() {}\n---\n",
            ),
            (
                "second.lit",
                "@s Second\n--- Shared\nfn second_local() {}\n---\n",
            ),
            (
                "user.lit",
                "@s User\n--- user.rs\nfn before_ambiguous() {}\n\
                 @{Shared}\nfn after_ambiguous() {}\n---\n",
            ),
        ],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let index = analyze(&program);
    let names = index
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();

    for present in [
        "first_local",
        "second_local",
        "before_ambiguous",
        "after_ambiguous",
    ] {
        assert!(names.contains(&present), "{present}: {index:#?}");
    }
}

#[test]
fn identifier_index_output_collision_is_a_planning_error_and_writes_nothing() {
    let directory = TestDirectory::new("identifier-index-collision");
    write_book(
        &directory,
        "@book\n@title Collision\n@code_type rust .rs\n[Reserved](index-identifiers.lit)\n",
        &[(
            "index-identifiers.lit",
            "@s Rust\n--- generated.rs\nfn indexed_name() {}\n---\n",
        )],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let WeavePlanError::Weave(errors) = plan_weave(&program).unwrap_err() else {
        panic!("the reserved index path should be a weave error");
    };
    assert_eq!(errors.as_slice()[0].origin.line, 4);
    assert!(matches!(
        &errors.as_slice()[0].kind,
        WeaveErrorKind::IdentifierIndexPathCollision { path }
            if path == Path::new("index-identifiers.html")
    ));

    let without_index = plan_weave_without_index(&program);
    assert_eq!(html_outputs(&without_index).len(), 2);

    let output_directory = directory.path("generated");
    let result = lw(&[
        Path::new("-w"),
        Path::new("-odir"),
        &output_directory,
        &directory.path("index.lit"),
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains(
        "the identifier index output index-identifiers.html conflicts with another book page"
    ));
    assert!(!output_directory.exists());
}

#[test]
fn unsupported_language_book_has_no_empty_identifier_page_or_navigation() {
    let directory = TestDirectory::new("empty-identifier-index");
    write_book(
        &directory,
        "@book\n@title C Book\n@code_type c .c\n[C](chapter.lit)\n",
        &[(
            "chapter.lit",
            "@s C\n--- example.c\nint c_name(void) { return 0; }\n---\n",
        )],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let plan = plan_weave(&program).unwrap();
    assert_eq!(
        html_outputs(&plan)
            .iter()
            .map(|output| output.relative_path.as_path())
            .collect::<Vec<_>>(),
        vec![Path::new("index.html"), Path::new("chapter.html")]
    );
    for output in html_outputs(&plan) {
        let html = String::from_utf8(output.bytes.clone()).unwrap();
        assert!(!html.contains("Identifier Index"));
        assert!(!html.contains("book-navigation-indexes"));
    }
}

#[test]
fn book_math_pages_share_one_bundle_and_use_page_relative_links() {
    let directory = TestDirectory::new("book-math");
    write_book(
        &directory,
        "@book\n@title Math Book\nIntroduction $i^2 = -1$.\n[Math](chapters/math.lit)\n[Plain](nested/deep/plain.lit)\n",
        &[
            (
                "chapters/math.lit",
                "@title Math\n@s Equation\n$$x^2 + y^2 = z^2$$\n",
            ),
            (
                "nested/deep/plain.lit",
                "@title Plain\n@s Prose\nThere is no equation here.\n",
            ),
        ],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let plan = plan_weave(&program).unwrap();
    assert_eq!(plan.outputs.len(), 28);

    let html = |path: &str| {
        String::from_utf8(
            plan.outputs
                .iter()
                .find(|output| output.relative_path == Path::new(path))
                .unwrap()
                .bytes
                .clone(),
        )
        .unwrap()
    };
    let contents = html("index.html");
    let math = html("chapters/math.html");
    let plain = html("nested/deep/plain.html");

    assert!(contents.contains(
        "<html lang=\"en\" data-litweb-ready=\"pending\" \
         data-litweb-math-ready=\"pending\">"
    ));
    assert!(math.contains(
        "<html lang=\"en\" data-litweb-ready=\"pending\" \
         data-litweb-math-ready=\"pending\">"
    ));
    assert!(plain.contains("<html lang=\"en\">"));
    assert!(!plain.contains("data-litweb-ready"));
    assert!(contents.contains("href=\"litweb-assets/katex-0.18.1/katex.min.css\""));
    assert!(contents.contains("src=\"litweb-assets/katex-0.18.1/litweb-math.js\""));
    assert!(math.contains("href=\"../litweb-assets/katex-0.18.1/katex.min.css\""));
    assert!(math.contains("src=\"../litweb-assets/katex-0.18.1/litweb-math.js\""));
    assert!(!plain.contains("data-litweb-math-ready"));
    assert!(!plain.contains("katex.min"));
    assert!(!plain.contains("litweb-math"));
    assert_eq!(
        plan.outputs
            .iter()
            .filter(|output| {
                output
                    .relative_path
                    .starts_with("litweb-assets/katex-0.18.1")
            })
            .count(),
        25
    );
}

#[test]
fn book_highlighting_inherits_overrides_disables_and_links_one_shared_bundle() {
    let directory = TestDirectory::new("book-highlighting");
    write_book(
        &directory,
        "@book\n@title Highlight Book\n@code_type rust .rs\n@colorscheme dark\n\
         Introduction uses @code{let contents = true;}.\n\
         [Light](chapters/light.lit)\n[Disabled](nested/disabled.lit)\n[Plain](plain.lit)\n",
        &[
            (
                "chapters/light.lit",
                "@title Light\n@colorscheme coy\n@s Code\n\
                 --- light.rs\nfn light() {}\n---\n",
            ),
            (
                "nested/disabled.lit",
                "@title Disabled\n@colorscheme none\n@s Code\n\
                 --- disabled.rs\nfn disabled() {}\n---\n",
            ),
            (
                "plain.lit",
                "@title Plain\n@s Prose\nNo target code here.\n",
            ),
        ],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let plan = plan_weave(&program).unwrap();
    let html = |path: &str| {
        String::from_utf8(
            plan.outputs
                .iter()
                .find(|output| output.relative_path == Path::new(path))
                .unwrap()
                .bytes
                .clone(),
        )
        .unwrap()
    };

    let contents = html("index.html");
    assert!(contents.contains("<code class=\"source-code\">let contents = true;</code>"));
    assert!(!contents.contains("data-litweb-highlight"));
    assert!(!contents.contains("prism-1.30.0"));

    let light = html("chapters/light.html");
    assert!(light.contains("border-left:10px solid #358ccb"));
    assert!(light.contains("src=\"../litweb-assets/prism-1.30.0/prism-all.min.js\""));

    let disabled = html("nested/disabled.html");
    assert!(
        disabled.contains("<pre class=\"language-rust\"><code class=\"language-rust\">")
    );
    assert!(!disabled.contains("data-litweb-highlight=\"pending\""));
    assert!(!disabled.contains("data-litweb-highlight-ready"));
    assert!(!disabled.contains("prism-1.30.0"));

    let plain = html("plain.html");
    assert!(!plain.contains("data-litweb-highlight"));
    assert!(!plain.contains("prism-1.30.0"));
    assert_eq!(
        plan.outputs
            .iter()
            .filter(|output| output
                .relative_path
                .starts_with("litweb-assets/prism-1.30.0"))
            .count(),
        14
    );

    let uniform = plan_weave_with_options(
        &program,
        WeaveOptions {
            identifier_index: true,
            color_scheme: Some(PathBuf::from("litweb")),
        },
    )
    .unwrap();
    let contents = uniform
        .outputs
        .iter()
        .find(|output| output.relative_path == Path::new("index.html"))
        .unwrap();
    let contents = String::from_utf8(contents.bytes.clone()).unwrap();
    assert!(!contents.contains("data-litweb-highlight"));
    assert!(!contents.contains("prism-1.30.0"));

    for path in ["chapters/light.html", "nested/disabled.html"] {
        let page = uniform
            .outputs
            .iter()
            .find(|output| output.relative_path == Path::new(path))
            .unwrap();
        let html = String::from_utf8(page.bytes.clone()).unwrap();
        assert!(
            html.contains("Litweb's GitHub-inspired light theme for Prism"),
            "{path}"
        );
        assert!(
            html.contains("data-litweb-highlight-ready=\"pending\""),
            "{path}"
        );
    }
}

#[test]
fn presentation_book_uses_derived_local_and_cross_page_locations() {
    let program = load_book(fixture("presentation/index.lit")).unwrap();
    let first = plan_weave_without_index(&program);
    let second = plan_weave_without_index(&program);
    assert_eq!(first, second);

    let expected = [
        (
            "index.html",
            include_bytes!("fixtures/book/presentation/expected_html/index.html")
                .as_slice(),
        ),
        (
            "chapters/first.html",
            include_bytes!(
                "fixtures/book/presentation/expected_html/chapters/first.html"
            )
            .as_slice(),
        ),
        (
            "chapters/middle.html",
            include_bytes!(
                "fixtures/book/presentation/expected_html/chapters/middle.html"
            )
            .as_slice(),
        ),
        (
            "chapters/last.html",
            include_bytes!("fixtures/book/presentation/expected_html/chapters/last.html")
                .as_slice(),
        ),
    ];
    let pages = html_outputs(&first);
    assert_eq!(pages.len(), expected.len());
    for (output, (path, expected)) in pages.iter().zip(expected) {
        assert_eq!(output.relative_path, Path::new(path));
        assert_eq!(output.bytes, expected, "{path}");
    }

    let first_page = String::from_utf8(pages[1].bytes.clone()).unwrap();
    assert_eq!(first_page.matches("<section class=\"section\"").count(), 3);
    assert!(first_page.contains("id=\"1:1\""));
    assert!(first_page.contains("id=\"1:3\""));
    assert!(
        first_page.contains(
            "See also section <a href=\"#1:2\" aria-label=\"section 2\">2</a>."
        )
    );
    assert!(first_page.contains(
        "<span class=\"section-number\">2.</span> <span class=\"visually-hidden\">Untitled section</span></h2>\n<div class=\"codeblock_name\"><span class=\"section-name\">⟨Shared <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is continued by\">+≡</span>"
    ));
    assert!(!first_page.contains("Previous:"));
    assert!(first_page.contains("Next: Middle"));

    let middle_page = String::from_utf8(pages[2].bytes.clone()).unwrap();
    assert_eq!(middle_page.matches("<section class=\"section\"").count(), 2);
    assert!(
        middle_page
            .contains("href=\"first.html#1:3\" aria-label=\"section 1.3\">1.3</a>")
    );
    assert!(
        middle_page.contains("href=\"last.html#3:3\" aria-label=\"section 3.3\">3.3</a>")
    );
    assert!(middle_page.contains(
        "<div class=\"section-opening\">\n<h2 class=\"section-heading section-heading-untitled\"><span class=\"section-number\">2.</span> <span class=\"visually-hidden\">Untitled section</span></h2>\n<p>Prose after code starts an implicit section with its following code.</p>\n</div>\n<div class=\"codeblock\">"
    ));
    assert!(middle_page.contains("Previous: First"));
    assert!(middle_page.contains("Next: Last"));

    let last_page = String::from_utf8(pages[3].bytes.clone()).unwrap();
    assert_eq!(last_page.matches("<section class=\"section\"").count(), 3);
    assert!(last_page.contains(
        "<span class=\"nocode\">⟨Shared <a href=\"#3:1\" aria-label=\"section 1\">1</a>⟩</span>"
    ));
    assert!(last_page.contains("Previous: Middle"));
    assert!(!last_page.contains("Next:"));
}

#[test]
fn nested_unicode_page_paths_are_percent_encoded_only_in_urls() {
    let directory = TestDirectory::new("encoded-book-links");
    write_book(
        &directory,
        "@book\n@title Escaped <Book>\n[One & <](nested space/é.lit)\n[Two](other/two.lit)\n",
        &[
            (
                "nested space/é.lit",
                "@title <First & Page>\n@s One\n--- Shared\ntext\n---\n",
            ),
            (
                "other/two.lit",
                "@s Two\nThe @{Shared} block is elsewhere.\n",
            ),
        ],
    );
    let program = load_book(directory.path("index.lit")).unwrap();
    let plan = plan_weave_without_index(&program);

    assert_eq!(
        plan.outputs[1].relative_path,
        Path::new("nested space/é.html")
    );
    let contents = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(contents.contains("<title>Escaped &lt;Book&gt;</title>"));
    assert!(contents.contains("href=\"nested%20space/%C3%A9.html\""));
    assert!(contents.contains("One &amp; &lt;"));
    let second = String::from_utf8(plan.outputs[2].bytes.clone()).unwrap();
    assert!(second.contains("href=\"../nested%20space/%C3%A9.html#1:1\""));
    assert!(second.contains("Previous: One &amp; &lt;"));
}

#[test]
fn public_cli_supports_every_book_mode_with_deterministic_outputs() {
    let directory = TestDirectory::new("cli-book-modes");
    let input = fixture("rust_shape/index.lit");
    let tangle = directory.path("tangle");
    let weave = directory.path("weave");
    let default = directory.path("default");
    let indexed = directory.path("indexed");

    let tangle_result = lw(&[Path::new("-t"), Path::new("-odir"), &tangle, &input]);
    let weave_result = lw(&[
        Path::new("-w"),
        Path::new("--no-index"),
        Path::new("-odir"),
        &weave,
        &input,
    ]);
    let default_result = lw(&[
        Path::new("--no-index"),
        Path::new("-odir"),
        &default,
        &input,
    ]);
    for result in [&tangle_result, &weave_result, &default_result] {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    for path in [
        "src/lib.rs",
        "src/bin/demo.rs",
        "examples/demo.rs",
        "tests/generated.rs",
        "src/main.rs",
    ] {
        assert_eq!(
            fs::read(tangle.join(path)).unwrap(),
            fs::read(default.join(path)).unwrap()
        );
        assert!(!weave.join(path).exists());
    }
    for path in [
        "index.html",
        "chapters/library.html",
        "chapters/helpers.html",
        "chapters/application.html",
    ] {
        assert_eq!(
            fs::read(weave.join(path)).unwrap(),
            fs::read(default.join(path)).unwrap()
        );
        assert!(!tangle.join(path).exists());
    }

    let indexed_result = lw(&[Path::new("-w"), Path::new("-odir"), &indexed, &input]);
    assert!(
        indexed_result.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed_result.stderr)
    );
    assert!(indexed.join("index-identifiers.html").is_file());
    assert!(
        fs::read_to_string(indexed.join("index.html"))
            .unwrap()
            .contains("Identifier Index")
    );
    assert!(
        fs::read_to_string(indexed.join("chapters/helpers.html"))
            .unwrap()
            .contains("href=\"../index-identifiers.html\"")
    );
}

#[test]
fn book_failures_create_no_partial_cli_output() {
    let directory = TestDirectory::new("cli-book-failures");
    directory.write(
        "loading/index.lit",
        "@book\n@title Missing\n[Missing](missing.lit)\n",
    );
    let loading_output = directory.path("loading-output");
    let loading = lw(&[
        Path::new("-w"),
        Path::new("-odir"),
        &loading_output,
        &directory.path("loading/index.lit"),
    ]);
    assert_eq!(loading.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&loading.stderr)
            .contains("cannot read chapter missing.lit")
    );
    assert!(!loading_output.exists());

    write_book(
        &directory,
        "@book\n@title Render\nThe @{Missing} block is absent.\n[One](one.lit)\n",
        &[("one.lit", "@s One\n--- output.txt\ntext\n---\n")],
    );
    let render_output = directory.path("render-output");
    let rendering = lw(&[
        Path::new("-odir"),
        &render_output,
        &directory.path("index.lit"),
    ]);
    assert_eq!(rendering.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&rendering.stderr)
            .contains("code block {Missing} is not defined")
    );
    assert!(!render_output.exists());

    let obstacle_output = directory.path("obstacle-output");
    fs::create_dir(&obstacle_output).unwrap();
    directory.write("obstacle-output/chapters", "not a directory");
    let obstacle = lw(&[
        Path::new("-w"),
        Path::new("-odir"),
        &obstacle_output,
        &fixture("rust_shape/index.lit"),
    ]);
    assert_eq!(obstacle.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&obstacle.stderr)
            .contains("an output parent is not a directory")
    );
    assert!(!obstacle_output.join("index.html").exists());
}

#[test]
fn book_outputs_cannot_overwrite_any_loaded_input() {
    let directory = TestDirectory::new("cli-book-input-collision");
    write_book(
        &directory,
        "@book\n@title Protected\n[Chapter](chapter.lit)\n",
        &[(
            "chapter.lit",
            "@s Root\n--- chapter.lit\nreplacement\n---\n",
        )],
    );
    let original = fs::read(directory.path("chapter.lit")).unwrap();

    let result = lw(&[
        Path::new("-t"),
        Path::new("-odir"),
        directory.path(".").as_path(),
        &directory.path("index.lit"),
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("the output would overwrite the input file")
    );
    assert_eq!(fs::read(directory.path("chapter.lit")).unwrap(), original);
}

#[test]
fn manifest_parse_resolution_and_combined_collisions_write_nothing() {
    let directory = TestDirectory::new("cli-book-planning-failures");

    directory.write("parse/index.lit", "@book\n@title Empty\n");
    let parse_output = directory.path("parse-output");
    let parsing = lw(&[
        Path::new("-w"),
        Path::new("-odir"),
        &parse_output,
        &directory.path("parse/index.lit"),
    ]);
    assert_eq!(parsing.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&parsing.stderr)
            .contains("book manifest has no chapters")
    );
    assert!(!parse_output.exists());

    directory.write(
        "resolution/index.lit",
        "@book\n@title Roots\n[A](a.lit)\n[B](b.lit)\n",
    );
    directory.write("resolution/a.lit", "@s A\n--- same.txt\na\n---\n");
    directory.write("resolution/b.lit", "@s B\n--- same.txt\nb\n---\n");
    let resolution_output = directory.path("resolution-output");
    let resolution = lw(&[
        Path::new("-t"),
        Path::new("-odir"),
        &resolution_output,
        &directory.path("resolution/index.lit"),
    ]);
    assert_eq!(resolution.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&resolution.stderr).contains("more than one root writes")
    );
    assert!(!resolution_output.exists());

    directory.write(
        "collision/index.lit",
        "@book\n@title Collision\n[One](one.lit)\n",
    );
    directory.write(
        "collision/one.lit",
        "@s One\n--- index.html\ntangled\n---\n",
    );
    let collision_output = directory.path("collision-output");
    let collision = lw(&[
        Path::new("-odir"),
        &collision_output,
        &directory.path("collision/index.lit"),
    ]);
    assert_eq!(collision.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&collision.stderr)
            .contains("more than one generated file has this destination")
    );
    assert!(!collision_output.exists());
}
