use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use litweb::parser::{
    Block, BlockKind, CommandKind, Modifier, ParseError, ParseErrorKind, Program,
    parse_bytes, parse_str,
};

const SUPPORTED: &str = include_str!("fixtures/parser/supported.lit");

fn parse_supported() -> Program {
    parse_str("supported.lit", SUPPORTED).expect("supported fixture should parse")
}

fn code(block: &Block) -> &litweb::parser::CodeBlock {
    block.code().expect("expected a code block")
}

fn one_error(file: &str, source: &str) -> ParseError {
    let errors = parse_str(file, source).expect_err("fixture should fail to parse");
    assert_eq!(errors.as_slice().len(), 1, "{errors:?}");
    errors.into_vec().pop().unwrap()
}

#[test]
fn supported_source_produces_the_ordered_literate_model() {
    let program = parse_supported();

    assert_eq!(program.file, "supported.lit");
    assert_eq!(program.origin.line, 1);
    assert_eq!(program.title, "Parser Fixture");
    assert_eq!(program.title_origin.as_ref().unwrap().line, 1);
    assert_eq!(program.text, SUPPORTED);
    assert_eq!(program.commands.len(), 2);
    assert_eq!(program.commands[0].kind, CommandKind::CodeType);
    assert_eq!(program.commands[0].name, "@code_type");
    assert_eq!(program.commands[0].arguments, "rust .rs");
    assert_eq!(program.commands[0].origin.line, 2);
    assert_eq!(program.commands[1].kind, CommandKind::CommentType);
    assert_eq!(program.commands[1].arguments, "// %s");
    assert_eq!(program.commands[1].origin.line, 3);

    assert_eq!(program.chapters.len(), 1);
    let chapter = &program.chapters[0];
    assert_eq!(chapter.title, "Parser Fixture");
    assert_eq!(chapter.title_origin.as_ref().unwrap().line, 1);
    assert_eq!(chapter.file, "supported.lit");
    assert_eq!(chapter.number(), "1");
    assert_eq!(chapter.sections.len(), 2);

    let first = &chapter.sections[0];
    assert_eq!(first.origin.line, 5);
    assert_eq!(first.title, "First section");
    assert_eq!(first.number, 1);
    assert_eq!(first.commands, chapter.commands);
    assert_eq!(first.blocks.len(), 10);

    assert!(matches!(first.blocks[0].kind, BlockKind::Prose));
    assert_eq!(first.blocks[0].origin.line, 5);
    assert_eq!(
        first.blocks[0]
            .lines
            .iter()
            .map(|line| (line.origin.line, line.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (6, ""),
            (7, "This prose keeps @{Piece} and an @unknown directive."),
            (9, ""),
        ]
    );

    let output = code(&first.blocks[1]);
    assert_eq!(first.blocks[1].origin.line, 10);
    assert_eq!(output.name, "output.rs");
    assert!(!output.quoted_name);
    assert!(output.modifiers.is_empty());
    assert_eq!(output.code_type, "rust .rs");
    assert_eq!(output.comment_string, "// %s");
    assert_eq!(
        first.blocks[1].text(),
        "fn main() {\n    // This comment is code and must be preserved.\n}\n"
    );
    assert_eq!(first.blocks[1].lines[0].origin.line, 11);

    assert!(matches!(first.blocks[2].kind, BlockKind::Prose));
    assert_eq!(first.blocks[2].origin.line, 14);
    assert_eq!(first.blocks[2].text(), "\nProse after the first block.\n\n");

    let makefile = code(&first.blocks[3]);
    assert_eq!(makefile.name, "Makefile");
    assert!(makefile.quoted_name);
    assert_eq!(
        makefile.modifiers,
        vec![Modifier::NoWeave, Modifier::NoHeader]
    );

    let original = code(&first.blocks[5]);
    let addition = code(&first.blocks[7]);
    let redefinition = code(&first.blocks[9]);
    assert_eq!(original.name, "Piece");
    assert!(original.modifiers.is_empty());
    assert_eq!(addition.name, "Piece");
    assert_eq!(addition.modifiers, vec![Modifier::Additive]);
    assert_eq!(redefinition.name, "Piece");
    assert_eq!(redefinition.modifiers, vec![Modifier::Redefinition]);

    let second = &chapter.sections[1];
    assert_eq!(second.origin.line, 35);
    assert_eq!(second.title, "Second section");
    assert_eq!(second.number, 2);
    assert_eq!(second.commands.len(), 3);
    assert_eq!(second.commands[2].kind, CommandKind::CommentType);
    assert_eq!(second.commands[2].arguments, "# %s");
    assert_eq!(second.commands[2].origin.line, 36);
    assert_eq!(second.blocks.len(), 2);
    assert_eq!(second.blocks[0].text(), "\n");
    let another = code(&second.blocks[1]);
    assert_eq!(another.name, "Another");
    assert_eq!(another.code_type, "rust .rs");
    assert_eq!(another.comment_string, "# %s");
}

#[test]
fn greeting_example_produces_the_documented_occurrences() {
    let source = "\
@title A Greeting
@s Greeting
This small program writes one line.

--- hello.txt
@{Greeting line}
---

The text comes from a separate named piece.

--- Greeting line
Hello
---
";
    let program = parse_str("greeting.lit", source).unwrap();
    let chapter = &program.chapters[0];
    let section = &chapter.sections[0];

    assert_eq!(program.title, "A Greeting");
    assert_eq!(chapter.title, "A Greeting");
    assert_eq!(section.title, "Greeting");
    assert_eq!(section.origin.line, 2);
    assert_eq!(section.blocks.len(), 4);
    assert_eq!(
        section.blocks[0].text(),
        "This small program writes one line.\n\n"
    );
    assert_eq!(code(&section.blocks[1]).name, "hello.txt");
    assert_eq!(section.blocks[1].origin.line, 5);
    assert_eq!(section.blocks[1].text(), "@{Greeting line}\n");
    assert_eq!(
        section.blocks[2].text(),
        "\nThe text comes from a separate named piece.\n\n"
    );
    assert_eq!(code(&section.blocks[3]).name, "Greeting line");
    assert_eq!(section.blocks[3].origin.line, 11);
    assert_eq!(section.blocks[3].text(), "Hello\n");
}

#[test]
fn every_section_boundary_discards_only_blank_trailing_prose() {
    let at_eof =
        parse_str("eof.lit", "@s First\nintro\n--- Piece\nvalue\n---\n\n").unwrap();
    let before_next_section = parse_str(
        "next.lit",
        "@s First\nintro\n--- Piece\nvalue\n---\n@s Second\nmore\n",
    )
    .unwrap();

    let eof_blocks = &at_eof.chapters[0].sections[0].blocks;
    let next_blocks = &before_next_section.chapters[0].sections[0].blocks;
    assert_eq!(eof_blocks.len(), 2);
    assert_eq!(next_blocks.len(), 2);
    assert_eq!(
        eof_blocks.iter().map(Block::text).collect::<Vec<_>>(),
        next_blocks.iter().map(Block::text).collect::<Vec<_>>()
    );

    let with_prose = parse_str(
        "prose.lit",
        "@s First\nintro\n--- Piece\nvalue\n---\n\nafter\n",
    )
    .unwrap();
    let blocks = &with_prose.chapters[0].sections[0].blocks;
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[2].text(), "\nafter\n\n");
}

