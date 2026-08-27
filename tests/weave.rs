use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use litweb::parser::parse_str;
use litweb::weaver::{
    WeaveErrorKind, WeaveOptions, WeavePlanError, plan_weave, plan_weave_with_options,
};

const MINIMAL: &str = include_str!("fixtures/weave/minimal/input.lit");
const MARKDOWN: &str = include_str!("fixtures/weave/markdown/input.lit");
const SECTIONS: &str = include_str!("fixtures/weave/sections/input.lit");
const DOCUMENTED_GREETING: &str = include_str!("fixtures/documented/greeting.lit");
const DOCUMENTED_NOTES_GRAPH: &str = include_str!("fixtures/documented/notes_graph.lit");
const DOCUMENTED_ENHANCED_PAGE: &str =
    include_str!("fixtures/documented/enhanced_page.lit");

fn weave(file: &str, source: &str) -> Vec<u8> {
    let program = parse_str(file, source).expect("fixture should parse");
    let plan = plan_weave(&program).expect("fixture should weave");
    plan.outputs
        .into_iter()
        .find(|output| {
            output
                .relative_path
                .extension()
                .is_some_and(|ext| ext == "html")
        })
        .expect("a single-file weave should contain its HTML page")
        .bytes
}

fn weave_without_index(file: &str, source: &str) -> Vec<u8> {
    let program = parse_str(file, source).expect("fixture should parse");
    let plan = plan_weave_with_options(
        &program,
        WeaveOptions {
            identifier_index: false,
            color_scheme: Some(PathBuf::from("none")),
        },
    )
    .expect("fixture should weave");
    plan.outputs
        .into_iter()
        .find(|output| {
            output
                .relative_path
                .extension()
                .is_some_and(|ext| ext == "html")
        })
        .expect("a single-file weave should contain its HTML page")
        .bytes
}

