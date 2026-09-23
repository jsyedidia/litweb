use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use litweb::book::load_book;
use litweb::latex::{
    LatexChapterOpening, LatexErrorKind, LatexFontSize, LatexOptions, LatexPlanError,
    plan_latex, plan_latex_with_options,
};
use litweb::output::{OutputErrorKind, write_planned_outputs};
use litweb::parser::parse_str;
use litweb::tangler::plan_tangle;

const STANDALONE: &str = include_str!("fixtures/latex/standalone/input.lit");
const MINI_INDEX: &str = include_str!("fixtures/latex/mini_index/input.lit");

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "litweb-latex-{label}-{}-{number}",
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

fn lw_in(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lw"))
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("lw should run")
}

fn standalone_plan() -> litweb::latex::LatexPlan {
    let program =
        parse_str("demonstration.lit", STANDALONE).expect("fixture should parse");
    plan_latex(&program).expect("fixture should render as LaTeX")
}

#[test]
fn standalone_latex_plans_one_document_and_its_packaged_style() {
    let plan = standalone_plan();
    assert_eq!(plan.outputs.len(), 2);
    assert_eq!(
        plan.outputs[0].relative_path,
        Path::new("demonstration.tex")
    );
    assert_eq!(
        plan.outputs[1].relative_path,
        Path::new("litweb-latex/litweb.sty")
    );
    assert_eq!(
        plan.outputs[1].bytes,
        include_bytes!("../assets/latex/litweb.sty")
    );
    let style = String::from_utf8(plan.outputs[1].bytes.clone()).unwrap();
    assert!(style.contains("linkcolor=blue"));
    assert!(style.contains("urlcolor=blue"));
    assert!(style.contains("citecolor=blue"));
    assert!(!style.contains("linkcolor=black"));
    assert!(style.contains("\\RequirePackage{geometry}"));
    assert!(style.contains("textwidth=6.5in"));
    assert!(style.contains("textheight=8.7in"));
    assert!(style.contains("\\Needspace*{4\\baselineskip}"));
    assert!(style.contains("\\Needspace*{6\\baselineskip}"));
    assert!(!style.contains("\\Needspace{4\\baselineskip}"));
    assert!(!style.contains("\\Needspace{6\\baselineskip}"));
    assert!(style.contains("\\newcount\\Litweb@codelinecount"));
    assert!(style.contains("\\ifnum\\Litweb@codelinecount<4"));
    assert_eq!(style.matches("vspace=\\smallskipamount").count(), 2);
    assert!(style.contains("\\par\\noindent\\textit{#1}\\par"));
    assert!(!style.contains("\\par\\smallskip\\noindent\\textit{#1}\\par"));
    assert!(style.contains("\\raise\\@tempdima\\box\\Litweb@mini"));
    assert!(style.contains("\\advance\\@tempdima by.5\\baselineskip"));
    assert!(style.contains("\\advance\\@tempdima by\\Litweb@bodydepth"));
    assert!(style.contains("\\raise\\@tempdimb\\box\\Litweb@body"));
    assert!(!style.contains("\\unvbox\\Litweb@body"));
    assert!(style.contains("\\newcommand{\\LitwebChaptersOpenRight}"));
    assert!(style.contains("\\newcommand{\\LitwebChaptersOpenLeft}"));
    assert!(style.contains("\\newcommand{\\LitwebPrepareLiterateMainMatter}"));
    assert!(style.contains("\\newcommand{\\LitwebLiterateChapterEnd}"));
    assert_eq!(style.matches("\\Litweb@updatesplitmarks").count(), 3);
    assert!(style.contains("\\setbox\\Litweb@trial=\\copy\\@cclv"));
    assert!(style.contains("\\unvbox\\@cclv\\vskip-\\@tempdima\\vfil"));
    assert!(style.contains(
        "\\newcommand{\\LitwebLiterateChapterEnd}{%\n  \\clearpage\n  \\ifodd\\c@page\n    \\Litweb@blankpage\n  \\fi\n"
    ));
    assert!(style.contains("  \\thispagestyle{empty}}\n\\makeatother"));
    assert_eq!(
        plan.outputs[0].bytes,
        include_bytes!("fixtures/latex/standalone/input.tex")
    );

    let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(latex.starts_with("\\documentclass[12pt]{article}\n"));
    assert!(latex.contains("\\input{litweb-latex/litweb.sty}"));
    assert!(latex.contains("\\LitwebSection{litweb-1-1}{1}"));
    assert!(latex.contains("\\href{\\detokenize{https://example.com/a_b?x=1&y=2}}"));
    assert!(latex.contains("[unsafe](javascript:alert)"));
    assert!(latex.contains("\\begin{aligned}"));
    assert!(latex.contains("\\begin{LitwebCode}"));
    assert!(latex.contains("\\LitwebMiniBlockUse{1}\\nobreak\n\\begin{tabularx}"));
    assert!(latex.contains("\\LitwebBackslash{}"));
    assert!(latex.contains("\\section*{Identifier Index}"));
    assert!(latex.ends_with("\\end{document}\n"));
}

