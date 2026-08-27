#!/usr/bin/env python3
"""Compare Rust files or trees by their non-trivia token streams."""

from __future__ import annotations

import argparse
import difflib
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Token:
    kind: str
    text: str


MULTI_PUNCTUATION = (
    "<<=",
    ">>=",
    "...",
    "..=",
    "::",
    "->",
    "=>",
    "==",
    "!=",
    "<=",
    ">=",
    "&&",
    "||",
    "+=",
    "-=",
    "*=",
    "/=",
    "%=",
    "^=",
    "&=",
    "|=",
    "<<",
    ">>",
    "..",
)


def rust_tree(path: Path) -> dict[Path, Path]:
    if not path.is_dir():
        raise ValueError(f"not a directory: {path}")
    files = {
        source.relative_to(path): source
        for source in sorted(path.rglob("*.rs"))
        if source.is_file()
    }
    if not files:
        raise ValueError(f"no Rust source files under: {path}")
    return files


def is_identifier_start(character: str) -> bool:
    return character == "_" or character.isalpha() or ord(character) >= 128


def is_identifier_continue(character: str) -> bool:
    return is_identifier_start(character) or character.isdigit()


def raw_literal_end(source: str, start: int) -> int | None:
    for prefix in ("br", "cr", "r"):
        if not source.startswith(prefix, start):
            continue
        marker = start + len(prefix)
        hashes = 0
        while marker + hashes < len(source) and source[marker + hashes] == "#":
            hashes += 1
        quote = marker + hashes
        if quote >= len(source) or source[quote] != '"':
            continue
        terminator = '"' + "#" * hashes
        end = source.find(terminator, quote + 1)
        if end < 0:
            raise ValueError("unterminated raw string literal")
        return end + len(terminator)
    return None


def quoted_literal_end(source: str, quote: int) -> int:
    index = quote + 1
    while index < len(source):
        if source[index] == "\\":
            index += 2
            continue
        if source[index] == '"':
            return index + 1
        index += 1
    raise ValueError("unterminated string literal")


def character_literal_end(source: str, quote: int) -> int | None:
    index = quote + 1
    if index >= len(source) or source[index] in "\r\n'":
        return None
    if source[index] != "\\":
        index += 1
    elif index + 1 >= len(source):
        return None
    elif source[index + 1] == "x":
        index += 4
    elif source[index + 1] == "u" and index + 2 < len(source) and source[index + 2] == "{":
        closing_brace = source.find("}", index + 3)
        if closing_brace < 0:
            return None
        index = closing_brace + 1
    else:
        index += 2
    return index + 1 if index < len(source) and source[index] == "'" else None


def numeric_literal_end(source: str, start: int) -> int:
    index = start
    if source.startswith(("0b", "0o", "0x"), start):
        index += 2
        while index < len(source) and (source[index].isalnum() or source[index] == "_"):
            index += 1
        return index

    while index < len(source) and (source[index].isdigit() or source[index] == "_"):
        index += 1
    if index < len(source) and source[index] == "." and not source.startswith("..", index):
        index += 1
        while index < len(source) and (source[index].isdigit() or source[index] == "_"):
            index += 1
    if index < len(source) and source[index] in "eE":
        index += 1
        if index < len(source) and source[index] in "+-":
            index += 1
        while index < len(source) and (source[index].isdigit() or source[index] == "_"):
            index += 1
    while index < len(source) and is_identifier_continue(source[index]):
        index += 1
    return index


def block_comment_end(source: str, start: int) -> int:
    depth = 1
    index = start + 2
    while index < len(source) and depth:
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    if depth:
        raise ValueError("unterminated block comment")
    return index


def is_line_doc_comment(source: str, index: int) -> bool:
    return source.startswith("//!", index) or (
        source.startswith("///", index) and not source.startswith("////", index)
    )


def is_block_doc_comment(source: str, index: int) -> bool:
    return source.startswith("/*!", index) or (
        source.startswith("/**", index) and not source.startswith("/***", index)
    )


