# Litweb User Manual

This manual describes Litweb's command-line interface, `.lit` authoring
format, and woven output. See the repository [README](../README.md) for build
and installation instructions, runnable examples, and contributor commands.

This is a reference manual, not something you need to read before using
Litweb. The `.lit` format is mostly self-explanatory: starting with one of the
checked-in [examples](../examples/README.md) and consulting the relevant
section here when a question arises is often the easiest way to learn it.

Litweb is descended from Zachary Yedidia's Literate and is mostly compatible
with Literate's `.lit` format. The
[Literate manual](https://zyedidia.github.io/literate/manual.html) is therefore
useful additional background. Litweb also has extensions and some deliberate
differences, so this manual and `lw --help` describe Litweb's own supported
interface.

Litweb accepts either one `.lit` chapter or an explicit `.lit` book manifest.
It tangles root code blocks into clean source files and weaves the explanation
into offline HTML or a LuaLaTeX document. HTML is the default: a book becomes
a contents page plus one page per chapter, with local browser assets for
equations and syntax highlighting. LaTeX instead places the whole book in one
document with a table of contents, spread-local mini-indexes, and a final
identifier index.

## Running `lw`

Run `lw` with an input file to tangle and weave it into the current directory,
or choose an output directory explicitly:

```sh
lw --out-dir generated program.lit
lw -t -odir generated program.lit
lw -w -odir generated program.lit
lw -t --no-headers --no-block-separators -odir comparison program.lit
lw -w --no-index program.lit
lw -w --colorscheme okaidia program.lit
lw -w --no-highlight program.lit
lw --format latex program.lit
lw -w --format latex --font-size 11pt program.lit
lw -w --format latex --chapter-opening left book.lit
```

If an input filename begins with `-`, put the conventional `--` marker before
it to end option recognition:

```sh
lw -t -- -draft.lit
```

Use `--tangle` (`-t`) for code only or `--weave` (`-w`) for only the selected
woven format. `--format html` is the default; `--format latex` selects LaTeX.
LaTeX uses 12-point base text by default. Add `--font-size 10pt` or
`--font-size 11pt` to select a smaller standard document-class size; an
explicit `--font-size 12pt` is also accepted. This option requires
`--format latex`.

LaTeX books begin major chapters on right-hand odd pages by default, with a
blank left page before each one. Use `--chapter-opening left` for the
bound-book layout in which a chapter begins on an even page. This option also
requires `--format latex`; standalone chapters retain their ordinary first
page.

All output paths must be safe relative paths beneath the selected directory.
The woven HTML and its generated assets need no network connection or external
Markdown command. Equations and syntax colors use local JavaScript; without
JavaScript, their original readable text remains in the page.

For an exact comparison before and after reorganizing named blocks,
`--no-headers` omits Litweb's generated block-name comments and
`--no-block-separators` omits its generated blank lines between adjacent
expanded block references. By default a header aligns with the first nonblank
authored line in its block, and Litweb adds a separator only when two
references are adjacent and the first expansion did not already end in a
blank line. The options do not remove comments, empty lines, or whitespace
written inside code blocks. They may be used independently, affect only
tangled files, and are harmless in weave-only mode.

Rust documents receive identifier indexes by default. HTML receives the final
index; LaTeX receives both the final index and mini-indexes at the foot of
right-hand pages. Use `--no-index` to suppress all of them. `--colorscheme`
and `--no-highlight` control HTML syntax highlighting and cannot be combined
with `--format latex`; the LaTeX backend uses readable monochrome code without
highlighting.

For a single chapter, the HTML filename is the input filename with its
extension replaced by `.html`. A book preserves the manifest's relative
chapter paths while replacing `.lit` with `.html`, and uses the manifest
filename for its contents page. The prose renderer supports paragraphs, flat
unordered and ordered lists, fenced fixed-width examples, pipe tables, display
equations, asterisk emphasis and strong text, generic backtick code spans,
target-language `@code{...}` spans, links, and `@{Block}` code references.
Input HTML is escaped rather than passed through. Named code blocks link
definitions, additions, redefinitions, and uses by section, including
relative links between book pages.

For LaTeX, a single chapter produces `<name>.tex` and a book produces one
document named from the manifest stem, together with the packaged
`litweb-latex/litweb.sty`. Litweb only generates these inert text files; it
never invokes TeX. Compiling them requires LuaLaTeX, the packages and
Libertinus and DejaVu fonts normally supplied by a full TeX Live installation,
and preferably Latexmk to perform the required repeated passes. The generated
document has internal links, but not HTML navigation bars or syntax colors.

Recognized mathematics is copied as TeX source so LuaLaTeX can typeset it.
Authors targeting both formats should use the useful common subset supported
by KaTeX and LuaLaTeX. Because TeX can execute commands and read local files
even when shell escape is disabled, compile generated LaTeX only when its
`.lit` source is trusted.

## Writing a `.lit` File

A `.lit` file alternates Litweb's documented prose subset with named code
blocks. `@title` names the document, `@s` begins a section, and a line
containing `---` closes a code block:

```text
@code_type rust .rs
@title Hello from Litweb

@s The program

The root block names the file that tangling will write.

--- hello.rs
fn main() {
    @{Print the greeting}
}
---

--- Print the greeting
println!("Hello, world!");
---
```

A block whose final name component has an extension is an output root. Quoting
a name, as in `--- "Makefile"`, makes an extensionless root. Other blocks are
expanded when a whole line contains `@{Block name}`; indentation before that
reference is applied to every expanded line.

Within prose, backticks mark arbitrary literal text such as filenames,
command-line options, and `.lit` notation. Use `@code{value}` when `value` is
code in the target language named by `@code_type`. Braces balance, so
`@code{Point { x: 1 }}` can contain an ordinary Rust, C, or similar expression;
use `\{`, `\}`, or `\\` for a literal brace or backslash that should not affect
that balancing. A target-language span remains on one source line and does not
interpret Markdown or `@{Block}` notation inside it.

An unordered prose item begins with `- ` and an ordered item begins with a
decimal number followed by `. `. Use spaces to indent a wrapped source line
to the item's content column and continue that item:

```text
- This item is long enough to wrap in the source,
  but it remains one item in the woven page.

3. An ordered list may begin at another number.
4. Later source numbers label successive items.
```

Lists are deliberately flat: nested lists and blank-separated paragraphs
inside one item are not yet part of Litweb's prose subset.

Three or more backticks or tildes make a fixed-width prose example rather than
a named or tangled code block. The first word after the opening fence may name
a language for syntax highlighting; `text`, `plain`, and `plaintext` remain
unhighlighted:

````text
```rust
fn example() {
    println!("woven, but not tangled");
}
```
````

Fenced contents are escaped, retain their whitespace, and do not interpret
inline markup, named-block references, equations, or target-language
`@code{...}` spans. A fence must have a matching close; otherwise it remains
ordinary prose and does not consume the rest of the document.

A pipe table begins with a header row followed immediately by a delimiter row
with the same number of cells. Outside pipes are optional. Each delimiter cell
has at least three hyphens; a leading colon selects left alignment, colons on
both sides select center alignment, and a trailing colon selects right
alignment:

| Form | Purpose | Order |
| :--- | :---: | ---: |
| `- ` | unordered item | 1 |
| `N. ` | ordered item | 2 |

Table cells use the ordinary inline forms. Write `\|` for a literal prose
pipe; pipes inside backticks, `@code{...}`, `@{Block}`, inline math, or a
complete link also remain inside their cell. A short body row is padded with
empty trailing cells, and extra cells are ignored. Tables do not contain
nested block-level prose.

These forms are a deliberate Litweb subset, not a promise of general
CommonMark or GitHub Flavored Markdown compatibility. In particular, headings,
nested lists, blank-separated paragraphs inside an item, and raw HTML are not
accepted as structural markup.

`--- Name +=` adds lines to an existing block, while `--- Name :=` explicitly
replaces its lines. Replacement is an exceptional compatibility feature: it
also discards earlier additions, while later additions still apply. It is not
the ordinary way to compose a program; named blocks and additions are clearer
for that purpose, and future change-file support will be a better fit for
maintained alternate editions.

The separated form `--- Name --- noWeave` hides an occurrence from HTML, and
`noHeader` suppresses a block's generated header comment. `@comment_type`
supplies a header pattern such as `// %s`; `@code_type` records the source
language and extension. The language selects HTML syntax highlighting, while
the extension also helps Litweb recognize output filenames and serves as a
fallback language identifier.

`noHeader` is a persistent source choice for one base block. The command-line
`--no-headers` option is instead a temporary override for every block in one
tangle run.

## How Weaving Presents Sections

Litweb keeps the `.lit` source language simple while giving its HTML a
CWEB-inspired reading structure. An explicit `@s` remains a semantic source
section: it supplies an optional title, starts a new command scope, and is the
only kind of section stored by the parser. Weaving derives numbered
presentation sections without changing that model or the tangled program.

Each presentation section contains at most one nonempty prose block followed
by at most one visible code block. A second prose block or a second code block
starts an untitled presentation section automatically. Blank-only prose is
ignored for layout, and a `noWeave` occurrence neither occupies a code slot nor
creates an implicit section. The next explicit `@s` always begins a new
presentation section, even when the preceding one is empty. Numbering restarts
in each chapter; cross-page links display the qualified chapter and section.

The section number, optional bold title, and first ordinary prose paragraph
read as one continuous opening, such as `1. Introduction. This file …`. An
untitled section begins directly with its prose, such as `2. The binary …`.
When code comes first, its named-code headline shares the opening and the code
body begins on the following line. Lists, fenced examples, tables, display
equations, and other block-first prose remain below the section heading rather
than being forced inline. Litweb adds one period after a title unless it
already ends in `.`, `?`, or `!`.

Named-code notation records the relationship between a block occurrence and
its first definition:

| Woven notation | Meaning |
| --- | --- |
| `⟨Name 7⟩ ≡` | first definition |
| `⟨Name 7⟩ +≡` | addition to the definition |
| `⟨Name 7⟩ :=` | explicit replacement of the definition |
| `⟨Name 7⟩` | use of the definition |

Cross-reference notes are complete sentences. For one location, additions say
“See also section 5.”, uses say “This code is used in section 5.”, and
replacements say “This code is replaced in section 5.” Two locations are
joined by “and”; three or more use commas and an Oxford comma. Repeated
references within one presentation section produce one location.

Linked section numbers in named-code notation and relationship notes use link
color without underlines. They remain ordinary semantic links and gain a clear
outline during keyboard focus. Ordinary prose, contents, and book-navigation
links retain their usual underlines.

These rules define Litweb's presentation, not a change to `.lit` syntax or
tangling. Supported source constructs retain their documented meaning, and
presentation grouping cannot change tangled bytes. Litweb's HTML is therefore
deliberately not byte-for-byte compatible with Literate's HTML even when both
accept the same source.

## HTML Syntax Highlighting

Woven target code is highlighted in the browser by the local Prism 1.30.0
bundle. Litweb passes Prism an explicit language from the first field of
`@code_type`; it does not guess from the code. Rust, C, Go, D, Python, and the
many other Prism languages are included offline. Common spellings such as
`C++`, `C#`, `HTML`, `XML`, and `TeX` are mapped to Prism's grammar names. An
unknown grammar leaves the source readable and uncolored.

Highlighting applies to named code-block bodies and fenced prose examples that
supply a language. A target-language `@code{...}` span remains ordinary inline
code: coloring short fragments can interrupt prose and resemble a clickable
link. Its semantic distinction from backticks still matters to identifier
analysis and future output backends. Block titles, `@{Name}` prose references,
relationship notes, identifier-index entries, and `text`, `plain`, or
`plaintext` fences are not highlighted. Inside a named code block, a
whole-line `@{Name}` use remains Litweb's linked angle-bracket notation. The
browser highlights the ordinary text on either side separately, so it never
removes or reconstructs the link. As a result, a rare string or comment that
crosses a fragment boundary may be colored imperfectly; placing the boundary
outside that construct avoids the ambiguity.

The default `litweb` scheme uses a restrained GitHub-inspired light palette
based on Primer Primitives. Select another bundled scheme in a chapter
prologue:

```text
@colorscheme solarized-light
```

The bundled names are `litweb`, `prism-default`, `dark`, `funky`, `okaidia`,
`twilight`, `coy`, `solarized-light`, and `tomorrow-night`; names are
case-insensitive. Litweb retains the same code font size, line spacing, and
panel geometry across these bundled schemes. `@colorscheme none` disables
highlighting. The command is page-wide and must precede the first `@s`. In a
book, a manifest choice is inherited and a chapter prologue may override it.

The built-in theme is available as
[`assets/prism/1.30.0/themes/litweb.css`](../assets/prism/1.30.0/themes/litweb.css)
in a source checkout and as
`litweb-assets/prism-1.30.0/themes/litweb.css` beside generated HTML. To make a
small variation, copy that file next to the relevant `.lit` source, edit its
token colors, and select the copy:

```text
@colorscheme my-litweb.css
```

An argument ending in `.css`, such as
`@colorscheme themes/my-scheme.css`, includes that UTF-8 file as the token
theme. A source path is resolved relative to the `.lit` file that contains the
command. Litweb does not rewrite the CSS, copy resources named by `url()`, or
block its `@import` rules, so a custom theme is trusted presentation input and
may itself request network resources.

`--colorscheme SCHEME` temporarily overrides every source choice; a
command-line `.css` path is relative to the current working directory.
`--no-highlight` is the global disabled form. Pages that have no target code,
or whose effective scheme is `none`, emit no Prism bundle. Otherwise a weave
writes one shared `litweb-assets/prism-1.30.0/` directory containing the local
all-language bundle, adapter, themes, licenses, and provenance record.
Source-level `@colorscheme` commands are harmless HTML metadata when LaTeX is
selected and have no effect on that document. The two command-line
highlighting options are instead rejected with `--format latex` so a requested
choice is not silently ignored. `@add_css` and `@overwrite_css`, which would
alter the complete page style rather than just token colors, remain
unsupported.

## Rust Identifier Indexes

When the effective `@code_type` begins with the exact language name `rust`,
Litweb reads visible code blocks and explicit `@code{...}` spans to build an
alphabetical identifier index. The index is appended after a single woven
chapter. Each identifier links to the numbered presentation sections where it
occurs; an underlined section number marks a definition. Repeated occurrences
in one presentation section produce one link, with a definition taking
precedence over a use.

A book receives one `index-identifiers.html` page beside its contents page.
Each spelling is divided into chapter groups in book order. A group links its
chapter number and label to the chapter page, then links chapter-local section
numbers to the occurrences. Contents and chapter navigation link to the
identifier index. If no Rust entries are retained, Litweb emits neither an
empty page nor index-navigation links.

The analyzer understands Rust tokens and common declaration and binding forms,
but it works on literate fragments rather than compiling a complete crate.
When declaration syntax spans named blocks, it uses the resolved whole-line
references as context while keeping every index link at the visible source
block and line. A reused source token still produces one occurrence;
definition evidence takes precedence over use evidence. Comments, literals,
keywords, and primitive types are not identifiers. A one-character name is
listed only where it is defined. Type occurrences are listed only when the
chapter declares that type, which keeps imported and external type names from
overwhelming the index. An `@code{...}` span is a prose use, not a definition.
The same spelling inside a fenced prose example is literal and does not enter
the identifier index. Whole-line `@{Block}` references remain part of Litweb's
separate named-block cross-reference system.

LaTeX also places a small three-column mini-index below the body of each
right-hand page. It lists meanings used on that two-page spread whose
definitions are elsewhere, gives each reliable declaration kind, and links to
the defining presentation section. Repeated uses make one entry, while a
definition anywhere on the spread suppresses that entry. Ambiguous and
unresolved names are omitted rather than guessed; this is a reading aid based
on Litweb's conservative analysis, not full Rust compiler name resolution.

Every major (unindented) literate manifest entry finishes through a right-hand
page. By default, a blank left page follows and the next major chapter begins
on the right-hand odd page after it. This loose-sheet layout keeps two major
chapters off the same physical sheet. `--chapter-opening left` instead begins
each major chapter on the even page immediately after the preceding chapter's
completed right page, keeping its first mini-index spread together for a bound
book. A minor (indented) manifest entry remains a section of its major chapter
and does not start a new spread.

Chapter opening is independent of identifier indexing: `--no-index` removes
the mini-indexes, final index, and marker machinery without changing the
selected opening sides. Front matter, the table of contents, and the final
identifier index keep their ordinary roles. A standalone chapter does not
acquire an opening blank page: its first page is a one-page right-hand spread,
followed by normal even/odd pairs.

Other `@code_type` languages currently weave normally without contributing to
either identifier index. Analyzers for additional languages are planned
separately.

## Equations

Single dollar signs place a TeX-style equation inside prose:

```text
The area of a circle is $A = \pi r^2$.
```

Double dollar signs make a centered display equation. A short display may
occupy one line, while a longer body may use separate delimiter lines:

```text
$$
\sum_{k=1}^{n} k = \frac{n(n+1)}{2}
$$
```

Only indentation may precede a display's opening `$$`, and only whitespace
may follow its closing `$$`. Inline equations stay on one source line. Write
`\$` for a literal dollar outside math. Dollar signs inside backticks,
`@code{...}`, `@{Name}` references, and code blocks remain literal; an
unmatched delimiter also remains readable text.

Woven HTML renders equations with KaTeX 0.18.1. When a document uses math,
`lw` writes a versioned `litweb-assets/katex-0.18.1/` directory containing the
local JavaScript, stylesheet, WOFF2 fonts, license, and provenance notice.
Math-free documents emit no such files. Generated equations therefore need
JavaScript but never need a network connection. Invalid TeX remains visible
with a browser-time error message instead of making weaving fail. Litweb does
not currently provide shared custom equation macros.

## Writing a Book

A book manifest is a `.lit` file with a standalone `@book` marker, one
nonempty `@title`, introductory prose, and an ordered list of chapter links:

```text
@book
@title A Small Rust Book
@code_type rust .rs
@comment_type // %s

This prose appears on the contents page.

[Introduction](chapters/introduction.lit)
    [A minor chapter](chapters/details.lit)
[The application](application.lit)
```

An unindented entry begins a major chapter. One or more leading spaces or tabs
make a minor chapter under the preceding major chapter; deeper nesting is not
currently defined. Each entry must occupy a complete line and name a safe,
portable, manifest-relative `.lit` path. Absolute paths, `.` and `..`
components, backslashes, drive prefixes, repeated paths, and paths that would
collide with the contents page are rejected before chapters are read.

The three title roles are independent. The manifest's `@title` names the book,
the link label appears in contents and Previous/Next navigation, and a
chapter's own `@title` names its page. A chapter without a nonempty title uses
its manifest label as the page title.

The safe manifest commands `@code_type` and `@comment_type` provide defaults
for all chapters. Repeated values use the last declaration, `none` clears a
value, and chapter or section declarations can override the defaults. A
manifest `@colorscheme` similarly supplies an HTML page-wide default, but may
be overridden only in a chapter prologue before its first `@s`. Book loading
does not execute commands from the manifest or its chapters.

Ordinary named blocks are local to a chapter, so natural names such as
`Imports` or `Local variables` can repeat throughout a long book. A reference,
addition, or redefinition first selects the definition in its own chapter. If
there is no local definition, one uniquely named definition elsewhere in the
book is selected; several candidates are an ordered ambiguity error. Root
output paths remain global to the book, and a chapter may define any number of
nested roots such as library modules, binaries, examples, and tests.

`lw` recognizes a book only through the exact standalone `@book` marker. It
does not discover chapters from a directory. Includes, change files, compiler
commands, whole-page custom styling commands, and external Markdown commands
are not implemented and produce an explicit diagnostic. Raw HTML is escaped.