fn weave_unhighlighted(file: &str, source: &str) -> Vec<u8> {
    let program = parse_str(file, source).expect("fixture should parse");
    let plan = plan_weave_with_options(
        &program,
        WeaveOptions {
            identifier_index: true,
            color_scheme: Some(PathBuf::from("none")),
        },
    )
    .expect("fixture should weave");
    plan.outputs
        .into_iter()
        .find(|output| {
            output
                .relative_path
                .extension()
                .is_some_and(|ext| ext == "html")
        })
        .expect("a single-file weave should contain its HTML page")
        .bytes
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "litweb-weave-{label}-{}-{number}",
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
fn reviewed_html_goldens_match_byte_for_byte() {
    for (file, source, expected) in [
        (
            "minimal.lit",
            MINIMAL,
            include_bytes!("fixtures/weave/minimal/input.html").as_slice(),
        ),
        (
            "markdown.lit",
            MARKDOWN,
            include_bytes!("fixtures/weave/markdown/input.html").as_slice(),
        ),
        (
            "sections.lit",
            SECTIONS,
            include_bytes!("fixtures/weave/sections/input.html").as_slice(),
        ),
    ] {
        assert_eq!(weave_unhighlighted(file, source), expected, "{file}");
    }
}

#[test]
fn documented_greeting_weave_keeps_all_authored_relationships() {
    let html =
        String::from_utf8(weave_without_index("greeting.lit", DOCUMENTED_GREETING))
            .unwrap();

    assert_eq!(html.matches("<section class=\"section\" id=").count(), 5);
    for section in 1..=5 {
        assert!(
            html.contains(&format!("id=\"1:{section}\"")),
            "missing presentation section {section}"
        );
    }
    assert!(html.contains("<span class=\"section-title\">Greeting</span>."));
    assert_eq!(
        html.matches("aria-label=\"is continued by\">+≡</span>")
            .count(),
        2
    );
    assert_eq!(
        html.matches("aria-label=\"is replaced by\">:=</span>")
            .count(),
        1
    );
    assert!(html.contains(
        "<span class=\"nocode\">⟨Greeting line <a href=\"#1:2\" \
         aria-label=\"section 2\">2</a>⟩</span>"
    ));
    assert!(html.contains("<p class=\"seealso\">See also sections "));
    assert!(html.contains("This code is replaced in section "));
    assert!(html.contains("This code is used in section "));
}

#[test]
fn documented_graph_occurrences_share_one_definition_location() {
    let html = String::from_utf8(weave_unhighlighted(
        "notes_graph.lit",
        DOCUMENTED_NOTES_GRAPH,
    ))
    .unwrap();

    assert_eq!(html.matches("<section class=\"section\" id=").count(), 1);
    assert!(html.contains(
        "<dt><code>Graph</code></dt>\n\
         <dd><a class=\"identifier-definition\" href=\"#1:1\" \
         aria-label=\"definition in section 1\">1</a></dd>"
    ));
}

#[test]
fn documented_enhanced_page_preserves_fallbacks_and_plans_both_asset_groups() {
    let program = parse_str("distance.lit", DOCUMENTED_ENHANCED_PAGE).unwrap();
    let plan = plan_weave_with_options(
        &program,
        WeaveOptions {
            identifier_index: false,
            color_scheme: None,
        },
    )
    .unwrap();

    assert_eq!(plan.outputs[0].relative_path, Path::new("distance.html"));
    let math_start = plan
        .outputs
        .iter()
        .position(|output| {
            output
                .relative_path
                .starts_with("litweb-assets/katex-0.18.1")
        })
        .unwrap();
    let highlight_start = plan
        .outputs
        .iter()
        .position(|output| {
            output
                .relative_path
                .starts_with("litweb-assets/prism-1.30.0")
        })
        .unwrap();
    assert_eq!(math_start, 1);
    assert!(highlight_start > math_start);
    assert!(plan.outputs[1..highlight_start].iter().all(|output| {
        output
            .relative_path
            .starts_with("litweb-assets/katex-0.18.1")
    }));
    assert!(plan.outputs[highlight_start..].iter().all(|output| {
        output
            .relative_path
            .starts_with("litweb-assets/prism-1.30.0")
    }));

    let html = std::str::from_utf8(&plan.outputs[0].bytes).unwrap();
    assert!(html.contains(
        "<span class=\"litweb-math litweb-math-inline\">d^2 = x^2 + y^2</span>"
    ));
    assert!(html.contains(
        "<code class=\"section-reference\">⟨Squared distance <a href=\"#1:1\""
    ));
    assert!(html.contains("fn squared_distance(x: f64, y: f64) -&gt; f64 {"));
    assert!(html.contains("data-litweb-highlight=\"pending\""));
    assert!(html.contains(
        "<html lang=\"en\" data-litweb-ready=\"pending\" \
         data-litweb-math-ready=\"pending\" data-litweb-highlight-ready=\"pending\">"
    ));
}

#[test]
fn fenced_prose_preserves_literal_text_and_reuses_highlighting_assets() {
    let source = r#"@code_type rust .rs
@title Fenced prose

@s Examples
The examples below are prose rather than tangled blocks.

  ````rust extra words
  fn main() {
    println!("<hello & goodbye>");
  }
  `````

~~~text
@code{not_an_identifier()} @{Missing} $not_math$ <b>not HTML</b>
~~~
"#;
    let program = parse_str("fenced.lit", source).unwrap();
    let plan = plan_weave(&program).unwrap();
    let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();

    assert!(html.contains(
        "<pre class=\"language-rust\"><code class=\"language-rust\" \
         data-litweb-highlight=\"pending\" data-litweb-language=\"rust\">\
fn main() {\n  println!(\"&lt;hello &amp; goodbye&gt;\");\n}\n</code></pre>"
    ));
    assert!(html.contains(
        "<pre><code>@code{not_an_identifier()} @{Missing} $not_math$ \
         &lt;b&gt;not HTML&lt;/b&gt;\n</code></pre>"
    ));
    assert!(!html.contains("litweb-math-inline"));
    assert!(!html.contains("section-reference\">⟨Missing"));
    assert!(
        plan.outputs
            .iter()
            .any(|output| output.relative_path.ends_with("litweb-highlight.js"))
    );

    let unhighlighted = plan_weave_with_options(
        &program,
        WeaveOptions {
            identifier_index: true,
            color_scheme: Some(PathBuf::from("none")),
        },
    )
    .unwrap();
    assert_eq!(unhighlighted.outputs.len(), 1);
    let html = String::from_utf8(unhighlighted.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains("<pre class=\"language-rust\"><code class=\"language-rust\">"));
    assert!(!html.contains("data-litweb-highlight"));
}

#[test]
fn pipe_tables_render_semantic_located_inline_cells() {
    let source = r#"@code_type rust .rs
@title Tables
@s Forms
Name | Center | Number
:--- | :---: | ---:
*alpha* | [guide](guide.html) | 1
`a|b` | @code{left | right} | $|x|$
escaped \| pipe | @{Piece} | <unsafe & text>
[bad](javascript:bad) | short
extra | middle | 3 | ignored

--- Piece
let piece = 1;
---
"#;
    let html = String::from_utf8(weave("tables.lit", source)).unwrap();

    assert!(html.contains(
        "<div class=\"table-scroll\">\n<table>\n<thead>\n<tr>\n\
         <th scope=\"col\" class=\"table-align-left\">Name</th>\n\
         <th scope=\"col\" class=\"table-align-center\">Center</th>\n\
         <th scope=\"col\" class=\"table-align-right\">Number</th>\n"
    ));
    assert!(html.contains("<td class=\"table-align-left\"><em>alpha</em></td>"));
    assert!(html.contains(
        "<td class=\"table-align-center\"><a href=\"guide.html\">guide</a></td>"
    ));
    assert!(html.contains("<td class=\"table-align-left\"><code>a|b</code></td>"));
    assert!(html.contains(
        "<td class=\"table-align-center\"><code class=\"source-code\">left | right</code></td>"
    ));
    assert!(html.contains(
        "<td class=\"table-align-right\"><span class=\"litweb-math \
         litweb-math-inline\">|x|</span></td>"
    ));
    assert!(html.contains("<td class=\"table-align-left\">escaped | pipe</td>"));
    assert!(html.contains(
        "<td class=\"table-align-center\"><code class=\"section-reference\">⟨Piece "
    ));
    assert!(html.contains("&lt;unsafe &amp; text&gt;"));
    assert!(html.contains("[bad](javascript:bad)"));
    assert!(!html.contains("href=\"javascript:"));
    assert!(html.contains(
        "<td class=\"table-align-center\">short</td>\n\
         <td class=\"table-align-right\"></td>"
    ));
    assert!(!html.contains(">ignored<"));
    assert_eq!(html.matches("<td class=\"table-align-").count(), 15);
    assert!(html.contains("</tbody>\n</table>\n</div>"));
}

#[test]
fn table_width_and_block_transitions_follow_the_supported_subset() {
    let source = r#"@title Table boundaries
@s Header only
| One | Two |
| --- | --- |

@s Mismatch
A | B
--- | --- | ---
ordinary paragraph

@s Transitions
Kind | Meaning
--- | ---
row | value
- list begins | and remains a list item
```text
fixed | prose
```
$$
x | y
$$
following paragraph
"#;
    let html = String::from_utf8(weave("table-boundaries.lit", source)).unwrap();

    let header_only = html
        .split("<section class=\"section\" id=\"1:1\">")
        .nth(1)
        .unwrap()
        .split("</section>")
        .next()
        .unwrap();
    assert!(header_only.contains("<thead>"));
    assert!(!header_only.contains("<tbody>"));
    assert!(html.contains("<p>A | B\n--- | --- | ---\nordinary paragraph</p>"));
    assert!(html.contains(
        "</table>\n</div>\n<ul>\n<li>list begins | and remains a list item</li>\n</ul>\n\
         <pre><code>fixed | prose\n</code></pre>\n\
         <div class=\"litweb-math litweb-math-display\">x | y</div>\n\
         <p>following paragraph</p>"
    ));
}

#[test]
fn block_first_lists_fences_and_tables_remain_below_section_headings() {
    let source = r#"@title Block openings
@s Ordered
1. first
2. second

@s Fence
```text
fixed
```

@s Table
Name | Meaning
--- | ---
one | first
"#;
    let html = String::from_utf8(weave("block-openings.lit", source)).unwrap();

    assert!(html.contains(
        "<h2 class=\"section-heading\"><span class=\"section-number\">1.</span> \
         <span class=\"section-title\">Ordered</span>.</h2>\n<ol>"
    ));
    assert!(html.contains(
        "<h2 class=\"section-heading\"><span class=\"section-number\">2.</span> \
         <span class=\"section-title\">Fence</span>.</h2>\n<pre><code>fixed\n</code></pre>"
    ));
    assert!(html.contains(
        "<h2 class=\"section-heading\"><span class=\"section-number\">3.</span> \
         <span class=\"section-title\">Table</span>.</h2>\n<div class=\"table-scroll\">"
    ));
    assert!(!html.contains(
        "<div class=\"section-opening\">\n<h2 class=\"section-heading\"><span \
         class=\"section-number\">1.</span> <span class=\"section-title\">Ordered"
    ));
}

#[test]
fn unclosed_fence_does_not_turn_the_remaining_prose_into_code() {
    let source = r#"@code_type rust .rs
@title Unclosed fence
@s Example
```rust
The later @code{still_prose()} remains ordinary prose.
"#;
    let html = String::from_utf8(weave("unclosed-fence.lit", source)).unwrap();

    assert!(!html.contains("<pre"));
    assert!(html.contains("<code class=\"source-code\">still_prose()</code>"));
    assert!(html.contains("<dt><code>still_prose</code></dt>"));
}

#[test]
fn wrapped_list_references_keep_their_physical_source_origin() {
    let source = "\
@title Located list
@s Example
- the reference appears
  on this @{Missing list line}.
";
    let program = parse_str("located-list.lit", source).unwrap();
    let error = plan_weave(&program).expect_err("weaving should fail");
    let WeavePlanError::Weave(errors) = error else {
        panic!("expected weave errors");
    };

    assert_eq!(errors.as_slice().len(), 1);
    assert_eq!(errors.as_slice()[0].origin.line, 4);
    assert_eq!(
        errors.as_slice()[0].kind,
        WeaveErrorKind::UndefinedBlock {
            name: "Missing list line".to_owned(),
        }
    );
}

#[test]
fn table_cell_references_keep_their_physical_source_origin() {
    let source = "\
@title Located table
@s Example
Place | Reference
--- | ---
second row | @{Missing table cell}
";
    let program = parse_str("located-table.lit", source).unwrap();
    let error = plan_weave(&program).expect_err("weaving should fail");
    let WeavePlanError::Weave(errors) = error else {
        panic!("expected weave errors");
    };

    assert_eq!(errors.as_slice().len(), 1);
    assert_eq!(errors.as_slice()[0].origin.line, 5);
    assert_eq!(
        errors.as_slice()[0].kind,
        WeaveErrorKind::UndefinedBlock {
            name: "Missing table cell".to_owned(),
        }
    );
}

#[test]
fn a_rust_document_appends_a_deduplicated_identifier_index() {
    let source = r#"@code_type rust .rs
@title Indexed Rust

@s Definition
The prose mentions @code{helper()} in the defining section.
--- indexed.rs
fn helper(value: usize) -> usize {
    value
}
---

@s Use
--- Call helper
helper(1);
---
"#;
    let html = String::from_utf8(weave("indexed.lit", source)).unwrap();

    assert!(html.contains(
        "<section class=\"identifier-index\" aria-labelledby=\"identifier-index-title\">"
    ));
    assert!(html.contains("<h2 id=\"identifier-index-title\">Identifier Index</h2>"));
    assert!(html.contains(
        "<dt><code>helper</code></dt>\n<dd><a class=\"identifier-definition\" href=\"#1:1\" aria-label=\"definition in section 1\">1</a>, <a href=\"#1:2\" aria-label=\"section 2\">2</a></dd>"
    ));
    assert!(html.contains(
        "<dt><code>value</code></dt>\n<dd><a class=\"identifier-definition\" href=\"#1:1\" aria-label=\"definition in section 1\">1</a></dd>"
    ));
    assert_eq!(html.matches("<dt><code>helper</code>").count(), 1);
    assert!(!html.contains("<dt><code>usize</code>"));
    assert!(html.contains("font-family: Arial, Helvetica, sans-serif;"));
    assert!(html.contains("font-variant-numeric: lining-nums tabular-nums;"));
    assert!(html.contains(".identifier-index dd {\n    font-size: 0.875em;\n}"));
    assert!(html.contains(".identifier-index a {\n    text-decoration: none;\n}"));
    assert!(html.contains(
        ".identifier-index a.identifier-definition {\n    text-decoration: underline;"
    ));
    assert!(html.contains("text-decoration-skip-ink: none;"));
    assert!(
        html.find("id=\"1:2\"").unwrap() < html.find("identifier-index-title").unwrap()
    );
}

#[test]
fn a_cross_block_field_definition_is_underlined_at_its_visible_section() {
    let source = r#"@code_type rust .rs
@s Graph
--- graph.rs
struct Graph {
    @{Graph fields}
}
---
--- Graph fields
@{Stored field}
---
--- Stored field
vertex_count: usize,
---
"#;
    let html = String::from_utf8(weave("graph.lit", source)).unwrap();

    assert!(html.contains(
        "<dt><code>vertex_count</code></dt>\n<dd><a class=\"identifier-definition\""
    ));
    assert_eq!(html.matches("<dt><code>vertex_count</code>").count(), 1);
}

#[test]
fn identifier_index_options_and_unsupported_languages_emit_no_index_markup() {
    let rust = r#"@code_type rust .rs
@s Rust
--- indexed.rs
fn indexed_name() {}
---
"#;
    let disabled = String::from_utf8(weave_without_index("disabled.lit", rust)).unwrap();
    assert!(!disabled.contains("identifier-index"));
    assert!(!disabled.contains("Identifier Index"));

    let c = r#"@code_type c .c
@s C
--- plain.c
int plain_name(void) { return 0; }
---
"#;
    let unsupported = String::from_utf8(weave("plain.lit", c)).unwrap();
    assert!(!unsupported.contains("identifier-index"));
    assert!(!unsupported.contains("Identifier Index"));
}

#[test]
fn no_index_cli_option_suppresses_only_the_woven_index() {
    let directory = TestDirectory::new("no-index");
    let input = directory.path().join("program.lit");
    let output_directory = directory.path().join("generated");
    fs::write(
        &input,
        r#"@code_type rust .rs
@s Program
--- program.rs
fn generated_name() {}
---
"#,
    )
    .unwrap();

    let output = lw(&[
        Path::new("--no-index"),
        Path::new("-odir"),
        &output_directory,
        &input,
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(output_directory.join("program.rs")).unwrap(),
        "fn generated_name() {}\n"
    );
    let html = fs::read_to_string(output_directory.join("program.html")).unwrap();
    assert!(!html.contains("Identifier Index"));
    assert!(!html.contains("identifier-index"));
    assert!(html.contains("fn generated_name() {}"));
}

#[test]
fn presentation_sections_follow_the_frozen_split_contract() {
    let html = String::from_utf8(weave("sections.lit", SECTIONS)).unwrap();

    assert_eq!(html.matches("<section class=\"section\"").count(), 22);
    assert_eq!(html.matches("<h2 class=\"section-heading").count(), 22);
    assert_eq!(html.matches("section-heading-untitled").count(), 10);
    assert_eq!(html.matches("<div class=\"section-opening\">").count(), 19);
    assert_eq!(
        html.matches("<div class=\"codeblock codeblock-first\">")
            .count(),
        9
    );
    assert!(!html.contains("<h4>"));
    for section in 1..=22 {
        assert!(html.contains(&format!("id=\"1:{section}\"")), "{section}");
    }
    assert!(!html.contains("id=\"1:23\""));

    for (section, title) in [
        (1, "Empty explicit section"),
        (3, "Prose only"),
        (4, "Code only"),
        (5, "Prose and code"),
        (6, "Prose after code"),
        (8, "Prose and consecutive code"),
        (10, "Code prose code"),
        (12, "Three code blocks"),
        (15, "Hidden blocks and commands"),
        (16, "Hidden code between prose blocks"),
        (18, "Hidden-only explicit section"),
        (19, "Relationships"),
    ] {
        assert!(html.contains(&format!(
            "<span class=\"section-number\">{section}.</span> <span class=\"section-title\">{title}</span>"
        )));
    }
    assert!(html.contains(
        "<span class=\"section-number\">2.</span> <span class=\"visually-hidden\">Untitled section</span>"
    ));
    assert!(html.contains(
        "<div class=\"section-opening\">\n<h2 class=\"section-heading\"><span class=\"section-number\">3.</span> <span class=\"section-title\">Prose only</span>.</h2>\n<p>This section contains prose and no code.</p>\n</div>"
    ));
    assert!(html.contains(
        "<section class=\"section\" id=\"1:4\">\n<div class=\"codeblock codeblock-first\">\n<div class=\"section-opening\">\n<h2 class=\"section-heading\"><span class=\"section-number\">4.</span> <span class=\"section-title\">Code only</span>.</h2>\n<div class=\"codeblock_name\">"
    ));

    assert!(!html.contains("hidden_base();"));
    assert!(!html.contains("hidden_addition();"));
    assert!(!html.contains("hidden_separator();"));
    assert!(!html.contains("not_rendered();"));
    assert!(html.contains("visible();"));

    assert_eq!(html.matches("class=\"definition-operator\"").count(), 15);
    assert!(html.contains(
        "<code class=\"section-reference\">⟨Target <a href=\"#1:19\" aria-label=\"section 19\">19</a>⟩</code>"
    ));
    assert!(html.contains(
        "<span class=\"section-name\">⟨Target <a href=\"#1:19\" aria-label=\"section 19\">19</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is defined as\">≡</span>"
    ));
    assert!(html.contains(
        "<span class=\"section-name\">⟨Target <a href=\"#1:19\" aria-label=\"section 19\">19</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is continued by\">+≡</span>"
    ));
    assert!(html.contains(
        "<span class=\"section-name\">⟨Target <a href=\"#1:19\" aria-label=\"section 19\">19</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is replaced by\">:=</span>"
    ));
    assert!(html.contains(
        "<span class=\"nocode\">⟨Target <a href=\"#1:19\" aria-label=\"section 19\">19</a>⟩</span>"
    ));
    assert!(html.contains(
        "See also section <a href=\"#1:20\" aria-label=\"section 20\">20</a>."
    ));
    assert!(html.contains(
        "This code is replaced in section <a href=\"#1:21\" aria-label=\"section 21\">21</a>."
    ));
    assert!(html.contains(
        "This code is used in section <a href=\"#1:22\" aria-label=\"section 22\">22</a>."
    ));
    assert!(!html.contains("Added to in section"));
    assert!(!html.contains("Used in section"));
    assert!(!html.contains("Redefined in section"));
    assert!(!html.contains("content:"));
}

#[test]
fn section_openings_flow_into_the_first_supported_content() {
    let source = r#"@title Opening forms
@s Paragraph title
First paragraph.

Second paragraph.
--- Later code
later();
---
@s Question?
- alpha
- beta

A paragraph after the list.
@s Code title!
--- output.txt
base
---
--- output.txt +=
addition
---
--- output.txt :=
replacement
---
@s Already.
@s Hidden then visible
--- Hidden --- noWeave
hidden
---
--- A deliberately long visible code block name that can wrap on narrow screens
visible
---
@s Literal Markdown
# unsupported heading syntax remains literal paragraph text.
"#;
    let html = String::from_utf8(weave("openings.lit", source)).unwrap();

    assert_eq!(html.matches("<section class=\"section\"").count(), 8);
    assert_eq!(html.matches("<div class=\"section-opening\">").count(), 6);
    assert_eq!(
        html.matches("<div class=\"codeblock codeblock-first\">")
            .count(),
        4
    );
    assert!(html.contains(
        "<div class=\"section-opening\">\n<h2 class=\"section-heading\"><span class=\"section-number\">1.</span> <span class=\"section-title\">Paragraph title</span>.</h2>\n<p>First paragraph.</p>\n</div>\n<p>Second paragraph.</p>\n<div class=\"codeblock\">"
    ));
    assert!(html.contains(
        "<section class=\"section\" id=\"1:2\">\n<h2 class=\"section-heading\"><span class=\"section-number\">2.</span> <span class=\"section-title\">Question?</span></h2>\n<ul>\n<li>alpha</li>\n<li>beta</li>\n</ul>\n<p>A paragraph after the list.</p>"
    ));
    assert!(html.contains(
        "<h2 class=\"section-heading\"><span class=\"section-number\">3.</span> <span class=\"section-title\">Code title!</span></h2>\n<div class=\"codeblock_name\"><span class=\"section-name\">⟨<strong>output.txt</strong>"
    ));
    assert!(html.contains(
        "<span class=\"section-number\">4.</span> <span class=\"visually-hidden\">Untitled section</span></h2>\n<div class=\"codeblock_name\"><span class=\"section-name\">⟨<strong>output.txt</strong> <a href=\"#1:3\" aria-label=\"section 3\">3</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is continued by\">+≡</span>"
    ));
    assert!(html.contains(
        "<span class=\"section-number\">5.</span> <span class=\"visually-hidden\">Untitled section</span></h2>\n<div class=\"codeblock_name\"><span class=\"section-name\">⟨<strong>output.txt</strong> <a href=\"#1:3\" aria-label=\"section 3\">3</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is replaced by\">:=</span>"
    ));
    assert!(html.contains(
        "<section class=\"section\" id=\"1:6\">\n<h2 class=\"section-heading\"><span class=\"section-number\">6.</span> <span class=\"section-title\">Already.</span></h2>\n</section>"
    ));
    assert!(html.contains(
        "<span class=\"section-title\">Hidden then visible</span>.</h2>\n<div class=\"codeblock_name\"><span class=\"section-name\">⟨A deliberately long visible code block name that can wrap on narrow screens"
    ));
    assert!(!html.contains("hidden\n"));
    assert!(html.contains(
        "<span class=\"section-title\">Literal Markdown</span>.</h2>\n<p># unsupported heading syntax remains literal paragraph text.</p>"
    ));
}

#[test]
fn weaving_is_deterministic_and_has_no_external_presentation_assets() {
    let first = weave("minimal.lit", MINIMAL);
    let second = weave("minimal.lit", MINIMAL);
    assert_eq!(first, second);

    let html = String::from_utf8(first).unwrap();
    assert!(!html.contains("<script"));
    assert!(!html.contains("<link"));
    assert!(!html.contains("http://"));
    assert!(!html.contains("https://"));
    assert!(html.contains(
        "<span class=\"section-name\">⟨<strong>minimal.txt</strong> <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is defined as\">≡</span>"
    ));
    assert!(html.contains(
        ".section-name a,\n.section-reference a,\n.nocode a,\n.seealso a {\n    text-decoration: none;\n}"
    ));
    assert!(html.contains(
        ".section-name a:focus-visible,\n.section-reference a:focus-visible,\n.nocode a:focus-visible,\n.seealso a:focus-visible {\n    outline: 2px solid currentColor;\n    outline-offset: 2px;\n}"
    ));
}

#[test]
fn definitions_changes_and_uses_link_in_source_order() {
    let source = "\
@title Relationships
@s Definition
--- Piece
one
---
@s Addition
--- Piece +=
two
---
@s Redefinition
--- Piece :=
three
---
@s Use
--- output.txt
@{Piece}
---
";
    let html = String::from_utf8(weave("relationships.lit", source)).unwrap();

    for anchor in ["1:1", "1:2", "1:3", "1:4"] {
        assert!(html.contains(&format!("id=\"{anchor}\"")));
    }
    assert!(html.contains(
        "⟨Piece <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is continued by\">+≡</span>"
    ));
    assert!(html.contains(
        "⟨Piece <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</span> <span class=\"definition-operator\" aria-label=\"is replaced by\">:=</span>"
    ));
    assert!(html.contains(
        "<span class=\"nocode\">⟨Piece <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</span>"
    ));
    assert!(
        html.contains(
            "See also section <a href=\"#1:2\" aria-label=\"section 2\">2</a>."
        )
    );
    assert!(html.contains(
        "This code is replaced in section <a href=\"#1:3\" aria-label=\"section 3\">3</a>."
    ));
    assert!(html.contains(
        "This code is used in section <a href=\"#1:4\" aria-label=\"section 4\">4</a>."
    ));
}

#[test]
fn relationship_sentences_use_cweb_list_grammar_and_unique_sections() {
    let source = "\
@title Relationship grammar
@s Three locations
--- Target
base
---
--- Target +=
add_one
---
--- Target +=
add_two
---
--- Target +=
add_three
---
--- Target :=
replace_one
---
--- Target :=
replace_two
---
--- Target :=
replace_three
---
--- Use one
@{Target}
@{Target}
---
--- Use two
@{Target}
---
--- Use three
@{Target}
---
@s Two locations
--- Pair
base
---
--- Pair user one
@{Pair}
---
--- Pair user two
@{Pair}
---
@s One location
--- Single
base
---
--- Single user
@{Single}
@{Single}
---
";
    let html = String::from_utf8(weave("relationship-grammar.lit", source)).unwrap();

    assert!(html.contains(
        "<p class=\"seealso\">See also sections <a href=\"#1:2\" aria-label=\"section 2\">2</a>, <a href=\"#1:3\" aria-label=\"section 3\">3</a>, and <a href=\"#1:4\" aria-label=\"section 4\">4</a>.</p>"
    ));
    assert!(html.contains(
        "<p class=\"seealso\">This code is replaced in sections <a href=\"#1:5\" aria-label=\"section 5\">5</a>, <a href=\"#1:6\" aria-label=\"section 6\">6</a>, and <a href=\"#1:7\" aria-label=\"section 7\">7</a>.</p>"
    ));
    assert!(html.contains(
        "<p class=\"seealso\">This code is used in sections <a href=\"#1:8\" aria-label=\"section 8\">8</a>, <a href=\"#1:9\" aria-label=\"section 9\">9</a>, and <a href=\"#1:10\" aria-label=\"section 10\">10</a>.</p>"
    ));
    assert!(html.contains(
        "<p class=\"seealso\">See also sections <a href=\"#1:3\" aria-label=\"section 3\">3</a> and <a href=\"#1:4\" aria-label=\"section 4\">4</a>.</p>"
    ));
    assert!(html.contains(
        "<p class=\"seealso\">This code is used in sections <a href=\"#1:12\" aria-label=\"section 12\">12</a> and <a href=\"#1:13\" aria-label=\"section 13\">13</a>.</p>"
    ));
    assert!(html.contains(
        "<p class=\"seealso\">This code is used in section <a href=\"#1:15\" aria-label=\"section 15\">15</a>.</p>"
    ));
    assert!(!html.contains("(twice)"));
}

#[test]
fn input_text_is_escaped_and_unsafe_links_are_not_activated() {
    let source = r#"@title <unsafe & title>
@s <section>
Raw <b>markup</b> & [bad](javascript:alert(1)).
The @{λ ⟨inner⟩ <tag> & quoted} name remains safe.
--- λ ⟨inner⟩ <tag> & quoted
helper text
---
--- output.txt
@{λ ⟨inner⟩ <tag> & quoted}
<tag attr="value"> & text
---
"#;
    let html = String::from_utf8(weave("escaping.lit", source)).unwrap();

    assert!(html.contains("<title>&lt;unsafe &amp; title&gt;</title>"));
    assert!(html.contains("&lt;section&gt;"));
    assert!(
        html.contains("Raw &lt;b&gt;markup&lt;/b&gt; &amp; [bad](javascript:alert(1)).")
    );
    assert!(!html.contains("href=\"javascript:"));
    assert!(html.contains(
        "<code class=\"section-reference\">⟨λ ⟨inner⟩ &lt;tag&gt; &amp; quoted <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</code>"
    ));
    assert!(html.contains(
        "<span class=\"section-name\">⟨λ ⟨inner⟩ &lt;tag&gt; &amp; quoted <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</span>"
    ));
    assert!(html.contains(
        "<span class=\"nocode\">⟨λ ⟨inner⟩ &lt;tag&gt; &amp; quoted <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</span>"
    ));
    assert!(html.contains("&lt;tag attr=\"value\"&gt; &amp; text"));
}

#[test]
fn language_classes_and_literal_markdown_forms_are_explicit() {
    let source = r#"@code_type rust .rs
@title Literal forms
@s One
under_score, \*literal asterisks\*, and \$x^2\$ stay literal.
--- output.rs
fn main() {}
---
"#;
    let html = String::from_utf8(weave("literal.lit", source)).unwrap();

    assert!(html.contains(
        "<pre class=\"language-rust\"><code class=\"language-rust\" \
         data-litweb-highlight=\"pending\" data-litweb-language=\"rust rs\">"
    ));
    assert!(html.contains("under_score, *literal asterisks*, and $x^2$ stay literal."));
    assert!(!html.contains("<em>literal asterisks</em>"));
}

#[test]
fn representative_language_names_and_fallbacks_are_explicit() {
    let source = r#"@title Languages
@s Rust
@code_type Rust .rs
Rust @code{let value = 1;} and `let plain = 2;`.
--- rust.rs
fn rust_code() {}
---
@s C
@code_type C .c
--- c.c
int c_code(void) { return 0; }
---
@s Go
@code_type Go .go
--- go.go
func goCode() {}
---
@s D
@code_type D .d
--- d.d
void dCode() {}
---
@s Python
@code_type Python .py
--- python.py
def python_code(): pass
---
@s Alias
@code_type C++ .cc
--- alias.cc
int alias_code() {}
---
@s Extension fallback
@code_type ??? .rs
--- fallback.rs
fn fallback() {}
---
@s Unsupported
@code_type invented .invented
--- unsupported.invented
invented code
---
@s Cleared
@code_type none
--- plain.txt
plain
---
"#;
    let html = String::from_utf8(weave("languages.lit", source)).unwrap();

    for metadata in [
        "data-litweb-language=\"rust rs\"",
        "data-litweb-language=\"c\"",
        "data-litweb-language=\"go\"",
        "data-litweb-language=\"d\"",
        "data-litweb-language=\"python py\"",
        "data-litweb-language=\"cpp cc\"",
        "data-litweb-language=\"rs\"",
        "data-litweb-language=\"invented\"",
    ] {
        assert!(html.contains(metadata), "{metadata}");
    }
    assert!(html.contains("<code class=\"source-code\">let value = 1;</code>"));
    assert!(html.contains("<code>let plain = 2;</code>"));
    assert!(!html.contains("<code class=\"language-none\""));
}

#[test]
fn highlighting_is_conditional_and_the_default_bundle_is_complete() {
    let plain = parse_str(
        "plain.lit",
        "@title Plain\n@s One\n--- plain.txt\nplain\n---\n",
    )
    .unwrap();
    let plain_plan = plan_weave(&plain).unwrap();
    assert_eq!(plain_plan.outputs.len(), 1);
    let plain_html = String::from_utf8(plain_plan.outputs[0].bytes.clone()).unwrap();
    assert!(!plain_html.contains("data-litweb-ready"));
    assert!(!plain_html.contains("data-litweb-highlight-ready"));
    assert!(!plain_html.contains("prism-1.30.0"));

    let inline_only = parse_str(
        "inline.lit",
        "@code_type rust .rs\n@title Inline\n@s One\nOnly @code{let value = 1;}.\n",
    )
    .unwrap();
    let inline_plan = plan_weave(&inline_only).unwrap();
    assert_eq!(inline_plan.outputs.len(), 1);
    let inline_html = String::from_utf8(inline_plan.outputs[0].bytes.clone()).unwrap();
    assert!(inline_html.contains("<code class=\"source-code\">let value = 1;</code>"));
    assert!(!inline_html.contains("data-litweb-highlight"));
    assert!(!inline_html.contains("prism-1.30.0"));

    let rust = parse_str(
        "rust.lit",
        "@code_type rust .rs\n@s One\n--- rust.rs\nfn main() {}\n---\n",
    )
    .unwrap();
    let first = plan_weave(&rust).unwrap();
    let second = plan_weave(&rust).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.outputs.len(), 15);
    let html = String::from_utf8(first.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains(
        "<html lang=\"en\" data-litweb-ready=\"pending\" \
         data-litweb-highlight-ready=\"pending\">"
    ));
    assert!(html.contains(
        "<script defer src=\"litweb-assets/prism-1.30.0/prism-all.min.js\"></script>"
    ));
    assert!(html.contains(
        "<script defer src=\"litweb-assets/prism-1.30.0/litweb-highlight.js\"></script>"
    ));
    assert!(html.contains("Litweb's GitHub-inspired light theme for Prism"));
    assert!(html.contains(".token.namespace {\n    color: #4d5762;\n}"));
    assert!(!html.contains(".token.namespace {\n    opacity:"));
    assert_eq!(
        first
            .outputs
            .iter()
            .filter(|output| output
                .relative_path
                .starts_with("litweb-assets/prism-1.30.0"))
            .count(),
        14
    );
    for path in [
        "prism-all.min.js",
        "litweb-highlight.js",
        "LICENSE-Prism",
        "LICENSE-Primer",
        "README.md",
        "themes/litweb.css",
        "themes/default.css",
        "themes/dark.css",
        "themes/funky.css",
        "themes/okaidia.css",
        "themes/twilight.css",
        "themes/coy.css",
        "themes/solarized-light.css",
        "themes/tomorrow-night.css",
    ] {
        assert!(first.outputs.iter().any(|output| {
            output.relative_path == Path::new("litweb-assets/prism-1.30.0").join(path)
        }));
    }
}

#[test]
fn named_none_builtins_and_custom_css_choose_page_appearance() {
    let disabled = parse_str(
        "disabled.lit",
        "@colorscheme NoNe\n@code_type rust .rs\n@s One\n--- out.rs\nfn main() {}\n---\n",
    )
    .unwrap();
    let plan = plan_weave(&disabled).unwrap();
    assert_eq!(plan.outputs.len(), 1);
    let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains("<pre class=\"language-rust\"><code class=\"language-rust\">"));
    assert!(!html.contains("data-litweb-highlight=\"pending\""));
    assert!(!html.contains("data-litweb-highlight-ready"));
    assert!(!html.contains("prism-1.30.0"));

    let dark = parse_str(
        "dark.lit",
        "@colorscheme OKAIDIA\n@code_type rust .rs\n@s One\n--- out.rs\nfn main() {}\n---\n",
    )
    .unwrap();
    let plan = plan_weave(&dark).unwrap();
    let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains("background:#272822"));

    for name in [
        "litweb",
        "prism-default",
        "dark",
        "funky",
        "okaidia",
        "twilight",
        "coy",
        "solarized-light",
        "tomorrow-night",
    ] {
        let source = format!(
            "@colorscheme {name}\n@code_type rust .rs\n\
             @s One\n--- out.rs\nfn main() {{}}\n---\n"
        );
        let program = parse_str(format!("{name}.lit"), &source).unwrap();
        let plan = plan_weave(&program).unwrap();
        assert_eq!(plan.outputs.len(), 15, "{name}");
        let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
        assert!(
            html.contains("data-litweb-highlight-ready=\"pending\""),
            "{name}"
        );
        assert!(
            html.contains("Litweb owns code typography and panel geometry."),
            "{name}"
        );
        assert!(html.contains("font-family: monospace;"), "{name}");
        assert!(html.contains("font-size: inherit;"), "{name}");
        assert!(html.contains("line-height: 1.35;"), "{name}");
        if name == "prism-default" {
            let prism_typography = html.find("font-family:Consolas").unwrap();
            let litweb_typography = html
                .find("Litweb owns code typography and panel geometry.")
                .unwrap();
            assert!(prism_typography < litweb_typography);
        }
    }

    let directory = TestDirectory::new("custom-theme");
    let source_path = directory.path().join("nested/input.lit");
    let theme_path = directory.path().join("nested/themes/my scheme.css");
    fs::create_dir_all(theme_path.parent().unwrap()).unwrap();
    fs::write(
        &theme_path,
        ".token.keyword { color: rgb(1, 2, 3); }\n\
         .sentinel::after { content: \"</StYlE>\"; }\n",
    )
    .unwrap();
    let source = "@colorscheme themes/my scheme.css\n@code_type rust .rs\n\
                  @s One\n--- out.rs\nfn main() {}\n---\n";
    let custom = parse_str(source_path.to_str().unwrap(), source).unwrap();
    let plan = plan_weave(&custom).unwrap();
    let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains(".token.keyword { color: rgb(1, 2, 3); }"));
    assert!(html.contains("content: \"<\\/style>\""));
    assert!(!html.contains("content: \"</StYlE>\""));
    assert!(!html.contains("Litweb owns code typography and panel geometry."));
}