def rust_tokens(source: str) -> list[Token]:
    """Return Rust tokens, ignoring ordinary comments and outside whitespace.

    Literal spelling and documentation comments remain exact. In particular,
    whitespace and comment-looking lines inside raw strings are data rather
    than trivia.
    """

    tokens = []
    index = 0
    while index < len(source):
        character = source[index]
        if character.isspace():
            index += 1
            continue

        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            if end < 0:
                end = len(source)
            if is_line_doc_comment(source, index):
                tokens.append(Token("doc-comment", source[index:end]))
            index = end
            continue

        if source.startswith("/*", index):
            end = block_comment_end(source, index)
            if is_block_doc_comment(source, index):
                tokens.append(Token("doc-comment", source[index:end]))
            index = end
            continue

        end = raw_literal_end(source, index)
        if end is not None:
            tokens.append(Token("literal", source[index:end]))
            index = end
            continue

        string_prefix = next(
            (prefix for prefix in ('b"', 'c"', '"') if source.startswith(prefix, index)),
            None,
        )
        if string_prefix is not None:
            quote = index + len(string_prefix) - 1
            end = quoted_literal_end(source, quote)
            tokens.append(Token("literal", source[index:end]))
            index = end
            continue

        if source.startswith("b'", index):
            end = character_literal_end(source, index + 1)
            if end is None:
                raise ValueError("invalid byte character literal")
            tokens.append(Token("literal", source[index:end]))
            index = end
            continue

        if character == "'":
            end = character_literal_end(source, index)
            if end is not None:
                tokens.append(Token("literal", source[index:end]))
                index = end
                continue
            tokens.append(Token("punctuation", character))
            index += 1
            continue

        if source.startswith("r#", index) and index + 2 < len(source) and is_identifier_start(source[index + 2]):
            end = index + 3
            while end < len(source) and is_identifier_continue(source[end]):
                end += 1
            tokens.append(Token("identifier", source[index:end]))
            index = end
            continue

        if is_identifier_start(character):
            end = index + 1
            while end < len(source) and is_identifier_continue(source[end]):
                end += 1
            tokens.append(Token("identifier", source[index:end]))
            index = end
            continue

        if character.isdigit():
            end = numeric_literal_end(source, index)
            tokens.append(Token("literal", source[index:end]))
            index = end
            continue

        punctuation = next(
            (value for value in MULTI_PUNCTUATION if source.startswith(value, index)),
            character,
        )
        tokens.append(Token("punctuation", punctuation))
        index += len(punctuation)

    return tokens


def token_lines(path: Path) -> list[str]:
    source = path.read_text(encoding="utf-8")
    try:
        tokens = rust_tokens(source)
    except ValueError as error:
        raise ValueError(f"{path}: {error}") from error
    return [f"{token.kind}: {token.text!r}\n" for token in tokens]


def compare(before: Path, after: Path) -> bool:
    if before.is_file() and after.is_file():
        if before.suffix != ".rs" or after.suffix != ".rs":
            raise ValueError("both input files must have the .rs extension")
        before_files = {Path("."): before}
        after_files = {Path("."): after}
    elif before.is_dir() and after.is_dir():
        before_files = rust_tree(before)
        after_files = rust_tree(after)
    else:
        raise ValueError("inputs must both be Rust files or both be directories")
    before_names = set(before_files)
    after_names = set(after_files)
    matches = True

    for relative in sorted(before_names - after_names):
        print(f"only before: {relative}", file=sys.stderr)
        matches = False
    for relative in sorted(after_names - before_names):
        print(f"only after: {relative}", file=sys.stderr)
        matches = False

    for relative in sorted(before_names & after_names):
        before_tokens = token_lines(before_files[relative])
        after_tokens = token_lines(after_files[relative])
        if before_tokens == after_tokens:
            continue
        matches = False
        diff = difflib.unified_diff(
            before_tokens,
            after_tokens,
            fromfile=str(before_files[relative]),
            tofile=str(after_files[relative]),
        )
        sys.stderr.writelines(diff)

    if matches:
        print(
            "Rust tokens match after ignoring ordinary comments and "
            f"outside-literal whitespace: files={len(before_names)}"
        )
    return matches


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Compare Rust files or directory trees by token sequence, ignoring "
            "ordinary comments and whitespace outside literals while preserving "
            "literal contents and documentation comments exactly."
        )
    )
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        return 0 if compare(args.before, args.after) else 1
    except (OSError, UnicodeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