#[test]
fn standalone_latex_is_deterministic_and_can_omit_the_identifier_index() {
    let first = standalone_plan();
    let second = standalone_plan();
    assert_eq!(first, second);

    let program = parse_str("demonstration.lit", STANDALONE).unwrap();
    let left_opening = plan_latex_with_options(
        &program,
        LatexOptions {
            chapter_opening: LatexChapterOpening::Left,
            ..LatexOptions::default()
        },
    )
    .unwrap();
    assert_eq!(first, left_opening);

    let plan = plan_latex_with_options(
        &program,
        LatexOptions {
            identifier_index: false,
            ..LatexOptions::default()
        },
    )
    .unwrap();
    let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(!latex.contains("Identifier Index"));
    assert!(!latex.contains("LitwebDeclareMiniIndexMeaning"));
    assert!(!latex.contains("LitwebMiniIndexBegin"));
    assert!(!latex.contains("LitwebMiniUse"));
    assert!(!latex.contains("LitwebCodeLine"));
    assert!(!latex.contains("LitwebMarkedCode"));
    assert!(!latex.contains("\\markboth"));
}

#[test]
fn book_index_running_headings_use_the_escaped_book_title_after_the_page_break() {
    let mut book = load_book(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/book/rust_shape/index.lit"),
    )
    .unwrap();
    book.title = "Routes & Rivers: 50% #1_{Guide} \\ Notes".to_owned();
    let heading = concat!(
        "{\\MakeUppercase{Routes \\& Rivers: 50\\% \\#1\\_\\{Guide\\} ",
        "\\textbackslash{} Notes --- Identifier Index}}"
    );
    let expected = format!(
        "\\chapter*{{Identifier Index}}\n\\markboth{heading}{heading}\n\
         \\addcontentsline{{toc}}{{chapter}}{{Identifier Index}}\n"
    );
    for chapter_opening in [LatexChapterOpening::Left, LatexChapterOpening::Right] {
        let plan = plan_latex_with_options(
            &book,
            LatexOptions {
                chapter_opening,
                ..LatexOptions::default()
            },
        )
        .unwrap();
        let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
        assert!(latex.contains(&expected), "missing {expected}");
        let index_start = latex.find("\\backmatter").unwrap();
        assert!(!latex[..index_start].contains("\\markboth"));
        assert_eq!(latex.matches("\\markboth").count(), 1);
        assert!(latex.contains("\\hyperlink{litweb-1-1}{1}"));
    }

    let no_index = plan_latex_with_options(
        &book,
        LatexOptions {
            identifier_index: false,
            ..LatexOptions::default()
        },
    )
    .unwrap();
    assert!(
        !String::from_utf8(no_index.outputs[0].bytes.clone())
            .unwrap()
            .contains("\\markboth")
    );

    let directory = TestDirectory::new("empty-index-headings");
    fs::write(
        directory.path().join("index.lit"),
        "@book\n@title Empty Index\n[One](one.lit)\n",
    )
    .unwrap();
    fs::write(directory.path().join("one.lit"), "@title One\n").unwrap();
    let empty_book = load_book(directory.path().join("index.lit")).unwrap();
    let empty_index = plan_latex(&empty_book).unwrap();
    let latex = String::from_utf8(empty_index.outputs[0].bytes.clone()).unwrap();
    assert!(!latex.contains("Identifier Index"));
    assert!(!latex.contains("\\markboth"));
    let standalone = standalone_plan();
    let latex = String::from_utf8(standalone.outputs[0].bytes.clone()).unwrap();
    assert!(latex.contains("\\section*{Identifier Index}"));
    assert!(!latex.contains("\\markboth"));
}

