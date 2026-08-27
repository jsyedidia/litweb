import unittest

from compare_rust_code import rust_tokens


class RustTokenTests(unittest.TestCase):
    def assert_same(self, before: str, after: str) -> None:
        self.assertEqual(rust_tokens(before), rust_tokens(after))

    def assert_different(self, before: str, after: str) -> None:
        self.assertNotEqual(rust_tokens(before), rust_tokens(after))

    def test_ignores_ordinary_comments_and_outside_whitespace(self) -> None:
        self.assert_same(
            "fn example() { let value = 1 + 2; }",
            "fn  example ( ) { // fragment\n let value=1/* nested /* comment */ ok */+2;}",
        )

    def test_preserves_raw_string_contents(self) -> None:
        self.assert_different(
            'const STYLE: &str = r#"a\n// data\n\nb"#;',
            'const STYLE: &str = r#"a\n// data\nb"#;',
        )

    def test_preserves_normal_string_and_character_literals(self) -> None:
        self.assert_different('let text = "a b";', 'let text = "ab";')
        self.assert_same("let value = 'λ';", "let value='λ';")

    def test_preserves_documentation_comments(self) -> None:
        self.assert_different("/// first\nfn item() {}", "/// second\nfn item() {}")
        self.assert_different("/*! first */ fn item() {}", "/*! second */ fn item() {}")

    def test_preserves_token_boundaries_and_joint_punctuation(self) -> None:
        self.assert_different("let a b;", "let ab;")
        self.assert_different("macro_rules! m { (>>=) => {} }", "macro_rules! m { (> >=) => {} }")

    def test_preserves_numeric_literal_boundaries(self) -> None:
        self.assert_different("let value = 1e-3;", "let value = 1e - 3;")
        self.assert_different("let value = 1u32;", "let value = 1 u32;")

    def test_distinguishes_raw_identifiers_from_raw_strings(self) -> None:
        self.assert_same('let r#type = br##"bytes"##;', 'let r#type=br##"bytes"##;')
        self.assert_different("let r#type = 1;", "let r # type = 1;")

    def test_rejects_unterminated_literals_and_comments(self) -> None:
        with self.assertRaisesRegex(ValueError, "unterminated raw string"):
            rust_tokens('let value = r#"unfinished";')
        with self.assertRaisesRegex(ValueError, "unterminated block comment"):
            rust_tokens("fn item() { /* unfinished")


if __name__ == "__main__":
    unittest.main()
