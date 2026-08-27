use litweb::identifier::{
    IdentifierEntry, IdentifierIndex, IdentifierKind, IdentifierMeaning,
    IdentifierNamespace, IdentifierRole, analyze,
};
use litweb::parser::parse_str;

const DOCUMENTED_NOTES_GRAPH: &str = include_str!("fixtures/documented/notes_graph.lit");

fn index(source: &str) -> IdentifierIndex {
    let program = parse_str("identifiers.lit", source).expect("fixture should parse");
    analyze(&program)
}

fn entry<'a>(index: &'a IdentifierIndex, name: &str) -> &'a IdentifierEntry {
    index
        .entries()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("missing identifier {name:?}: {index:#?}"))
}

fn names(index: &IdentifierIndex) -> Vec<&str> {
    index
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
}

fn has_definition(entry: &IdentifierEntry, namespace: IdentifierNamespace) -> bool {
    entry.occurrences.iter().any(|occurrence| {
        occurrence.role == IdentifierRole::Definition && occurrence.namespace == namespace
    })
}

fn meanings<'a>(index: &'a IdentifierIndex, name: &str) -> Vec<&'a IdentifierMeaning> {
    index
        .meanings()
        .iter()
        .filter(|meaning| meaning.name == name)
        .collect()
}

fn chapter_index(chapters: &[&str]) -> IdentifierIndex {
    let mut program = parse_str("book.lit", "@code_type rust .rs\n")
        .expect("empty book shell should parse");
    program.chapters.clear();
    for (chapter, source) in chapters.iter().enumerate() {
        let parsed = parse_str(format!("chapter-{}.lit", chapter + 1), source)
            .expect("book chapter should parse");
        program.chapters.extend(parsed.chapters);
    }
    analyze(&program)
}

