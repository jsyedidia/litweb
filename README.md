# Litweb

Litweb is a modern literate-programming system descended from Zachary
Yedidia's [Literate](https://github.com/zyedidia/literate). Its `lw` command
reads `.lit` files and books, tangles their named code blocks into source
files, and weaves their explanations as HTML or LuaLaTeX.

This is an early public release. Litweb is already useful for everyday literate
programming, but its interface and supported formats may continue to grow
before version 1.0.

## Features

- **Readable source output.** Litweb expands named blocks with correct
  indentation and useful block-name comments, producing ordinary source files
  suitable for reading, reviewing, editing, and compiling. It preserves the
  formatting written in the `.lit` file rather than invoking a
  language-specific formatter.
- **A complete working example.** The Rust files under [`src/`](src/) were
  tangled directly from the canonical [`lit/`](lit/) book. They are committed
  as normal source, so users can build Litweb without already having `lw`.
- **Language-independent tangling.** A document can generate one or more files
  in Rust, C, Python, or another textual programming language.
- **Readable literate source.** Markdown-like prose, equations, and named code
  blocks keep a `.lit` file useful even before it is woven.
- **Documents and books.** Litweb handles standalone chapters as well as
  ordered, multi-chapter books with navigation and a table of contents.
- **HTML and LuaLaTeX output.** HTML is self-contained for offline reading and
  supports syntax highlighting and equations. LuaLaTeX produces printable
  books with internal links and, for Rust, identifier indexes and
  mini-indexes.
- **Linked explanations.** Woven output connects named-block definitions,
  additions, replacements, and uses to the sections where they appear.

## Documentation

- Read the [user manual](docs/manual.md) for the command-line interface,
  `.lit` syntax, weaving, books, equations, and indexes.
- Work through the [`examples/`](examples/README.md) for complete small
  programs in C and Python.
- Read [`lit/index.lit`](lit/index.lit) and its chapters for Litweb's
  substantial self-hosted implementation.

## Install or build `lw`

Litweb requires Rust 1.97 or newer and its Cargo build tool. If they are not
already installed, follow the
[official Rust installation instructions](https://rust-lang.org/install.html).
Installing Rust with `rustup` also installs Cargo.

On macOS or Linux, the simplest way to install `lw` from the repository root
is:

```sh
make install
```

This builds the release executable and installs it as `~/.cargo/bin/lw`. On
Windows, or on a system without `make`, use Cargo directly:

```sh
cargo install --locked --path .
```

Cargo normally installs the program as `~/.cargo/bin/lw` on macOS and Linux,
or `%USERPROFILE%\.cargo\bin\lw.exe` on Windows. A custom Cargo installation
root can change that location. Rust's standard installer normally adds this
binary directory to the command search path; after opening a new terminal,
confirm that `lw` is available with:

```sh
lw --version
```

To build without installing, run:

```sh
cargo build --release
```

The resulting program is `target/release/lw` (`target\release\lw.exe` on
Windows). See its command-line options with `lw --help` after installation, or
with `target/release/lw --help` from the repository root.

## Try an example

The smallest example tangles a C program and weaves its explanation:

```sh
lw --out-dir target/examples/hello examples/hello.lit
cc target/examples/hello/hello.c -o target/examples/hello/hello
target/examples/hello/hello
```

The generated directory also contains `hello.html`. See
[`examples/README.md`](examples/README.md) for the C word-count tutorial and
Python Hangman example as well as the corresponding commands.

For your own file, the basic form is:

```sh
lw --out-dir generated program.lit
```

By default this tangles source and weaves HTML. Use `--tangle` or `--weave` to
request only one operation, and `--format latex` to weave LuaLaTeX. Run
`lw --help` for the complete command-line interface.

## Litweb is written in Litweb

Litweb's canonical implementation is the book
[`lit/index.lit`](lit/index.lit), whose manifest points to the individual
`.lit` chapters under [`lit/`](lit/). Those files explain the program while
tangling the readable Rust committed under [`src/`](src/).

The committed Rust means an ordinary Cargo build does not require an existing
`lw`. To see the woven output from this substantial Litweb program, run:

```sh
make build
```

Then open `html/index.html` in a browser. It is the book's table of contents
and links to the woven chapters, which together explain Litweb's own
implementation.

The same command is also the normal regeneration workflow after editing the
canonical `.lit` sources: it regenerates the Rust under `src/`, builds `lw`,
and replaces the woven book under `html/`.

The repository-local bootstrap tool performs that regeneration without
depending on a previously installed Litweb. Its lower-level commands are
documented in [`tools/bootstrap/README.md`](tools/bootstrap/README.md).

## Modifying Litweb

If you modify Litweb for your own purposes, edit the canonical files under
[`lit/`](lit/) rather than the generated Rust under [`src/`](src/). Run
`make build` to regenerate `src/` and the HTML book, followed by `make test` to
check the result.

The project is not accepting pull requests at present. Requests for new
features may be submitted through the
[GitHub issue tracker](https://github.com/jsyedidia/litweb/issues).

## Test and typeset the book

Run the ordinary Rust tests with:

```sh
cargo test --locked
```

The complete project check, including formatting, Clippy, documentation,
release packaging, and exact self-regeneration, is:

```sh
make test
```

To generate the Litweb book as LuaLaTeX without compiling it, or to compile a
PDF when Latexmk and a suitable TeX Live installation are available, run:

```sh
make latex
make pdf
```

Neither the ordinary Cargo build nor the test suite requires TeX.

## License

Litweb is distributed under the [MIT License](LICENSE).
