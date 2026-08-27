use std::path::Path;

use litweb::parser::parse_str;
use litweb::tangler::{TanglePlan, plan_tangle};

const HELLO: &str = include_str!("../examples/hello.lit");
const HANGMAN: &str = include_str!("../examples/hangman.lit");
const WC: &str = include_str!("../examples/wc.lit");

fn plan(file: &str, source: &str) -> TanglePlan {
    let program = parse_str(file, source).expect("example should parse");
    plan_tangle(&program).expect("example should tangle")
}

#[test]
fn hello_example_tangles_to_the_reviewed_c_program() {
    let result = plan("examples/hello.lit", HELLO);

    assert!(result.warnings.is_empty());
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0].relative_path, Path::new("hello.c"));
    assert_eq!(
        result.outputs[0].bytes,
        concat!(
            "// hello.c\n",
            "// Includes\n",
            "#include <stdio.h>\n",
            "\n",
            "int main() {\n",
            "    // Print a string\n",
            "    printf(\"Hello, world!\\n\");\n",
            "    return 0;\n",
            "}\n",
        )
        .as_bytes()
    );
}

#[test]
fn hangman_example_tangles_to_python_three_and_its_word_file() {
    let result = plan("examples/hangman.lit", HANGMAN);

    assert!(result.warnings.is_empty());
    assert_eq!(result.outputs.len(), 2);
    assert_eq!(result.outputs[0].relative_path, Path::new("hangman.py"));
    assert_eq!(result.outputs[1].relative_path, Path::new("words.txt"));

    let python = std::str::from_utf8(&result.outputs[0].bytes).unwrap();
    assert!(python.starts_with("# hangman.py\nimport random\nimport sys\n"));
    assert!(python.contains("print(\"Welcome to hangman!\")"));
    assert!(python.contains("guess = input()"));
    assert!(python.contains("sys.exit()"));
    assert!(!python.contains("raw_input"));
    assert!(!python.contains("print \""));

    let words = std::str::from_utf8(&result.outputs[1].bytes).unwrap();
    let words = words.split_whitespace().collect::<Vec<_>>();
    assert_eq!(words.len(), 287);
    assert_eq!(words.first(), Some(&"able"));
    assert_eq!(words.last(), Some(&"young"));

    assert!(!HELLO.contains("@compiler"));
    assert!(!HANGMAN.contains("@compiler"));
}

#[test]
fn word_count_example_tangles_to_the_reviewed_c_program() {
    let result = plan("examples/wc.lit", WC);

    assert!(result.warnings.is_empty());
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0].relative_path, Path::new("wc.c"));

    let c = std::str::from_utf8(&result.outputs[0].bytes).unwrap();
    assert!(c.starts_with("/* wc.c */\n/* Header files to include */\n"));
    assert!(c.contains("#define OK 0"));
    assert!(c.contains("status |= cannot_open_file;"));
    assert!(c.contains("if ((status & usage_error) == 0)"));
    assert!(c.contains("status |= usage_error;"));
    assert!(c.contains("if (c > ' ' && c < 0177)"));
    assert!(c.contains("#define print_count(n) printf(\"%8ld\", n)"));
    assert!(c.contains("return status;"));

    let macro_explanation = WC.find("#define print_count").unwrap();
    let function_definition = WC.find("void wc_print").unwrap();
    assert!(macro_explanation < function_definition);
    assert!(!WC.contains("@compiler"));
}
