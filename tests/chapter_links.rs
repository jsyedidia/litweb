use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use litweb::book::load_book;
use litweb::latex::{
    LatexErrorKind, LatexOptions, LatexPlanError, plan_latex, plan_latex_with_options,
};
use litweb::output::PlannedOutput;
use litweb::parser::parse_str;
use litweb::tangler::plan_tangle;
use litweb::weaver::{
    WeaveErrorKind, WeaveOptions, WeavePlanError, plan_weave, plan_weave_with_options,
};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/book/chapter_links/index.lit")
}

fn output(outputs: &[PlannedOutput], name: &str) -> String {
    let file = outputs
        .iter()
        .find(|p| p.relative_path == Path::new(name))
        .unwrap();
    String::from_utf8(file.bytes.clone()).unwrap()
}

#[test]
fn book_links_select_declared_chapters_in_both_formats() {
    let book = load_book(fixture()).unwrap();
    assert_eq!(book.chapters.len(), 6);
    for identifier_index in [false, true] {
        let html = plan_weave_with_options(
            &book,
            WeaveOptions {
                identifier_index,
                color_scheme: Some(PathBuf::from("none")),
            },
        )
        .unwrap();
        let tex = plan_latex_with_options(
            &book,
            LatexOptions {
                identifier_index,
                ..LatexOptions::default()
            },
        )
        .unwrap();
        let introduction = output(&html.outputs, "index.html");
        assert!(
            introduction
                .contains("<a href=\"start.html\">The <strong>start</strong></a>")
        );
        let start = output(&html.outputs, "start.html");
        for (label, target) in [
            ("The map", "maps/map.html"),
            ("The route map", "routes/map.html"),
            ("Details", "routes/details.html"),
            ("Empty", "empty.html"),
            ("Names", "names/space%20%26%20%25.html"),
            ("This chapter", "start.html"),
            ("The <em>map</em>", "maps/map.html"),
            ("Table map", "maps/map.html"),
        ] {
            assert!(start.contains(&format!("<a href=\"{target}\">{label}</a>")));
        }
        let details = output(&html.outputs, "routes/details.html");
        for (label, target) in [
            ("Sibling", "map.html"),
            ("Other map", "../maps/map.html"),
            ("Start", "../start.html"),
            ("Self", "details.html"),
        ] {
            assert!(details.contains(&format!("<a href=\"{target}\">{label}</a>")));
        }
        assert!(
            output(&html.outputs, "routes/map.html")
                .contains("<a href=\"../maps/map.html\">Other map</a>")
        );
        let latex = output(&tex.outputs, "index.tex");
        for (label, chapter) in [
            ("The \\textbf{start}", 1),
            ("The map", 4),
            ("The route map", 3),
            ("Details", 2),
            ("Empty", 5),
            ("Names", 6),
            ("This chapter", 1),
            ("The \\emph{map}", 4),
            ("Table map", 4),
            ("Sibling", 3),
            ("Other map", 4),
            ("Start", 1),
            ("Self", 2),
        ] {
            let link = format!("\\hyperlink{{litweb-chapter-{chapter}}}{{{label}}}");
            assert!(latex.contains(&link), "missing {link}");
        }
        for chapter in 1..=6 {
            assert_eq!(
                latex
                    .matches(&format!("\\hypertarget{{litweb-chapter-{chapter}}}"))
                    .count(),
                1
            );
        }
        assert!(latex.contains(
            "\\section[Details]{\\hypertarget{litweb-chapter-2}{}Route Details}"
        ));
        assert!(latex.contains(
            "\\LitwebLiterateChapter[Empty]{\\hypertarget{litweb-chapter-5}{}Empty Chapter}"
        ));
        assert!(!latex.contains("\\href{\\detokenize{maps/map.lit}}"));
        assert!(start.contains("<code>[Absent](missing.lit)</code>"));
        assert!(!start.contains("href=\"missing.lit\""));
    }
    let tangled = plan_tangle(&book).unwrap();
    assert!(
        output(&tangled.outputs, "link_example.rs")
            .contains("pub const LINK: &str = \"[Absent](missing.lit)\";")
    );
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "litweb-chapter-links-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("index.lit"),
            "@book\n@title Links\n@code_type text .txt\n@comment_type none\n[One](one.lit)\n",
        )
        .unwrap();
        Self(path)
    }

    fn chapter(&self, text: &str) {
        fs::write(self.0.join("one.lit"), text).unwrap();
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn invalid_chapter_links_report_the_authored_line_in_both_backends() {
    let directory = TestDirectory::new();
    for target in [
        "missing.lit",
        "../one.lit",
        "dir/../../one.lit",
        "index.lit",
        "a//one.lit",
    ] {
        directory.chapter(&format!(
            "@title One\n@s Reading\nFirst line\nthen **see [Absent]({target})**.\n"
        ));
        let book = load_book(directory.0.join("index.lit")).unwrap();
        let WeavePlanError::Weave(errors) = plan_weave(&book).unwrap_err() else {
            panic!("expected a weave error");
        };
        let error = &errors.as_slice()[0];
        assert_eq!(error.origin.line, 4);
        assert!(error.origin.file.ends_with("one.lit"));
        assert_eq!(
            error.kind,
            WeaveErrorKind::InvalidChapterLink {
                target: target.to_owned()
            }
        );
        let LatexPlanError::Latex(errors) = plan_latex(&book).unwrap_err() else {
            panic!("expected a LaTeX error");
        };
        let error = &errors.as_slice()[0];
        assert_eq!(error.origin.line, 4);
        assert!(error.origin.file.ends_with("one.lit"));
        assert_eq!(
            error.kind,
            LatexErrorKind::InvalidChapterLink {
                target: target.to_owned()
            }
        );
    }
}

#[test]
fn introduction_links_are_prose_and_errors_refer_to_the_manifest() {
    let directory = TestDirectory::new();
    directory.chapter("@title One\n");
    for (target, succeeds) in [("./one.lit", true), ("absent.lit", false)] {
        fs::write(
            directory.0.join("index.lit"),
            format!("@book\n@title Links\nRead [One]({target}) next.\n[One](one.lit)\n"),
        )
        .unwrap();
        let book = load_book(directory.0.join("index.lit")).unwrap();
        assert_eq!(book.chapters.len(), 1);
        if succeeds {
            let html = plan_weave(&book).unwrap();
            assert!(
                output(&html.outputs, "index.html")
                    .contains("Read <a href=\"one.html\">One</a> next.")
            );
            let tex = plan_latex(&book).unwrap();
            assert!(
                output(&tex.outputs, "index.tex")
                    .contains("Read \\hyperlink{litweb-chapter-1}{One} next.")
            );
        } else {
            let WeavePlanError::Weave(errors) = plan_weave(&book).unwrap_err() else {
                panic!("expected a weave error");
            };
            assert_eq!(errors.as_slice()[0].origin.line, 3);
            assert!(errors.as_slice()[0].origin.file.ends_with("index.lit"));
            let LatexPlanError::Latex(errors) = plan_latex(&book).unwrap_err() else {
                panic!("expected a LaTeX error");
            };
            assert_eq!(errors.as_slice()[0].origin.line, 3);
            assert!(errors.as_slice()[0].origin.file.ends_with("index.lit"));
        }
    }
}

#[test]
fn ordinary_links_and_standalone_source_links_keep_their_targets() {
    let directory = TestDirectory::new();
    let targets = [
        "guide.html",
        "guide.pdf",
        "https://example.org/map.lit",
        "//example.org/map.lit",
        "/map.lit",
        "mailto:map.lit",
        "map.lit?view=1",
        "map.lit#part",
        "#part",
        "map.LIT",
        "folder\\map.lit",
    ];
    let mut prose = String::from("@title Links\n@s Reading\n");
    for target in targets {
        prose.push_str(&format!("See [Link]({target}).\n"));
    }
    prose.push_str("Unsafe [Link](javascript:map.lit).\n");
    directory.chapter(&prose);
    let book = load_book(directory.0.join("index.lit")).unwrap();
    let html = output(&plan_weave(&book).unwrap().outputs, "one.html");
    let tex = output(&plan_latex(&book).unwrap().outputs, "index.tex");
    for target in targets {
        assert!(html.contains(&format!("href=\"{target}\"")), "{target}");
        // Backslashes are escaped specially in LaTeX URL data.
        if !target.contains('\\') {
            assert!(
                tex.contains(&format!("\\href{{\\detokenize{{{target}}}}}")),
                "{target}"
            );
        }
    }
    assert!(!html.contains("href=\"javascript:"));
    assert!(!tex.contains("\\href{\\detokenize{javascript:"));
    let standalone =
        parse_str("alone.lit", "@s Reading\nSee [Map](missing.lit).\n").unwrap();
    assert!(
        output(&plan_weave(&standalone).unwrap().outputs, "alone.html")
            .contains("<a href=\"missing.lit\">Map</a>")
    );
    assert!(
        output(&plan_latex(&standalone).unwrap().outputs, "alone.tex")
            .contains("\\href{\\detokenize{missing.lit}}{Map}")
    );
}

#[test]
fn cli_link_failures_leave_outputs_untouched_and_tangle_only_still_works() {
    let directory = TestDirectory::new();
    directory.chapter(
        "@s Reading\nSee [Absent](missing.lit).\n\n--- result.txt\nunchanged\n---\n",
    );
    let out = directory.0.join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("result.txt"), "existing").unwrap();
    for format in ["html", "latex"] {
        let result = Command::new(env!("CARGO_BIN_EXE_lw"))
            .current_dir(&directory.0)
            .args(["--format", format, "--out-dir", "out", "index.lit"])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains("one.lit:2")
        );
        assert_eq!(
            fs::read_to_string(out.join("result.txt")).unwrap(),
            "existing"
        );
        assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
    }
    let result = Command::new(env!("CARGO_BIN_EXE_lw"))
        .current_dir(&directory.0)
        .args(["-t", "--out-dir", "out", "index.lit"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(
        fs::read_to_string(out.join("result.txt")).unwrap(),
        "unchanged\n"
    );
}