#[test]
fn colorscheme_is_page_level_and_preserves_explicit_none() {
    let program = parse_str(
        "theme.lit",
        "@colorscheme litweb\n@colorscheme none\n@s One\ntext\n",
    )
    .unwrap();
    assert_eq!(program.commands.len(), 2);
    assert_eq!(program.commands[0].kind, CommandKind::ColorScheme);
    assert_eq!(program.commands[0].arguments, "litweb");
    assert_eq!(program.commands[1].kind, CommandKind::ColorScheme);
    assert_eq!(program.commands[1].arguments, "");

    let error = one_error("late-theme.lit", "@s One\n@colorscheme okaidia\ntext\n");
    assert_eq!(error.origin.line, 2);
    assert_eq!(
        error.kind,
        ParseErrorKind::PageCommandAfterSection {
            name: "@colorscheme".to_owned(),
        }
    );

    let error = one_error("missing-theme.lit", "@colorscheme\n@s One\ntext\n");
    assert_eq!(error.origin.line, 1);
    assert_eq!(
        error.kind,
        ParseErrorKind::MissingCommandArguments {
            name: "@colorscheme".to_owned(),
        }
    );
}

#[test]
fn empty_lines_and_crlf_endings_keep_one_based_origins() {
    let source = "@s Lines\r\n\r\nalpha\r\n\r\n";
    let program = parse_str("lines.lit", source).unwrap();
    let block = &program.chapters[0].sections[0].blocks[0];

    assert_eq!(program.text, source);
    assert_eq!(block.text(), "\nalpha\n\n\n");
    assert_eq!(
        block
            .lines
            .iter()
            .map(|line| line.origin.line)
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
}

#[test]
fn exact_command_tokens_leave_unknown_at_lines_as_prose() {
    let source =
        "@s Commands\n@section is prose\n@titleish is prose\n@{Code reference}\n";
    let program = parse_str("commands.lit", source).unwrap();
    let prose = &program.chapters[0].sections[0].blocks[0];

    assert_eq!(
        prose.text(),
        "@section is prose\n@titleish is prose\n@{Code reference}\n\n"
    );
}

#[test]
fn markdown_rules_are_not_mistaken_for_code_openings() {
    let source = "@s Delimiters\n---\n----\n--- Named\ntext\n---\n";
    let program = parse_str("delimiters.lit", source).unwrap();
    let blocks = &program.chapters[0].sections[0].blocks;

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].text(), "---\n----\n");
    assert_eq!(code(&blocks[1]).name, "Named");
    assert_eq!(blocks[1].text(), "text\n");
}

