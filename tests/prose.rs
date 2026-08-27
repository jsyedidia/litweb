use std::path::PathBuf;

use litweb::identifier::analyze;
use litweb::parser::{BlockKind, parse_str};
use litweb::weaver::{WeaveOptions, plan_weave_with_options};

const NOTES: &str = r#"@code_type rust .rs
@s Notes
Call @code{parse_str()} before continuing.

```text
@s Not a section
--- Not a block
@code{not_rust()}
```

Name | Example
--- | ---
type | @code{Graph}
"#;

#[test]
fn documented_prose_boundaries_agree_across_consumers() {
    let program = parse_str("notes.lit", NOTES).expect("documented source should parse");
    let chapter = &program.chapters[0];
    assert_eq!(chapter.sections.len(), 1);
    assert_eq!(chapter.sections[0].title, "Notes");
    assert_eq!(chapter.sections[0].blocks.len(), 1);

    let prose = &chapter.sections[0].blocks[0];
    assert!(matches!(prose.kind, BlockKind::Prose));
    assert_eq!(
        prose.text(),
        "Call @code{parse_str()} before continuing.\n\n\
         ```text\n@s Not a section\n--- Not a block\n@code{not_rust()}\n```\n\n\
         Name | Example\n--- | ---\ntype | @code{Graph}\n\n"
    );

    let identifiers = analyze(&program);
    let names = identifiers
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"parse_str"));
    assert!(!names.contains(&"not_rust"));

    let plan = plan_weave_with_options(
        &program,
        WeaveOptions {
            identifier_index: false,
            color_scheme: Some(PathBuf::from("none")),
        },
    )
    .expect("documented source should weave");
    assert_eq!(plan.outputs.len(), 1);
    let html = String::from_utf8(plan.outputs[0].bytes.clone()).unwrap();

    assert!(html.contains(
        "<p>Call <code class=\"source-code\">parse_str()</code> before continuing.</p>"
    ));
    assert!(html.contains(
        "<pre><code>@s Not a section\n--- Not a block\n@code{not_rust()}\n</code></pre>"
    ));
    assert!(html.contains("<th scope=\"col\" class=\"table-align-left\">Name</th>"));
    assert!(html.contains(
        "<td class=\"table-align-left\"><code class=\"source-code\">Graph</code></td>"
    ));
    assert!(!html.contains("section-title\">Not a section"));
    assert!(!html.contains("<code class=\"source-code\">not_rust()</code>"));
}
