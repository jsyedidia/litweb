# Prism assets used by Litweb

Litweb vendors Prism 1.30.0 and the GitHub-inspired `litweb` light color theme
so woven HTML can syntax-highlight target code without a network connection.

## Prism

- Package: `prismjs` 1.30.0
- Source:
  `https://registry.npmjs.org/prismjs/-/prismjs-1.30.0.tgz`
- npm integrity:
  `sha512-DEvV2ZF2r2/63V+tK8hQvrR2ZGn10srHbXviTlcv7Kpzw8jWiNTqbVgjO3IY8RxrrOUF8VPMQQFysYYYv0YZxw==`
- Archive SHA-512:
  `0c4bd5d99176af6ffadd5fad2bc850beb4766469f5d2cac76d7be24e572fecaa73c3c8d688d4ea6d58233b7218f11c6bace505f153cc410172b18618bf4619c7`
- License: MIT; see `LICENSE-Prism`.

`prism-all.min.js` was assembled without semantic modification from the
package's minified core and all 297 language components in the dependency
order computed by the package's `dependencies.js`. The one Litweb prefix
creates `window.Prism`, sets `Prism.manual`, and thereby prevents Prism's
automatic whole-page pass before Litweb can protect named-fragment markup. Its
SHA-256 is
`50ff3e6c2e1195d450539e0489df568de6e5b93f112863615d067cbb789eef47`.

The eight official Prism files under `themes/` are renamed copies of Prism's
minified themes:

| Litweb name | Prism package file |
| --- | --- |
| `prism-default` | `themes/prism.min.css` |
| `dark` | `themes/prism-dark.min.css` |
| `funky` | `themes/prism-funky.min.css` |
| `okaidia` | `themes/prism-okaidia.min.css` |
| `twilight` | `themes/prism-twilight.min.css` |
| `coy` | `themes/prism-coy.min.css` |
| `solarized-light` | `themes/prism-solarizedlight.min.css` |
| `tomorrow-night` | `themes/prism-tomorrow.min.css` |

The official files include Prism's typography and panel-layout rules. Weaver
embeds their colors and then restores Litweb's code font and panel geometry so
switching a bundled scheme does not resize the explanation.

## The GitHub-inspired Litweb colors

- Package: `@primer/primitives` 11.9.0
- Source:
  `https://registry.npmjs.org/@primer/primitives/-/primitives-11.9.0.tgz`
- npm integrity:
  `sha512-yESOalhd7s7S3unV1V32v3Z0RszXiiz6pzy6hVI9xpdTh1q1Gt8vyDFxRlqIvuwc5ZaO1+gYQTDbjxb4nWBzMw==`
- Archive SHA-512:
  `c8448e6a585deeced2dee9d5d55df6bf767446ccd78a2cfaa73cba85523dc69753875ab51adf2fc83171465a88beec1ce5968ed7e8184130db8f16f89d607333`
- License: MIT; see `LICENSE-Primer`.

`themes/litweb.css`, exposed as the `litweb` scheme, is Litweb-owned CSS that
began by mapping Prism's standard token names to the light `prettylights`
values in `dist/css/functional/themes/light.css`. Litweb uses a muted burgundy
for keywords and distinguishes slate-blue functions from purple type-like
names. It is not copied from the archived `primer/github-syntax-light`
package, and Prism's grammars are not claimed to produce exactly the same
token boundaries as GitHub's Linguist grammars. Litweb uses a `#fafbfc` code
background; comments have a 7.10:1 contrast ratio against it, and every
selected foreground has a ratio of at least 6.01:1.

## Litweb adapter

`litweb-highlight.js` is original Litweb code. It selects an explicitly named
grammar and highlights only the text nodes that were direct children of the
target code element. Existing element subtrees, including `.nocode`
fragment-reference links, never leave the DOM and are not given to Prism. A
fragment therefore divides the surrounding code into independently tokenized
pieces; this protects the link reliably at the acceptable cost of not carrying
a rare string or comment state across that boundary. The adapter reports
browser readiness after every selected element has succeeded or fallen back
safely.
