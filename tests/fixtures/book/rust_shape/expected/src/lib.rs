// src/lib.rs
// Shared prelude
use std::fmt;
use std::path::Path;

// Local variables
const LOCAL: &str = "library";
pub fn library() -> &'static str { LOCAL }
