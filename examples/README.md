# Litweb Examples

These examples are adapted from the examples in Zachary Yedidia's
[Literate](https://github.com/zyedidia/literate). `hello.lit` is a small C
program, `wc.lit` is a detailed C word-count tutorial descended from the CWEB
example by Silvio Levy and Donald Knuth, and `hangman.lit` is a Hangman game
updated to Python 3.

From the repository root, generate the hello-world C source and HTML
explanation, then compile and run it with:

```sh
cargo lw -odir target/examples/hello examples/hello.lit
cc target/examples/hello/hello.c -o target/examples/hello/hello
target/examples/hello/hello
```

Generate, compile, and run the word-count example with:

```sh
cargo lw -odir target/examples/wc examples/wc.lit
cc target/examples/wc/wc.c -o target/examples/wc/wc
target/examples/wc/wc examples/wc.lit
```

The word-count program uses POSIX file operations and is intended for
Unix-like systems.

Generate and run Hangman with:

```sh
cargo lw -odir target/examples/hangman examples/hangman.lit
(cd target/examples/hangman && python3 hangman.py)
```