#[test]
fn standard_font_sizes_become_document_class_options() {
    let program = parse_str("demonstration.lit", STANDALONE).unwrap();
    for (font_size, opening) in [
        (LatexFontSize::TenPoint, "\\documentclass{article}\n"),
        (
            LatexFontSize::ElevenPoint,
            "\\documentclass[11pt]{article}\n",
        ),
        (
            LatexFontSize::TwelvePoint,
            "\\documentclass[12pt]{article}\n",
        ),
    ] {
        let plan = plan_latex_with_options(
            &program,
            LatexOptions {
                font_size,
                ..LatexOptions::default()
            },
        )
        .unwrap();
        let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
        assert!(latex.starts_with(opening));
    }
}

#[test]
fn rust_latex_emits_meaning_registry_and_exact_inert_page_markers() {
    let program = parse_str("mini-index.lit", MINI_INDEX).expect("fixture should parse");
    let plan = plan_latex(&program).expect("fixture should render as LaTeX");
    let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();

    assert!(latex.contains(
        "\\LitwebDeclareMiniIndexMeaning{1}{\\LitwebMiniIndexName{Alpha}: struct, \\hyperlink{litweb-1-1}{1}.}"
    ));
    assert!(latex.contains(
        "\\LitwebDeclareMiniIndexMeaning{11}{\\LitwebMiniIndexName{Theta}: struct"
    ));
    assert!(latex.contains("\\LitwebMiniIndexBegin\n"));
    assert!(latex.contains("uses \\texttt{\\LitwebMiniUse{1}Alpha}"));
    assert!(latex.contains(
        "\\texttt{\\LitwebMiniUse{1}Alpha}, \\texttt{\\LitwebMiniUse{2}Beta}, and \\texttt{\\LitwebMiniUse{7}Gamma}"
    ));
    assert!(latex.contains("\\LitwebCodeLine{}{\\LitwebCodeDefinition{1}}struct Alpha;"));
    assert!(
        latex
            .contains("{\\LitwebCodeDefinition{3}\\LitwebCodeDefinition{12}}fn combine(")
    );
    assert!(latex.contains("\\LitwebMiniIndexEnd\n"));

    let style = String::from_utf8(plan.outputs[1].bytes.clone()).unwrap();
    assert!(style.contains("\\splitbotmarks"));
    assert!(style.contains("\\vsplit\\@cclv"));
    assert!(style.contains("\\Litweb@resetmarks"));
}

#[test]
fn unsupported_languages_emit_no_identifier_or_mini_index_machinery() {
    let program = parse_str(
        "plain-c.lit",
        "@title Plain C\n@code_type c .c\n@s One\n--- example.c\nint main(void) { return 0; }\n---\n",
    )
    .unwrap();
    let plan = plan_latex(&program).unwrap();
    let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();

    assert!(!latex.contains("Identifier Index"));
    assert!(!latex.contains("LitwebDeclareMiniIndexMeaning"));
    assert!(!latex.contains("LitwebMiniIndexBegin"));
    assert!(!latex.contains("LitwebCodeLine"));
    assert!(!latex.contains("LitwebMarkedCode"));
}

#[test]
fn source_colorscheme_metadata_has_no_effect_on_latex() {
    let program = parse_str(
        "source-theme.lit",
        "@title Source Theme\n@colorscheme not-a-real-theme.css\n@s One\nText.\n",
    )
    .unwrap();
    let plan = plan_latex(&program).unwrap();
    let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(!latex.contains("not-a-real-theme"));
}

#[test]
fn an_unusable_name_has_a_structured_error() {
    let program = parse_str("/", "@title Invalid\n@s One\nText.\n").unwrap();
    let error = plan_latex(&program).unwrap_err();
    assert!(matches!(
        error,
        LatexPlanError::Latex(errors)
            if matches!(errors.as_slice()[0].kind, LatexErrorKind::InvalidOutputName)
    ));
}

