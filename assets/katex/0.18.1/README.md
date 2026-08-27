# KaTeX 0.18.1 assets

Litweb redistributes selected production assets from KaTeX 0.18.1:

- `katex.min.js`;
- `katex.min.css`; and
- the twenty WOFF2 files under `fonts/`.

The source package was downloaded from
<https://registry.npmjs.org/katex/-/katex-0.18.1.tgz>. Its npm integrity value
is
`sha512-Td8GCYSxDAoMhHOlKmCFMJ/hz5qlAAb71n66Dryw9nfCVfumLo7nhuotbvKom/XPADmrYC3O5QR71EPq4DarJQ==`,
and its SHA-256 digest is
`7e6100b7fe6439ba91d918d8cb2873171a9fdec979281d508959cf5f7dba1da8`.
The package records upstream Git commit
`cdf479f6749fd2a04c2ccad9be0ead2ac26c33d2`.

The JavaScript and fonts are unmodified. Litweb's copy of `katex.min.css`
retains only each `woff2` font source, removing the redundant `woff` and
`truetype` fallback URLs because those font files are not redistributed.
No style rules were otherwise changed.

KaTeX is Copyright (c) 2013-2020 Khan Academy and other contributors and is
available under the MIT license reproduced in `LICENSE`.