#[test]
fn valid_prose_fences_shield_litweb_commands_and_code_delimiters() {
    let source = r#"@code_type rust .rs
@s Visible section
```text
@s Example section
@code_type none
--- example.rs
// retained prose
---
```
--- real.rs
fn real() {}
---
"#;
    let program = parse_str("fenced-parser.lit", source).unwrap();
    let chapter = &program.chapters[0];

    assert_eq!(chapter.sections.len(), 1);
    let section = &chapter.sections[0];
    assert_eq!(section.blocks.len(), 2);
    assert_eq!(
        section.blocks[0].text(),
        "```text\n@s Example section\n@code_type none\n--- example.rs\n\
         // retained prose\n---\n```\n"
    );
    assert_eq!(code(&section.blocks[1]).name, "real.rs");
    assert_eq!(code(&section.blocks[1]).code_type, "rust .rs");
}

#[test]
fn prose_tables_shield_commands_and_code_like_delimiter_rows() {
    let source = r#"@code_type rust .rs
@s Visible section
Command | Meaning
--- | ---
@s Example section | literal text
@code_type none | literal text
--- example.rs | literal text
// retained prose | literal text

--- real.rs
fn real() {}
---
"#;
    let program = parse_str("table-parser.lit", source).unwrap();
    let chapter = &program.chapters[0];

    assert_eq!(chapter.sections.len(), 1);
    let section = &chapter.sections[0];
    assert_eq!(section.blocks.len(), 2);
    assert_eq!(
        section.blocks[0].text(),
        "Command | Meaning\n--- | ---\n@s Example section | literal text\n\
         @code_type none | literal text\n--- example.rs | literal text\n\
         // retained prose | literal text\n\n"
    );
    assert_eq!(code(&section.blocks[1]).name, "real.rs");
    assert_eq!(code(&section.blocks[1]).code_type, "rust .rs");
}

#[test]
fn an_unclosed_prose_fence_does_not_shield_later_commands() {
    let source = "@s First\n```text\n@s Second\nordinary prose\n";
    let program = parse_str("unclosed-fence.lit", source).unwrap();
    let chapter = &program.chapters[0];

    assert_eq!(chapter.sections.len(), 2);
    assert!(chapter.sections[0].blocks[0].text().contains("```text"));
    assert_eq!(chapter.sections[1].title, "Second");
}

#[test]
fn none_clears_a_supported_configuration_value() {
    let source = "@comment_type none\n@s Config\n--- file.rs\ntext\n---\n";
    let program = parse_str("none.lit", source).unwrap();
    let block = &program.chapters[0].sections[0].blocks[1];

    assert_eq!(program.commands[0].arguments, "");
    assert_eq!(code(block).comment_string, "");
}

#[test]
fn additions_and_redefinitions_do_not_require_targets_during_parsing() {
    let source = "@s Deferred semantics\n--- Later +=\naddition\n---\n--- Other --- :=\nreplacement\n---\n";
    let program = parse_str("deferred-semantics.lit", source).unwrap();
    let blocks = &program.chapters[0].sections[0].blocks;

    assert_eq!(code(&blocks[1]).modifiers, vec![Modifier::Additive]);
    assert_eq!(code(&blocks[3]).modifiers, vec![Modifier::Redefinition]);
}