#[test]
fn a_book_uses_one_document_with_front_matter_chapter_hierarchy_and_index() {
    let book = load_book(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/book/rust_shape/index.lit"),
    )
    .unwrap();
    let plan = plan_latex(&book).unwrap();
    assert_eq!(plan.outputs[0].relative_path, Path::new("index.tex"));
    let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(latex.starts_with("\\documentclass[12pt]{book}\n"));
    assert!(latex.contains("\\LitwebChaptersOpenRight\n"));
    assert!(latex.contains("\\frontmatter\n\\maketitle\n"));
    assert!(
        latex.contains("This introduction includes \\href{\\detokenize{guide.html}}")
    );
    assert!(latex.contains(
        "\\tableofcontents\n\\LitwebPrepareLiterateMainMatter\n\\mainmatter\n"
    ));
    assert!(latex.contains("\\LitwebLiterateChapter[Library]{\\hypertarget{litweb-chapter-1}{}Library Internals}"));
    assert!(
        latex.contains("\\section[Helpers]{\\hypertarget{litweb-chapter-2}{}Helpers}")
    );
    assert!(latex.contains("\\LitwebLiterateChapter[Application]{\\hypertarget{litweb-chapter-3}{}Application Page}"));
    assert_eq!(latex.matches("\\LitwebMiniIndexBegin\n").count(), 2);
    assert_eq!(latex.matches("\\LitwebMiniIndexEnd\n").count(), 2);
    assert_eq!(latex.matches("\\LitwebLiterateChapterEnd\n").count(), 2);
    assert!(latex.contains(
        "\\LitwebMiniIndexEnd\n\\LitwebLiterateChapterEnd\n\\LitwebLiterateChapter[Application]{\\hypertarget{litweb-chapter-3}{}Application Page}\n\\LitwebMiniIndexBegin\n"
    ));
    assert!(latex.contains("\\LitwebSection{litweb-2-1}{1}"));
    assert!(latex.contains("\\backmatter\n\\chapter*{Identifier Index}"));
    assert!(latex.contains("\\hyperlink{litweb-1-1}{1}"));
    assert!(latex.contains("\\hyperlink{litweb-1-2}{1.2}"));
    assert!(latex.contains("\\hyperlink{litweb-2-4}{4}"));
    assert!(latex.contains("\\hyperlink{litweb-2-5}{1.1.5}"));
    assert!(latex.contains("\\hyperlink{litweb-3-1}{2.1}"));

    let second = plan_latex(&book).unwrap();
    assert_eq!(plan, second);

    let left_opening = plan_latex_with_options(
        &book,
        LatexOptions {
            chapter_opening: LatexChapterOpening::Left,
            ..LatexOptions::default()
        },
    )
    .unwrap();
    let left_opening = String::from_utf8(left_opening.outputs[0].bytes.clone()).unwrap();
    assert!(left_opening.contains("\\LitwebChaptersOpenLeft\n"));
    assert!(!left_opening.contains("\\LitwebChaptersOpenRight\n"));

    let without_index = plan_latex_with_options(
        &book,
        LatexOptions {
            identifier_index: false,
            ..LatexOptions::default()
        },
    )
    .unwrap();
    let without_index =
        String::from_utf8(without_index.outputs[0].bytes.clone()).unwrap();
    assert!(!without_index.contains("\\backmatter"));
    assert!(!without_index.contains("Identifier Index"));
    assert!(!without_index.contains("LitwebDeclareMiniIndexMeaning"));
    assert!(!without_index.contains("LitwebMiniIndex"));
    assert!(without_index.contains("\\LitwebChaptersOpenRight\n"));
    assert_eq!(without_index.matches("\\LitwebLiterateChapter[").count(), 2);
    assert_eq!(
        without_index
            .matches("\\LitwebLiterateChapterEnd\n")
            .count(),
        2
    );
    assert!(
        without_index.contains("\\LitwebLiterateChapter[Library]{\\hypertarget{litweb-chapter-1}{}Library Internals}")
    );
    assert!(
        without_index
            .contains("\\section[Helpers]{\\hypertarget{litweb-chapter-2}{}Helpers}")
    );
    assert!(
        without_index.contains("\\LitwebLiterateChapter[Application]{\\hypertarget{litweb-chapter-3}{}Application Page}")
    );
}

