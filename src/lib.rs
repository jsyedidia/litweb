// src/lib.rs
//! A modern literate programming system.
//!
//! The library parses, tangles, and weaves single-file `.lit` input and
//! explicit book manifests containing ordered chapters. Books resolve and
//! tangle as one program and weave into navigable multi-page HTML or one
//! LuaLaTeX document.
//! Production implementation is maintained in canonical `.lit` files under
//! `lit/`; committed modules under `src/` are their generated, formatted form.
//! Ordinary Cargo builds compile those committed modules and never regenerate
//! them implicitly.

pub mod book;
pub mod config;
pub mod identifier;
mod inline;
pub mod latex;
pub mod output;
pub mod parser;
mod prose;
pub mod resolver;
pub mod tangler;
pub mod util;
pub mod weaver;
mod woven;