#[test]
fn malformed_and_duplicate_constructs_have_typed_origins() {
    let cases = [
        (
            "unclosed.lit",
            include_str!("fixtures/parser/unclosed.lit"),
            2,
            ParseErrorKind::UnclosedBlock {
                name: "broken.rs".to_owned(),
            },
        ),
        (
            "early.lit",
            include_str!("fixtures/parser/code_before_section.lit"),
            1,
            ParseErrorKind::CodeBeforeSection,
        ),
        (
            "duplicate.lit",
            include_str!("fixtures/parser/duplicate.lit"),
            5,
            ParseErrorKind::DuplicateDefinition {
                name: "Same name".to_owned(),
            },
        ),
        (
            "invalid-modifier.lit",
            include_str!("fixtures/parser/invalid_modifier.lit"),
            2,
            ParseErrorKind::InvalidModifier {
                name: "glitter".to_owned(),
            },
        ),
        (
            "conflicting.lit",
            include_str!("fixtures/parser/conflicting_modifiers.lit"),
            2,
            ParseErrorKind::ConflictingModifiers,
        ),
        (
            "duplicate-modifier.lit",
            include_str!("fixtures/parser/duplicate_modifier.lit"),
            2,
            ParseErrorKind::DuplicateModifier {
                name: "noWeave".to_owned(),
            },
        ),
        (
            "unsupported-command.lit",
            include_str!("fixtures/parser/unsupported_command.lit"),
            2,
            ParseErrorKind::UnsupportedCommand {
                name: "@include".to_owned(),
            },
        ),
        (
            "unsupported-modifier.lit",
            include_str!("fixtures/parser/unsupported_modifier.lit"),
            2,
            ParseErrorKind::UnsupportedModifier {
                name: "noTangle".to_owned(),
            },
        ),
        (
            "malformed-quote.lit",
            include_str!("fixtures/parser/malformed_quote.lit"),
            2,
            ParseErrorKind::MalformedQuotedName {
                name: "\"broken".to_owned(),
            },
        ),
        (
            "malformed-delimiter.lit",
            include_str!("fixtures/parser/malformed_delimiter.lit"),
            4,
            ParseErrorKind::MalformedClosingDelimiter {
                text: "--- trailing text".to_owned(),
            },
        ),
        (
            "missing-arguments.lit",
            include_str!("fixtures/parser/missing_arguments.lit"),
            1,
            ParseErrorKind::MissingCommandArguments {
                name: "@code_type".to_owned(),
            },
        ),
        (
            "empty-name.lit",
            "@s Empty name\n--- \ntext\n---\n",
            2,
            ParseErrorKind::EmptyBlockName,
        ),
    ];

    for (file, source, line, kind) in cases {
        let error = one_error(file, source);
        assert_eq!(error.origin.file, file);
        assert_eq!(error.origin.line, line);
        assert_eq!(error.kind, kind);
    }
}

#[test]
fn every_known_deferred_command_is_rejected_explicitly() {
    for command in [
        "@book",
        "@include file.lit",
        "@change file.lit",
        "@change_end",
        "@replace",
        "@with",
        "@end",
        "@compiler rustc",
        "@error_format format",
        "@add_css style.css",
        "@overwrite_css style.css",
    ] {
        let source = format!("@s Deferred\n{command}\n");
        let error = one_error("deferred.lit", &source);
        let name = command.split_whitespace().next().unwrap();
        assert_eq!(error.origin.line, 2);
        assert_eq!(
            error.kind,
            ParseErrorKind::UnsupportedCommand {
                name: name.to_owned()
            }
        );
    }
}

#[test]
fn invalid_utf8_reports_the_line_and_byte_boundary() {
    let source = b"@s Bytes\nprose\n\xff";
    let errors = parse_bytes("bytes.lit", source).unwrap_err();

    assert_eq!(errors.as_slice().len(), 1);
    assert_eq!(errors.as_slice()[0].origin.line, 3);
    assert_eq!(
        errors.as_slice()[0].kind,
        ParseErrorKind::InvalidUtf8 { valid_up_to: 15 }
    );
}

#[test]
fn repeated_parsing_has_identical_models_and_diagnostics() {
    assert_eq!(
        parse_str("supported.lit", SUPPORTED),
        parse_str("supported.lit", SUPPORTED)
    );
    let malformed = include_str!("fixtures/parser/conflicting_modifiers.lit");
    assert_eq!(
        parse_str("malformed.lit", malformed),
        parse_str("malformed.lit", malformed)
    );
}

#[test]
fn parsing_has_no_filesystem_effects() {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "litweb-parser-test-{}-{number}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let display_name = directory.join("would-be-input.lit");

    parse_str(display_name.to_string_lossy(), SUPPORTED).unwrap();
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);

    fs::remove_dir(directory).unwrap();
}