#[test]
fn book_mini_indexes_follow_major_boundaries_but_not_minor_entries() {
    let book = load_book(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/latex/book_mini_index/index.lit"),
    )
    .unwrap();
    let plan = plan_latex(&book).unwrap();
    let latex = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(latex.contains(
        "\\LitwebMiniIndexName{EarlierDefinition}: struct, \\hyperlink{litweb-1-1}{1.1}."
    ));
    assert!(latex.contains(
        "\\LitwebMiniIndexName{FutureDefinition}: struct, \\hyperlink{litweb-3-1}{2.1}."
    ));
    assert!(latex.contains(
        "The first spread needs \\texttt{FutureDefinition}, whose declaration appears in"
    ));
    assert!(latex.contains(
        "This companion remains in the first major chapter. It uses\n\\texttt{FutureDefinition} again"
    ));
    assert!(!latex.contains("\\LitwebMiniUse{10}FutureDefinition"));
    assert!(latex.contains("uses \\texttt{\\LitwebMiniUse{5}EarlierDefinition}"));
    assert!(latex.contains(
        "\\LitwebMiniIndexName{AnExtremelyLongIdentifierThatMustWrapInsideOneMiniIndexColumn}"
    ));
    let style = String::from_utf8(plan.outputs[1].bytes.clone()).unwrap();
    assert!(style.contains("#1\\allowbreak{}"));
    assert_eq!(latex.matches("\\LitwebLiterateChapter[").count(), 3);
    assert_eq!(latex.matches("\\LitwebMiniIndexBegin\n").count(), 3);
    assert_eq!(latex.matches("\\LitwebMiniIndexEnd\n").count(), 3);
    assert_eq!(latex.matches("\\LitwebLiterateChapterEnd\n").count(), 3);
    assert!(latex.contains(
        "\\LitwebLiterateChapter[First]{\\hypertarget{litweb-chapter-1}{}First Major Chapter}\n\\LitwebMiniIndexBegin\n"
    ));
    assert!(latex.contains(
        "\\section[First companion]{\\hypertarget{litweb-chapter-2}{}First companion}"
    ));
    assert!(!latex.contains("\\LitwebLiterateChapter[First companion]"));
    assert!(latex.contains(
        "\\LitwebMiniIndexEnd\n\\LitwebLiterateChapterEnd\n\\LitwebLiterateChapter[Second]{\\hypertarget{litweb-chapter-3}{}Second Major Chapter}\n\\LitwebMiniIndexBegin\n"
    ));
    assert!(latex.contains(
        "\\LitwebMiniIndexEnd\n\\LitwebLiterateChapterEnd\n\\LitwebLiterateChapter[Third]{\\hypertarget{litweb-chapter-4}{}Third Major Chapter}\n\\LitwebMiniIndexBegin\n"
    ));
    assert!(latex.contains(
        "\\LitwebMiniIndexEnd\n\\LitwebLiterateChapterEnd\n\\backmatter\n\\chapter*{Identifier Index}"
    ));
    assert!(
        !latex
            .split("\\begin{document}")
            .next()
            .unwrap()
            .contains("LitwebMiniUse")
    );
}

#[test]
fn packaged_style_path_uses_the_complete_output_collision_check() {
    let program = parse_str(
        "collision.lit",
        "@title Collision\n@code_type text .sty\n@s One\n--- litweb-latex/litweb.sty\nbody\n---\n",
    )
    .unwrap();
    let mut outputs = plan_tangle(&program).unwrap().outputs;
    outputs.extend(plan_latex(&program).unwrap().outputs);
    let output_directory = std::env::temp_dir().join(format!(
        "litweb-latex-output-collision-{}",
        std::process::id()
    ));
    fs::create_dir_all(&output_directory).unwrap();

    let error = write_planned_outputs(
        &outputs,
        &output_directory,
        &PathBuf::from("collision.lit"),
    )
    .unwrap_err();
    assert_eq!(error.kind, OutputErrorKind::DuplicateDestination);
    assert!(fs::read_dir(&output_directory).unwrap().next().is_none());
    fs::remove_dir(output_directory).unwrap();
}

#[test]
fn prose_reference_failures_return_no_partial_latex_plan() {
    let program =
        parse_str("missing.lit", "@title Missing\n@s One\nSee @{Absent}.\n").unwrap();
    let error = plan_latex(&program).unwrap_err();
    assert!(matches!(
        error,
        LatexPlanError::Latex(errors)
            if matches!(
                &errors.as_slice()[0].kind,
                LatexErrorKind::UndefinedBlock { name } if name == "Absent"
            )
    ));
}