#[test]
fn colorscheme_failures_are_structured_and_command_line_css_uses_the_working_directory() {
    let unknown = parse_str(
        "unknown.lit",
        "@colorscheme ultraviolet\n@code_type rust .rs\n@s One\n--- out.rs\nfn main() {}\n---\n",
    )
    .unwrap();
    let WeavePlanError::Weave(errors) = plan_weave(&unknown).unwrap_err() else {
        panic!("an unknown scheme should be a weave error");
    };
    assert_eq!(errors.as_slice()[0].origin.line, 1);
    assert!(matches!(
        &errors.as_slice()[0].kind,
        WeaveErrorKind::UnknownColorScheme { name } if name == "ultraviolet"
    ));
    for retired_name in ["github", "default"] {
        let source = format!(
            "@colorscheme {retired_name}\n@code_type rust .rs\n\
             @s One\n--- out.rs\nfn main() {{}}\n---\n"
        );
        let program = parse_str(format!("{retired_name}.lit"), &source).unwrap();
        let WeavePlanError::Weave(errors) = plan_weave(&program).unwrap_err() else {
            panic!("the retired scheme name should be a weave error");
        };
        assert!(matches!(
            &errors.as_slice()[0].kind,
            WeaveErrorKind::UnknownColorScheme { name } if name == retired_name
        ));
    }

    let directory = TestDirectory::new("theme-errors");
    let missing_source = directory.path().join("missing.lit");
    let missing = parse_str(
        missing_source.to_str().unwrap(),
        "@colorscheme absent.css\n@code_type rust .rs\n@s One\n--- out.rs\nfn main() {}\n---\n",
    )
    .unwrap();
    let WeavePlanError::Weave(errors) = plan_weave(&missing).unwrap_err() else {
        panic!("a missing custom stylesheet should be a weave error");
    };
    assert!(matches!(
        &errors.as_slice()[0].kind,
        WeaveErrorKind::ReadColorScheme { path, .. }
            if path == &directory.path().join("absent.css")
    ));

    let invalid_path = directory.path().join("invalid.css");
    fs::write(&invalid_path, [0xff, 0xfe]).unwrap();
    let invalid_source = directory.path().join("invalid.lit");
    let invalid = parse_str(
        invalid_source.to_str().unwrap(),
        "@colorscheme invalid.css\n@code_type rust .rs\n@s One\n--- out.rs\nfn main() {}\n---\n",
    )
    .unwrap();
    let WeavePlanError::Weave(errors) = plan_weave(&invalid).unwrap_err() else {
        panic!("non-UTF-8 CSS should be a weave error");
    };
    assert!(matches!(
        &errors.as_slice()[0].kind,
        WeaveErrorKind::ColorSchemeInvalidUtf8 {
            path,
            valid_up_to: 0
        } if path == &invalid_path
    ));

    let input = directory.path().join("command-line.lit");
    let output_directory = directory.path().join("output");
    fs::write(
        &input,
        "@code_type rust .rs\n@s One\n--- out.rs\nfn main() {}\n---\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("command-line.css"),
        ".token.keyword { color: rgb(4, 5, 6); }\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lw"))
        .current_dir(directory.path())
        .args([
            Path::new("--weave"),
            Path::new("--colorscheme"),
            Path::new("command-line.css"),
            Path::new("--out-dir"),
            &output_directory,
            &input,
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let html = fs::read_to_string(output_directory.join("command-line.html")).unwrap();
    assert!(html.contains(".token.keyword { color: rgb(4, 5, 6); }"));

    let disabled_directory = directory.path().join("disabled-output");
    let output = Command::new(env!("CARGO_BIN_EXE_lw"))
        .current_dir(directory.path())
        .args([
            Path::new("--weave"),
            Path::new("--no-highlight"),
            Path::new("--out-dir"),
            &disabled_directory,
            &input,
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let html = fs::read_to_string(disabled_directory.join("command-line.html")).unwrap();
    assert!(!html.contains("data-litweb-highlight"));
    assert!(
        !disabled_directory
            .join("litweb-assets/prism-1.30.0")
            .exists()
    );
}

#[test]
fn highlighting_preserves_static_fragment_markup_and_coordinates_with_math() {
    let source = r#"@code_type rust .rs
@title Mixed
@s Step
--- Run one step
println!("step");
---
@s Program
The target-language @code{run()} call differs from `run()`, and $x=1$.
--- program.rs
fn run() {
    @{Run one step}
    println!("done");
}
---
"#;
    let program = parse_str("mixed.lit", source).unwrap();
    let plan = plan_weave(&program).unwrap();
    let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains(
        "<span class=\"nocode\">    ⟨Run one step <a href=\"#1:1\" \
         aria-label=\"section 1\">1</a>⟩</span>\n    println!"
    ));
    assert!(html.contains("<code class=\"source-code\">run()</code>"));
    assert!(html.contains("<code>run()</code>"));
    assert!(html.contains("data-litweb-math-ready=\"pending\""));
    assert!(html.contains("data-litweb-highlight-ready=\"pending\""));
    assert!(html.contains("data-litweb-ready=\"pending\""));
}

#[test]
fn highlighting_adapter_leaves_fragment_elements_in_place() {
    let source = r#"@code_type rust .rs
@title Protected fragments
@s Part
--- Part
let part = 1;
---
@s Program
--- program.rs
fn program() {
    @{Part}
}
---
"#;
    let program = parse_str("protected.lit", source).unwrap();
    let plan = plan_weave(&program).unwrap();
    let adapter = plan
        .outputs
        .iter()
        .find(|output| output.relative_path.ends_with("litweb-highlight.js"))
        .expect("highlighted output includes the browser adapter");
    let adapter = std::str::from_utf8(&adapter.bytes).unwrap();
    let prism = plan
        .outputs
        .iter()
        .find(|output| output.relative_path.ends_with("prism-all.min.js"))
        .expect("highlighted output includes Prism");
    let prism = std::str::from_utf8(&prism.bytes).unwrap();

    assert!(adapter.contains(".call(element.childNodes)"));
    assert!(adapter.contains("node.nodeType === Node.TEXT_NODE"));
    assert!(adapter.contains("Prism.highlight(node.data, grammar, language)"));
    assert!(adapter.contains("parent.insertBefore(highlighted.firstChild, node)"));
    assert!(adapter.contains("parent.removeChild(node)"));
    assert!(!adapter.contains("Prism.highlightElement(element)"));
    assert!(!prism.contains("Prism.plugins.KeepMarkup"));
}

#[test]
fn inline_code_takes_precedence_over_code_reference_syntax() {
    let source = "\
@title Literal reference
@s One
`@{Missing}` describes the reference syntax without using the block.
--- output.txt
text
---
";
    let html = String::from_utf8(weave("literal-reference.lit", source)).unwrap();

    assert!(html.contains("<code>@{Missing}</code> describes the reference syntax"));
}

#[test]
fn inline_code_style_preserves_authored_whitespace_without_wrapping_code_blocks() {
    let source = r#"@title Whitespace
@s One
Both `@{Greeting line}` and `    @{Greeting line}` are shown exactly.

```text
    fixed width
```
"#;
    let html = String::from_utf8(weave("whitespace.lit", source)).unwrap();

    assert!(html.contains(
        "code {\n    white-space: pre-wrap;\n}\n\
         pre code {\n    white-space: inherit;\n}\n\
         pre {"
    ));
    assert!(html.contains(
        "Both <code>@{Greeting line}</code> and \
         <code>    @{Greeting line}</code> are shown exactly."
    ));
    assert!(html.contains("<pre><code>    fixed width\n</code></pre>"));
}

#[test]
fn emphasis_and_strong_text_may_cross_soft_line_breaks() {
    let source = r#"@code_type rust .rs
@title Multiline emphasis
@s Forms
Paragraph *italic
continues* and **strong
continues**.

Nested *starts
with @code{parse_value()} and @{Snippet}*.

Escaped \*not
emphasized*.

Unmatched **not
strong.

Separated *does not

cross*.

- List *italic
  continues* and **strong
  continues**.
- First *does not
- cross*.

--- Snippet
text
---
"#;
    let html = String::from_utf8(weave("multiline-emphasis.lit", source)).unwrap();

    assert!(html.contains(
        "<p>Paragraph <em>italic\ncontinues</em> and <strong>strong\ncontinues</strong>.</p>"
    ));
    assert!(html.contains(
        "<p>Nested <em>starts\nwith <code class=\"source-code\">parse_value()</code> and \
         <code class=\"section-reference\">⟨Snippet <a href=\"#1:1\" aria-label=\"section 1\">1</a>⟩</code></em>.</p>"
    ));
    assert!(html.contains("<p>Escaped *not\nemphasized*.</p>"));
    assert!(html.contains("<p>Unmatched **not\nstrong.</p>"));
    assert!(html.contains("<p>Separated *does not</p>\n<p>cross*.</p>"));
    assert!(html.contains(
        "<li>List <em>italic\ncontinues</em> and <strong>strong\ncontinues</strong>.</li>"
    ));
    assert!(html.contains("<li>First *does not</li>\n<li>cross*.</li>"));
    assert!(!html.contains("<em></em>"));
    assert!(!html.contains("<strong></strong>"));
}

#[test]
fn multiline_emphasis_references_keep_their_physical_source_origin() {
    let source = "\
@title Located emphasis
@s Example
*the emphasis begins
on this @{Missing emphasized reference} line*.
";
    let program = parse_str("located-emphasis.lit", source).unwrap();
    let error = plan_weave(&program).expect_err("weaving should fail");
    let WeavePlanError::Weave(errors) = error else {
        panic!("expected weave errors");
    };

    assert_eq!(errors.as_slice().len(), 1);
    assert_eq!(errors.as_slice()[0].origin.line, 4);
    assert_eq!(
        errors.as_slice()[0].kind,
        WeaveErrorKind::UndefinedBlock {
            name: "Missing emphasized reference".to_owned(),
        }
    );
}

#[test]
fn other_inline_forms_remain_bounded_by_their_source_line() {
    let source = r#"@title Line-bounded inline forms
@s Forms
Backtick `first
second`.

Target @code{first
second}.

Math $first
second$.

Link [first
second](guide.html).

Reference @{first
second}.
"#;
    let html = String::from_utf8(weave("line-bounded-inline.lit", source)).unwrap();

    for literal in [
        "<p>Backtick `first\nsecond`.</p>",
        "<p>Target @code{first\nsecond}.</p>",
        "<p>Math $first\nsecond$.</p>",
        "<p>Link [first\nsecond](guide.html).</p>",
        "<p>Reference @{first\nsecond}.</p>",
    ] {
        assert!(html.contains(literal), "missing literal form: {literal}");
    }
}

#[test]
fn target_language_code_in_prose_is_distinct_balanced_and_opaque() {
    let source = r#"@code_type rust .rs
@title Code in prose
@s Forms
@code{value < limit && map["x"] > 0} begins this prose line.
Nested @code{Point { x: 1 }} stays in one span.
Escaped @code{\{ \} \\ \n} delimiters are decoded.
Opaque @code{**strong** `tick` @{Missing}} text is not recursively rendered.
`@code{literal}` displays the notation.
Unclosed @code{still literal
--- output.rs
fn main() {}
---
"#;
    let html = String::from_utf8(weave("code-in-prose.lit", source)).unwrap();

    assert!(html.contains(
        r#"<code class="source-code">value &lt; limit &amp;&amp; map["x"] &gt; 0</code> begins this prose line."#
    ));
    assert!(html.contains(
        r#"Nested <code class="source-code">Point { x: 1 }</code> stays in one span."#
    ));
    assert!(html.contains(
        r#"Escaped <code class="source-code">{ } \ \n</code> delimiters are decoded."#
    ));
    assert!(html.contains(
        r#"Opaque <code class="source-code">**strong** `tick` @{Missing}</code> text is not recursively rendered."#
    ));
    assert!(html.contains("<code>@code{literal}</code> displays the notation."));
    assert!(html.contains("Unclosed @code{still literal"));
    assert!(!html.contains("<strong>strong</strong>"));
}

#[test]
fn equations_preserve_source_respect_inline_shielding_and_plan_local_assets() {
    let source = r#"@code_type rust .rs
@title Equations

@s Forms
Inline $x < y & z$ and escaped TeX $x + \$5$ stay in the sentence.
Even \\$x$; odd \\\$x$.
*Emphasized $e$* and [linked $l$](guide.html).
Opaque `$backtick$`, @code{price = "$5"}, and @{Dollars} stay opaque.
- A list has inline $q$ but keeps $$display$$ literal.
Before the display.
  $$
\begin{aligned}
x &= 1 \\
y &= 2
\end{aligned}
  $$
After the display.
An unmatched $inline opener stays literal.
$$an unmatched display opener stays literal
--- Dollars
let price = "$5";
---
"#;
    let program = parse_str("equations.lit", source).unwrap();
    let plan = plan_weave(&program).unwrap();
    assert_eq!(plan.outputs.len(), 40);
    assert_eq!(plan.outputs[0].relative_path, Path::new("equations.html"));

    let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains(
        r#"Inline <span class="litweb-math litweb-math-inline">x &lt; y &amp; z</span> and escaped TeX <span class="litweb-math litweb-math-inline">x + \$5</span>"#
    ));
    assert!(html.contains(
        r#"Even \<span class="litweb-math litweb-math-inline">x</span>; odd \$x$."#
    ));
    assert!(html.contains(
        r#"<em>Emphasized <span class="litweb-math litweb-math-inline">e</span></em> and <a href="guide.html">linked <span class="litweb-math litweb-math-inline">l</span></a>."#
    ));
    assert!(html.contains(
        r#"Opaque <code>$backtick$</code>, <code class="source-code">price = "$5"</code>, and <code class="section-reference">⟨Dollars <a"#
    ));
    assert!(html.contains(
        r#"<li>A list has inline <span class="litweb-math litweb-math-inline">q</span> but keeps $$display$$ literal.</li>"#
    ));
    assert!(html.contains(
        "<p>Before the display.</p>\n<div class=\"litweb-math litweb-math-display\">\\begin{aligned}\nx &amp;= 1 \\\\\ny &amp;= 2\n\\end{aligned}</div>\n<p>After the display."
    ));
    assert!(html.contains("An unmatched $inline opener stays literal."));
    assert!(html.contains("$$an unmatched display opener stays literal"));
    assert!(html.contains("let price = \"$5\";"));

    assert!(html.contains(
        "<html lang=\"en\" data-litweb-ready=\"pending\" \
         data-litweb-math-ready=\"pending\" data-litweb-highlight-ready=\"pending\">"
    ));
    assert!(html.contains(
        "<link rel=\"stylesheet\" href=\"litweb-assets/katex-0.18.1/katex.min.css\">"
    ));
    assert!(html.contains(
        "<script defer src=\"litweb-assets/katex-0.18.1/katex.min.js\"></script>"
    ));
    assert!(html.contains(
        "<script defer src=\"litweb-assets/katex-0.18.1/litweb-math.js\"></script>"
    ));
    assert!(html.contains(".litweb-math .katex {\n    font-size: 1.1em;"));
    assert!(html.contains("overflow-x: visible;\n        overflow-y: visible;"));
    assert!(!html.contains("cdnjs"));

    let paths = plan
        .outputs
        .iter()
        .skip(1)
        .map(|output| output.relative_path.as_path())
        .collect::<Vec<_>>();
    assert!(paths.contains(&Path::new("litweb-assets/katex-0.18.1/katex.min.js")));
    assert!(paths.contains(&Path::new("litweb-assets/katex-0.18.1/katex.min.css")));
    assert!(paths.contains(&Path::new("litweb-assets/katex-0.18.1/litweb-math.js")));
    assert!(paths.contains(&Path::new("litweb-assets/katex-0.18.1/LICENSE")));
    assert!(paths.contains(&Path::new("litweb-assets/katex-0.18.1/README.md")));
    assert_eq!(
        paths
            .iter()
            .filter(|path| path
                .extension()
                .is_some_and(|extension| extension == "woff2"))
            .count(),
        20
    );

    let css = plan
        .outputs
        .iter()
        .find(|output| {
            output.relative_path == Path::new("litweb-assets/katex-0.18.1/katex.min.css")
        })
        .unwrap();
    assert_eq!(
        css.bytes,
        include_bytes!("../assets/katex/0.18.1/katex.min.css")
    );
    assert!(
        !css.bytes
            .windows(b"format(\"woff\")".len())
            .any(|window| window == b"format(\"woff\")")
    );
    assert!(
        !css.bytes
            .windows(b".ttf".len())
            .any(|window| window == b".ttf")
    );
}

#[test]
fn no_weave_hides_code_and_suppresses_its_definition_link() {
    let source = "\
@title Hidden
@s One
The @{Hidden} block is intentionally omitted.
--- Hidden --- noWeave
secret text
---
";
    let html = String::from_utf8(weave("hidden.lit", source)).unwrap();

    assert!(html.contains("The <code class=\"section-reference\">⟨Hidden⟩</code> block"));
    assert!(!html.contains("secret text"));
    assert!(!html.contains("class=\"section-name\">⟨Hidden"));
}

#[test]
fn undefined_references_return_ordered_errors_and_no_plan() {
    let source = "\
@title Missing
@s One
The @{Missing prose} reference is missing.
--- output.txt
@{Missing code}
---
";
    let program = parse_str("missing.lit", source).unwrap();
    let error = plan_weave(&program).expect_err("weaving should fail");
    let WeavePlanError::Weave(errors) = error else {
        panic!("expected weave errors");
    };
    assert_eq!(errors.as_slice().len(), 2);
    assert_eq!(
        errors.as_slice()[0].kind,
        WeaveErrorKind::UndefinedBlock {
            name: "Missing prose".to_owned(),
        }
    );
    assert_eq!(errors.as_slice()[0].origin.line, 3);
    assert_eq!(
        errors.as_slice()[1].kind,
        WeaveErrorKind::UndefinedBlock {
            name: "Missing code".to_owned(),
        }
    );
    assert_eq!(errors.as_slice()[1].origin.line, 5);
}

#[test]
fn an_input_without_a_usable_stem_has_a_structured_error() {
    let program = parse_str("", "@title Empty path\n@s One\ntext\n").unwrap();
    let error = plan_weave(&program).expect_err("weaving should fail");
    let WeavePlanError::Weave(errors) = error else {
        panic!("expected weave errors");
    };
    assert_eq!(errors.as_slice().len(), 1);
    assert_eq!(errors.as_slice()[0].kind, WeaveErrorKind::InvalidOutputName);
    assert_eq!(errors.as_slice()[0].origin.line, 1);
}

#[test]
fn weave_only_and_default_cli_modes_write_the_expected_files() {
    let directory = TestDirectory::new("cli-modes");
    let input = directory.path().join("program.lit");
    let weave_only = directory.path().join("weave-only");
    let default = directory.path().join("default");
    fs::write(&input, MINIMAL).unwrap();

    let weave_result = lw(&[Path::new("-w"), Path::new("-odir"), &weave_only, &input]);
    let default_result = lw(&[Path::new("-odir"), &default, &input]);
    assert!(weave_result.status.success(), "{:?}", weave_result.stderr);
    assert!(
        default_result.status.success(),
        "{:?}",
        default_result.stderr
    );
    assert!(weave_result.stdout.is_empty() && weave_result.stderr.is_empty());
    assert!(default_result.stdout.is_empty() && default_result.stderr.is_empty());

    assert!(weave_only.join("program.html").is_file());
    assert!(!weave_only.join("minimal.txt").exists());
    assert!(default.join("program.html").is_file());
    assert!(default.join("minimal.txt").is_file());
    assert_eq!(
        fs::read(weave_only.join("program.html")).unwrap(),
        fs::read(default.join("program.html")).unwrap()
    );
}

#[test]
fn default_mode_weaves_a_document_without_roots_and_reports_the_tangle_warning() {
    let directory = TestDirectory::new("default-no-roots");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(
        &input,
        "@title No roots\n@s One\n--- Named block\ntext\n---\n",
    )
    .unwrap();

    let result = lw(&[Path::new("-odir"), &output_directory, &input]);
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    assert_eq!(
        String::from_utf8(result.stderr).unwrap(),
        format!(
            "{}:1: warning: no file code blocks; no code written\n",
            input.display()
        )
    );
    assert!(output_directory.join("input.html").is_file());
    assert_eq!(fs::read_dir(&output_directory).unwrap().count(), 1);
}

#[test]
fn a_default_output_collision_is_rejected_before_writing() {
    let directory = TestDirectory::new("output-collision");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(
        &input,
        "@title Collision\n@s One\n--- input.html\ntext\n---\n",
    )
    .unwrap();

    let result = lw(&[Path::new("-odir"), &output_directory, &input]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("more than one generated file has this destination")
    );
    assert!(!output_directory.exists());
}

#[test]
fn a_tangled_file_cannot_be_the_parent_of_the_math_asset_bundle() {
    let directory = TestDirectory::new("math-asset-parent-collision");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(
        &input,
        "@title Collision\n@s Math\nAn equation $x^2$.\n--- litweb-assets/katex-0.18.1\ntext\n---\n",
    )
    .unwrap();

    let result = lw(&[Path::new("-odir"), &output_directory, &input]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8(result.stderr).unwrap().contains(
            "another generated file occupies a parent directory of this output"
        )
    );
    assert!(!output_directory.exists());
}

#[test]
fn a_tangled_file_cannot_be_the_parent_of_the_highlighting_asset_bundle() {
    let directory = TestDirectory::new("highlight-asset-parent-collision");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(
        &input,
        "@code_type rust .rs\n@title Collision\n@s Code\n\
         --- litweb-assets/prism-1.30.0\nfn main() {}\n---\n",
    )
    .unwrap();

    let result = lw(&[Path::new("-odir"), &output_directory, &input]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8(result.stderr).unwrap().contains(
            "another generated file occupies a parent directory of this output"
        )
    );
    assert!(!output_directory.exists());
}

#[test]
fn a_weave_failure_discards_already_planned_tangled_output() {
    let directory = TestDirectory::new("no-partial-default");
    let input = directory.path().join("input.lit");
    let output_directory = directory.path().join("output");
    fs::write(
        &input,
        "@title Missing\n@s One\nThe @{Missing} block is absent.\n--- output.txt\ntext\n---\n",
    )
    .unwrap();

    let result = lw(&[Path::new("-odir"), &output_directory, &input]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains(":3: error: code block {Missing} is not defined")
    );
    assert!(!output_directory.exists());
}
