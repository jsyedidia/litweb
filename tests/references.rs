use litweb::parser::parse_str;
use litweb::tangler::plan_tangle;
use litweb::weaver::plan_weave;

const REFERENCE_GRAMMAR: &str = concat!(
    "@title Reference grammar\n",
    "@s Uses\n",
    "--- output.txt\n",
    "inline @{Piece} stays literal\n",
    "    @{Piece}   \n",
    "@{Piece} suffix stays literal\n",
    "---\n",
    "--- Piece\n",
    "expanded\n",
    "---\n",
);

#[test]
fn tangler_and_weaver_share_the_whole_line_reference_grammar() {
    let program = parse_str("references.lit", REFERENCE_GRAMMAR).unwrap();

    let tangle = plan_tangle(&program).unwrap();
    assert_eq!(tangle.outputs.len(), 1);
    assert_eq!(
        tangle.outputs[0].bytes,
        b"inline @{Piece} stays literal\n    expanded\n@{Piece} suffix stays literal\n"
    );

    let weave = plan_weave(&program).unwrap();
    assert_eq!(weave.outputs.len(), 1);
    let html = String::from_utf8(weave.outputs[0].bytes.clone()).unwrap();
    assert!(html.contains("inline @{Piece} stays literal"));
    assert!(html.contains("@{Piece} suffix stays literal"));
    assert_eq!(html.matches("class=\"nocode\"").count(), 1);
}