#[test]
fn cli_keeps_html_default_and_supports_weave_only_and_default_latex() {
    let directory = TestDirectory::new("cli-standalone");
    fs::write(directory.path().join("program.lit"), STANDALONE).unwrap();

    let default_html = lw_in(
        directory.path(),
        &["--weave", "--out-dir", "default-html", "program.lit"],
    );
    let explicit_html = lw_in(
        directory.path(),
        &[
            "--weave",
            "--format",
            "html",
            "--out-dir",
            "explicit-html",
            "program.lit",
        ],
    );
    assert!(default_html.status.success(), "{:?}", default_html.stderr);
    assert!(explicit_html.status.success(), "{:?}", explicit_html.stderr);
    assert_eq!(
        fs::read(directory.path().join("default-html/program.html")).unwrap(),
        fs::read(directory.path().join("explicit-html/program.html")).unwrap()
    );

    let weave_only = lw_in(
        directory.path(),
        &[
            "--weave",
            "--format",
            "latex",
            "--out-dir",
            "latex-only",
            "program.lit",
        ],
    );
    assert!(weave_only.status.success(), "{:?}", weave_only.stderr);
    assert!(directory.path().join("latex-only/program.tex").is_file());
    assert!(
        directory
            .path()
            .join("latex-only/litweb-latex/litweb.sty")
            .is_file()
    );
    assert!(!directory.path().join("latex-only/demo.rs").exists());

    let default_latex = lw_in(
        directory.path(),
        &[
            "--format",
            "latex",
            "--out-dir",
            "latex-and-code",
            "program.lit",
        ],
    );
    assert!(default_latex.status.success(), "{:?}", default_latex.stderr);
    assert!(
        directory
            .path()
            .join("latex-and-code/program.tex")
            .is_file()
    );
    assert!(directory.path().join("latex-and-code/demo.rs").is_file());

    let no_index = lw_in(
        directory.path(),
        &[
            "--weave",
            "--format",
            "latex",
            "--no-index",
            "--out-dir",
            "latex-no-index",
            "program.lit",
        ],
    );
    assert!(no_index.status.success(), "{:?}", no_index.stderr);
    let latex =
        fs::read_to_string(directory.path().join("latex-no-index/program.tex")).unwrap();
    assert!(!latex.contains("Identifier Index"));
}

#[test]
fn cli_generates_one_portable_latex_document_for_a_book() {
    let directory = TestDirectory::new("cli-book");
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/book/rust_shape/index.lit");
    let output = Command::new(env!("CARGO_BIN_EXE_lw"))
        .current_dir(directory.path())
        .args([
            "--weave",
            "--format",
            "latex",
            "--font-size",
            "12pt",
            "--chapter-opening",
            "left",
            "--out-dir",
            "book",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let latex = fs::read_to_string(directory.path().join("book/index.tex")).unwrap();
    assert!(latex.starts_with("\\documentclass[12pt]{book}\n"));
    assert!(latex.contains("\\LitwebChaptersOpenLeft\n"));
    assert!(latex.contains("\\LitwebLiterateChapter[Library]{\\hypertarget{litweb-chapter-1}{}Library Internals}"));
    assert!(
        latex.contains("\\section[Helpers]{\\hypertarget{litweb-chapter-2}{}Helpers}")
    );
    assert!(latex.contains("\\hyperlink{litweb-1-2}{1.2}"));
    assert!(!latex.contains(env!("CARGO_MANIFEST_DIR")));
    assert!(!latex.contains("reference_repos"));
    assert!(!latex.contains("/Users/"));
}

#[test]
fn cli_rejects_a_combined_latex_style_collision_before_writing() {
    let directory = TestDirectory::new("cli-collision");
    fs::write(
        directory.path().join("collision.lit"),
        "@title Collision\n@code_type text .sty\n@s One\n--- litweb-latex/litweb.sty\nbody\n---\n",
    )
    .unwrap();
    let output = lw_in(
        directory.path(),
        &[
            "--format",
            "latex",
            "--out-dir",
            "generated",
            "collision.lit",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("more than one generated file has this destination")
    );
    assert!(!directory.path().join("generated").exists());
}