#[test]
fn documented_notes_and_graph_produce_the_explained_source_index() {
    let index = index(DOCUMENTED_NOTES_GRAPH);

    assert_eq!(
        names(&index),
        vec![
            "Graph",
            "graph",
            "initial_count",
            "make_graph",
            "parse_str",
            "vertex_count",
        ]
    );

    let occurrences = |name| {
        entry(&index, name)
            .occurrences
            .iter()
            .map(|occurrence| {
                (
                    occurrence.role,
                    occurrence.namespace,
                    occurrence.site.origin.line,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        occurrences("Graph"),
        vec![
            (IdentifierRole::Use, IdentifierNamespace::Type, 13),
            (IdentifierRole::Definition, IdentifierNamespace::Type, 16,),
            (IdentifierRole::Use, IdentifierNamespace::Type, 20),
            (IdentifierRole::Use, IdentifierNamespace::Type, 21),
        ]
    );
    assert_eq!(
        occurrences("graph"),
        vec![
            (
                IdentifierRole::Definition,
                IdentifierNamespace::Ordinary,
                21,
            ),
            (IdentifierRole::Use, IdentifierNamespace::Ordinary, 24),
        ]
    );
    assert_eq!(
        occurrences("initial_count"),
        vec![
            (
                IdentifierRole::Definition,
                IdentifierNamespace::Ordinary,
                20,
            ),
            (IdentifierRole::Use, IdentifierNamespace::Ordinary, 22),
        ]
    );
    assert_eq!(
        occurrences("make_graph"),
        vec![(
            IdentifierRole::Definition,
            IdentifierNamespace::Ordinary,
            20,
        )]
    );
    assert_eq!(
        occurrences("parse_str"),
        vec![(IdentifierRole::Use, IdentifierNamespace::Ordinary, 3,)]
    );
    assert_eq!(
        occurrences("vertex_count"),
        vec![
            (
                IdentifierRole::Definition,
                IdentifierNamespace::Ordinary,
                17,
            ),
            (IdentifierRole::Use, IdentifierNamespace::Ordinary, 22),
        ]
    );
}

#[test]
fn rust_lexing_keeps_identifiers_out_of_comments_literals_and_numbers() {
    let index = index(
        r##"@code_type rust .rs
@s Lexical forms
--- lexical.rs
fn visible() {
    let café = 1u32;
    let r#match = café;
    let text = "string_hidden";
    let raw = r#"raw_hidden"#;
    let bytes = b"byte_hidden";
    let raw_bytes = br#"raw_byte_hidden"#;
    let c_text = c"c_hidden";
    let raw_c_text = cr#"raw_c_hidden"#;
    let character = 'x';
    let byte_character = b'y';
    // line_hidden
    /* outer_hidden /* nested_hidden */ still_hidden */
    visible();
}
---
"##,
    );

    assert!(has_definition(
        entry(&index, "visible"),
        IdentifierNamespace::Ordinary
    ));
    assert_eq!(entry(&index, "visible").occurrences.len(), 2);
    assert!(has_definition(
        entry(&index, "café"),
        IdentifierNamespace::Ordinary
    ));
    assert_eq!(entry(&index, "café").occurrences.len(), 2);
    assert!(has_definition(
        entry(&index, "r#match"),
        IdentifierNamespace::Ordinary
    ));

    for absent in [
        "string_hidden",
        "raw_hidden",
        "byte_hidden",
        "raw_byte_hidden",
        "c_hidden",
        "raw_c_hidden",
        "line_hidden",
        "outer_hidden",
        "nested_hidden",
        "still_hidden",
        "u32",
        "x",
        "y",
    ] {
        assert!(!names(&index).contains(&absent), "{absent}: {index:#?}");
    }
}

#[test]
fn an_unclosed_literal_recovers_without_failing_identifier_analysis() {
    let index = index(
        r#"@code_type rust .rs
@s Incomplete
--- incomplete.rs
fn before_incomplete() {}
let broken = "unterminated
fn after_incomplete() {}
---
"#,
    );

    entry(&index, "before_incomplete");
    assert!(!names(&index).contains(&"unterminated"));
    assert!(!names(&index).contains(&"after_incomplete"));
}

#[test]
fn declarations_cover_items_fields_variants_generics_and_bindings() {
    let index = index(
        r#"@code_type rust .rs
@s Declarations
--- declarations.rs
struct Widget<T> {
    field: T,
}

enum Choice {
    Empty,
    Named { value: Widget<u8> },
}

trait Service {
    type Item;
    fn call(&self, input: Self::Item) -> Widget<Self::Item>;
}

#[index_attribute]
fn make<'long, const N: usize>(input: Widget<u8>, values: Vec<Widget<u8>>) -> Widget<u8> {
    let local: Widget<u8> = input;
    let selected = local.field;
    local_macro!(selected);
    module_path::function_call();
    for element in values {
        let closure = |captured: Widget<u8>| captured;
        match element {
            Choice::Named { value: renamed } => closure(renamed),
            Choice::Empty => local,
        }
    }
}
---
"#,
    );

    for type_name in ["Widget", "Choice", "Service", "Item", "T"] {
        assert!(
            has_definition(entry(&index, type_name), IdentifierNamespace::Type),
            "{type_name}: {index:#?}"
        );
    }
    for ordinary_name in [
        "field", "Empty", "Named", "value", "call", "make", "N", "input", "values",
        "local", "selected", "element", "closure", "captured", "renamed",
    ] {
        assert!(
            has_definition(entry(&index, ordinary_name), IdentifierNamespace::Ordinary),
            "{ordinary_name}: {index:#?}"
        );
    }
    for used_name in [
        "index_attribute",
        "local_macro",
        "module_path",
        "function_call",
    ] {
        assert_eq!(
            entry(&index, used_name).occurrences[0].role,
            IdentifierRole::Use,
            "{used_name}: {index:#?}"
        );
    }
    assert_eq!(entry(&index, "field").occurrences.len(), 2);

    assert!(!names(&index).contains(&"Vec"));
    assert!(!names(&index).contains(&"long"));
    assert!(!names(&index).contains(&"u8"));
    assert!(!names(&index).contains(&"usize"));
}

#[test]
fn nested_named_blocks_preserve_a_field_definition_at_its_visible_source() {
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
    let program = parse_str("identifiers.lit", source).expect("fixture should parse");
    let stored_field_block = program.chapters[0].sections[0]
        .blocks
        .iter()
        .position(|block| block.code().is_some_and(|code| code.name == "Stored field"))
        .expect("stored field block should be visible");
    let index = analyze(&program);

    let field = entry(&index, "vertex_count");
    assert_eq!(field.occurrences.len(), 1);
    assert_eq!(field.occurrences[0].role, IdentifierRole::Definition);
    assert_eq!(
        field.occurrences[0].namespace,
        IdentifierNamespace::Ordinary
    );
    assert_eq!(field.occurrences[0].site.origin.line, 12);
    assert_eq!(field.occurrences[0].site.block, stored_field_block);
}

#[test]
fn reused_contexts_deduplicate_one_source_token_and_definition_wins() {
    let index = index(
        r#"@code_type rust .rs
@s Reuse
--- first.rs
fn first(input: usize) {
    let (
        @{Shared name}
    ) = (input,);
}
---
--- second.rs
fn second() {
    @{Shared name}
    @{Shared name}
}
---
--- Shared name
value
---
"#,
    );

    let value = entry(&index, "value");
    assert_eq!(value.occurrences.len(), 1);
    assert_eq!(value.occurrences[0].role, IdentifierRole::Definition);
    assert_eq!(value.occurrences[0].site.origin.line, 17);
    entry(&index, "first");
    entry(&index, "second");
}

#[test]
fn contextual_lexing_keeps_cross_block_literal_contents_out_of_the_index() {
    let index = index(
        r##"@code_type rust .rs
@s Literal
--- literal.rs
fn text() -> &'static str {
    r#"
    @{String contents}
    "#
}
---
--- String contents
hidden_identifier
---
"##,
    );

    assert!(has_definition(
        entry(&index, "text"),
        IdentifierNamespace::Ordinary
    ));
    assert!(!names(&index).contains(&"hidden_identifier"));
}

#[test]
fn missing_and_cyclic_contexts_keep_locally_analyzable_names() {
    let index = index(
        r#"@code_type rust .rs
@s Recovery
--- recovery.rs
fn before_reference() {}
@{Missing}
@{Cycle}
fn after_reference() {}
---
--- Cycle
fn inside_cycle() {}
@{Cycle}
---
"#,
    );

    for present in ["before_reference", "inside_cycle", "after_reference"] {
        assert!(
            has_definition(entry(&index, present), IdentifierNamespace::Ordinary),
            "{present}: {index:#?}"
        );
    }
}

#[test]
fn prose_language_scope_visibility_and_sgb_filters_are_applied() {
    let index = index(
        r#"@code_type rust .rs
@s Selection
The early @code{early_use()} is Rust.
@code_type none
The @code{disabled_use()} is not.
@code_type rust .rs
The @code{mentioned()} and **the next line's
@code{strong_use()}** are uses.
The bare @code{ExternalMention} looks like an external type.
The `@code{shielded_use()}` and \@code{escaped_use()} remain literal.
Even @code{fn prose_only() {}} is a prose use rather than a definition.

--- root.rs
use external::ImportedType;

struct LocalType;

fn consume(value: LocalType, imported: ImportedType) {
    let x: ExternalType = external_call();
    x;
    @{Other}
}
---

--- Other
fn other() {
    mentioned();
}
---

--- Hidden --- noWeave
fn hidden_name() {}
---
"#,
    );

    for present in [
        "early_use",
        "mentioned",
        "strong_use",
        "prose_only",
        "LocalType",
        "consume",
        "value",
        "external_call",
        "other",
    ] {
        entry(&index, present);
    }
    for absent in [
        "disabled_use",
        "shielded_use",
        "escaped_use",
        "ExternalMention",
        "ExternalType",
        "ImportedType",
        "Other",
        "hidden_name",
    ] {
        assert!(!names(&index).contains(&absent), "{absent}: {index:#?}");
    }

    assert_eq!(
        entry(&index, "prose_only").occurrences[0].role,
        IdentifierRole::Use
    );
    assert!(has_definition(
        entry(&index, "LocalType"),
        IdentifierNamespace::Type
    ));
    assert!(has_definition(
        entry(&index, "x"),
        IdentifierNamespace::Ordinary
    ));
    assert_eq!(entry(&index, "x").occurrences.len(), 1);
}

#[test]
fn fenced_prose_is_opaque_to_identifier_analysis() {
    let index = index(
        r#"@code_type rust .rs
@s Examples
The ordinary @code{outside_fence()} is a use.

```rust
@code{inside_backtick_fence()}
```

~~~text
@code{inside_tilde_fence()}
~~~

The later @code{after_fence()} is also a use.

```rust
An unclosed fence leaves @code{visible_after_unclosed()} as prose.
"#,
    );

    for present in ["outside_fence", "after_fence", "visible_after_unclosed"] {
        entry(&index, present);
    }
    for absent in ["inside_backtick_fence", "inside_tilde_fence"] {
        assert!(!names(&index).contains(&absent), "{absent}: {index:#?}");
    }
}

#[test]
fn table_cells_index_active_target_code_but_keep_opaque_forms_hidden() {
    let index = index(
        r#"@code_type rust .rs
@s Table
Form | Example
--- | ---
active | @code{table_use(left | right)}
math | $@code{hidden_in_math()} \mid x$
literal | `@code{hidden_in_backticks()}`
"#,
    );

    for present in ["table_use", "left", "right"] {
        entry(&index, present);
    }
    for absent in ["hidden_in_math", "hidden_in_backticks"] {
        assert!(!names(&index).contains(&absent), "{absent}: {index:#?}");
    }
}

#[test]
fn entries_are_deterministically_alphabetized_with_case_variants_together() {
    let index = index(
        r#"@code_type rust .rs
@s Ordering
--- order.rs
fn beta_name() {}
fn AlphaName() {}
fn alpha_name() {}
fn álpha_name() {}
---
"#,
    );

    assert_eq!(
        names(&index),
        vec!["alpha_name", "AlphaName", "beta_name", "álpha_name"]
    );
}

#[test]
fn an_unsupported_language_produces_no_identifier_entries() {
    let index = index(
        r#"@code_type c .c
@s C
The @code{prose_name} is C.
--- example.c
int code_name(void) { return 0; }
---
"#,
    );

    assert!(index.is_empty());
}

#[test]
fn mini_index_meanings_are_reliable_and_ambiguous_uses_remain_unbound() {
    let index = index(
        r#"@code_type rust .rs
@s Meanings
The later implementation works with @code{Graph}.
--- meanings.rs
use std::path::Path;

struct Graph {
    label: String,
}

const LIMIT: usize = 8;

fn build(input: Graph, path: Path) -> Graph {
    let scratch = input;
    scratch.label;
    external_call(path);
    scratch
}

fn first() {
    let repeated = LIMIT;
    repeated;
}

fn second() {
    let repeated = LIMIT;
    repeated;
}
---
"#,
    );

    for (name, kind) in [
        ("Path", IdentifierKind::Import),
        ("Graph", IdentifierKind::Struct),
        ("label", IdentifierKind::Field),
        ("LIMIT", IdentifierKind::Constant),
        ("build", IdentifierKind::Function),
        ("input", IdentifierKind::Parameter),
        ("scratch", IdentifierKind::LocalVariable),
    ] {
        let found = meanings(&index, name);
        assert_eq!(found.len(), 1, "{name}: {:#?}", index.meanings());
        assert_eq!(found[0].kind, kind, "{name}: {:#?}", index.meanings());
    }

    let graph = meanings(&index, "Graph")[0];
    assert!(index.meaning_occurrences().iter().any(|occurrence| {
        occurrence.meaning == graph.id && occurrence.role == IdentifierRole::Use
    }));
    let label = meanings(&index, "label")[0];
    assert!(index.meaning_occurrences().iter().any(|occurrence| {
        occurrence.meaning == label.id && occurrence.role == IdentifierRole::Use
    }));

    let repeated = meanings(&index, "repeated");
    assert_eq!(repeated.len(), 2);
    assert!(repeated.iter().all(|meaning| {
        index.meaning_occurrences().iter().all(|occurrence| {
            occurrence.meaning != meaning.id
                || occurrence.role == IdentifierRole::Definition
        })
    }));
    assert!(meanings(&index, "external_call").is_empty());
}

#[test]
fn mini_index_meanings_never_look_into_later_chapters() {
    let index = chapter_index(&[
        r#"@code_type rust .rs
@s First
--- first.rs
fn before() {
    future();
}

fn stable() {}
---
"#,
        r#"@code_type rust .rs
@s Second
--- second.rs
fn middle() {
    local_later();
    stable();
}

fn local_later() {}
---
"#,
        r#"@code_type rust .rs
@s Third
--- third.rs
fn future() {}
fn stable() {}
---
"#,
    ]);

    let future = meanings(&index, "future");
    assert_eq!(future.len(), 1, "{index:#?}");
    assert_eq!(future[0].definition.chapter, 2);
    assert!(index.meaning_occurrences().iter().all(|occurrence| {
        occurrence.meaning != future[0].id
            || occurrence.role == IdentifierRole::Definition
    }));
    assert_eq!(
        entry(&index, "future")
            .occurrences
            .iter()
            .map(|occurrence| (occurrence.role, occurrence.site.chapter))
            .collect::<Vec<_>>(),
        vec![(IdentifierRole::Use, 0), (IdentifierRole::Definition, 2),]
    );

    let local_later = meanings(&index, "local_later")[0];
    assert_eq!(local_later.definition.chapter, 1);
    assert!(index.meaning_occurrences().iter().any(|occurrence| {
        occurrence.meaning == local_later.id
            && occurrence.role == IdentifierRole::Use
            && occurrence.site.chapter == 1
    }));

    let stable = meanings(&index, "stable");
    assert_eq!(stable.len(), 2, "{index:#?}");
    let earlier = stable
        .iter()
        .find(|meaning| meaning.definition.chapter == 0)
        .expect("earlier stable definition");
    let later = stable
        .iter()
        .find(|meaning| meaning.definition.chapter == 2)
        .expect("later stable definition");
    assert!(index.meaning_occurrences().iter().any(|occurrence| {
        occurrence.meaning == earlier.id
            && occurrence.role == IdentifierRole::Use
            && occurrence.site.chapter == 1
    }));
    assert!(index.meaning_occurrences().iter().all(|occurrence| {
        occurrence.meaning != later.id || occurrence.role == IdentifierRole::Definition
    }));

    assert!(index.meaning_occurrences().iter().all(|occurrence| {
        occurrence.role == IdentifierRole::Definition
            || index.meanings()[occurrence.meaning].definition.chapter
                <= occurrence.site.chapter
    }));
}

#[test]
fn impl_for_does_not_turn_later_types_into_loop_bindings() {
    let index = index(
        r#"@code_type rust .rs
@s For forms
--- forms.rs
struct UtilityTypes;

impl Default for UtilityTypes {
    fn default() -> Self {
        Self
    }
}

#[derive(Default)]
enum UtilityValue {
    #[default]
    Unused,
    String(Option<String>),
}

fn with_callback<F>(callback: F)
where
    F: for<'a> Fn(&'a str),
{
    callback("ready");
}

struct Pair {
    left: usize,
    right: usize,
}

fn visit(pairs: Vec<Pair>) {
    for Pair { left, right } in pairs {
        let total = left + right;
        consume(total);
    }
}
---
"#,
    );

    for external in ["Default", "Fn", "Vec"] {
        assert!(meanings(&index, external).is_empty(), "{index:#?}");
    }
    let string = meanings(&index, "String");
    assert_eq!(string.len(), 1, "{index:#?}");
    assert_eq!(string[0].kind, IdentifierKind::Variant);
    assert!(index.meaning_occurrences().iter().all(|occurrence| {
        occurrence.meaning != string[0].id
            || occurrence.role == IdentifierRole::Definition
    }));

    for name in ["left", "right", "total"] {
        assert!(
            meanings(&index, name)
                .iter()
                .any(|meaning| meaning.kind == IdentifierKind::LocalVariable),
            "{name}: {index:#?}"
        );
    }
}

#[test]
fn literal_or_patterns_do_not_open_closures_across_later_code() {
    let index = index(
        r#"@code_type rust .rs
@s Literal alternatives
--- options.rs
fn classify(argument: Option<&str>) {
    match argument {
        Some("-h" | "--help") => show_help(),
        _ => {}
    }
}

fn parse_number(text: &str) -> Result<usize, ()> {
    text.parse().map_err(|_| ())
}
---
"#,
    );

    for external in ["Option", "Result", "Some"] {
        assert!(meanings(&index, external).is_empty(), "{index:#?}");
    }
    for (name, kind) in [
        ("argument", IdentifierKind::Parameter),
        ("text", IdentifierKind::Parameter),
    ] {
        let found = meanings(&index, name);
        assert_eq!(found.len(), 1, "{name}: {index:#?}");
        assert_eq!(found[0].kind, kind, "{name}: {index:#?}");
    }
}

#[test]
fn method_calls_and_field_accesses_select_different_meanings() {
    let index = index(
        r#"@code_type rust .rs
@s Members
--- members.rs
struct Record {
    index: usize,
}

struct Id(usize);

impl Id {
    fn index(self) -> usize {
        self.0
    }
}

fn inspect(id: Id, record: Record) {
    consume(id.index());
    consume(record.index);
}
---
"#,
    );

    let index_meanings = meanings(&index, "index");
    assert_eq!(index_meanings.len(), 2, "{index:#?}");
    for kind in [IdentifierKind::Function, IdentifierKind::Field] {
        let meaning = index_meanings
            .iter()
            .find(|meaning| meaning.kind == kind)
            .unwrap_or_else(|| panic!("missing {kind:?}: {index:#?}"));
        let uses = index
            .meaning_occurrences()
            .iter()
            .filter(|occurrence| {
                occurrence.meaning == meaning.id && occurrence.role == IdentifierRole::Use
            })
            .count();
        assert_eq!(uses, 1, "{kind:?}: {index:#?}");
    }
}

#[test]
fn macro_calls_do_not_select_non_macro_meanings() {
    let index = index(
        r#"@code_type rust .rs
@s Macro calls
--- macros.rs
mod write {}

macro_rules! local_macro {
    () => {};
}

fn inspect() {
    let matches = 1;
    write!(sink, "value");
    matches!(matches, 1);
    local_macro!();
}
---
"#,
    );

    let write = meanings(&index, "write")[0];
    assert!(index.meaning_occurrences().iter().all(|occurrence| {
        occurrence.meaning != write.id || occurrence.role == IdentifierRole::Definition
    }));

    let matches = meanings(&index, "matches")[0];
    assert_eq!(
        index
            .meaning_occurrences()
            .iter()
            .filter(|occurrence| {
                occurrence.meaning == matches.id && occurrence.role == IdentifierRole::Use
            })
            .count(),
        1,
        "the macro argument, but not the macro name, uses the local variable: {index:#?}"
    );

    let local_macro = meanings(&index, "local_macro");
    assert_eq!(local_macro.len(), 1, "{index:#?}");
    assert_eq!(local_macro[0].kind, IdentifierKind::Macro);
    assert_eq!(
        index
            .meaning_occurrences()
            .iter()
            .filter(|occurrence| {
                occurrence.meaning == local_macro[0].id
                    && occurrence.role == IdentifierRole::Use
            })
            .count(),
        1,
        "{index:#?}"
    );
}
