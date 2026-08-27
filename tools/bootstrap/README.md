# Litweb Bootstrap Tool

This dependency-free development tool checks that the canonical Literate Rust
under `lit/` regenerates every committed Rust module under `src/`.

For routine work, edit the canonical `.lit` sources and run `make build` from
the checkout root. That target safely stages and installs regenerated Rust,
builds `lw`, and writes woven documentation to the git-ignored `html/`
directory. Run `make test` for the complete local verification suite. The
commands below expose the lower-level bootstrap operations for direct review
and diagnosis.

Run the non-mutating check from the checkout root:

```sh
cargo run --locked --offline --example bootstrap -- check
```

The tool makes an isolated release build of `lw` with the current Cargo
toolchain, tangles the canonical `lit/index.lit` book once into a temporary
tree, validates its complete declared output set, checks the untouched
generated Rust with the current Rustfmt using edition 2024 and the repository's
`rustfmt.toml`, and compares exact bytes with `src/`. A formatting failure
aborts with Rustfmt's diff before any staged or committed source can be
installed. The tool does not use the old D implementation and never updates
committed source.

The tool locates the repository from its own manifest rather than the current
directory, so the same command also works elsewhere when `--manifest-path`
names the root `Cargo.toml` by an appropriate relative or absolute path.

To leave a generated tree available for review, name a destination that does
not exist or is an empty directory whose parent already exists:

```sh
cargo run --locked --offline --example bootstrap -- stage target/bootstrap-review
diff -ru src target/bootstrap-review/src
```

Use a fresh destination for each staging run; the tool refuses a nonempty
directory. The staged files are the exact raw tangle and have passed the
current Rustfmt check without modification. Review their complete diff before
replacing committed source.

After accepting the staged result, copy it explicitly and verify the complete
canonical/generated pair:

```sh
cp target/bootstrap-review/src/*.rs src/
cargo fmt --all -- --check
cargo run --locked --offline --example bootstrap -- check
cargo test --locked --offline
git diff -- lit src
```

These examples use a POSIX shell; use the equivalent recursive diff and file
copy on another shell. The tool has no update mode, and ordinary Cargo builds
never regenerate source as a side effect.

The root package and this tool require Rust 1.97 or newer, but routine local
development uses the currently installed toolchain.
