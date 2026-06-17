//! Shared helpers for the conformance tests: fixture discovery and a
//! semantic JSON diff.
//!
//! The diff compares two parsed `serde_json::Value` trees rather than strings,
//! so key ordering and whitespace don't matter. Two rules make it match the
//! commonmeta wire format precisely:
//!
//!   * **omitempty-aware**: a field present in one tree but absent in the other
//!     is only a mismatch if its value is non-empty. An empty string, empty
//!     array, empty object, null, or numeric zero is treated as equivalent to
//!     absent and our `skip_serializing_if`.
//!   * **numeric-aware**: `52` and `52.0` compare equal, so a fixture authored
//!     with an integer latitude won't spuriously fail against our `f64` field.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug)]
pub enum Mismatch {
    Lost(String),
    Spurious(String),
    Changed {
        path: String,
        expected: String,
        actual: String,
    },
    LengthChanged {
        path: String,
        expected: usize,
        actual: usize,
    },
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mismatch::Lost(p) => write!(f, "LOST     {p} (in expected, not in output)"),
            Mismatch::Spurious(p) => write!(f, "SPURIOUS {p} (in output, not in expected)"),
            Mismatch::Changed { path, expected, actual } => {
                write!(f, "CHANGED  {path}: expected {expected}, got {actual}")
            }
            Mismatch::LengthChanged { path, expected, actual } => {
                write!(f, "LENGTH   {path}: expected {expected} items, got {actual}")
            }
        }
    }
}

pub fn is_emptyish(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::Number(n) => n.as_f64().map(|f| f == 0.0).unwrap_or(false),
        Value::Bool(_) => false,
    }
}

fn num_eq(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

pub fn diff(expected: &Value, actual: &Value) -> Vec<Mismatch> {
    let mut out = Vec::new();
    diff_rec(expected, actual, "$", &mut out);
    out
}

fn join(path: &str, key: &str) -> String {
    format!("{path}.{key}")
}
fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

fn diff_rec(exp: &Value, act: &Value, path: &str, out: &mut Vec<Mismatch>) {
    match (exp, act) {
        (Value::Object(e), Value::Object(a)) => {
            for (k, ev) in e {
                match a.get(k) {
                    Some(av) => diff_rec(ev, av, &join(path, k), out),
                    None if !is_emptyish(ev) => out.push(Mismatch::Lost(join(path, k))),
                    None => {}
                }
            }
            for (k, av) in a {
                if !e.contains_key(k) && !is_emptyish(av) {
                    out.push(Mismatch::Spurious(join(path, k)));
                }
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                out.push(Mismatch::LengthChanged {
                    path: path.to_string(),
                    expected: e.len(),
                    actual: a.len(),
                });
            }
            for i in 0..e.len().min(a.len()) {
                diff_rec(&e[i], &a[i], &index(path, i), out);
            }
        }
        (Value::Number(_), Value::Number(_)) => {
            if !num_eq(exp, act) {
                out.push(Mismatch::Changed {
                    path: path.to_string(),
                    expected: exp.to_string(),
                    actual: act.to_string(),
                });
            }
        }
        _ => {
            if exp != act && !(is_emptyish(exp) && is_emptyish(act)) {
                out.push(Mismatch::Changed {
                    path: path.to_string(),
                    expected: exp.to_string(),
                    actual: act.to_string(),
                });
            }
        }
    }
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

pub fn collect_json(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

pub fn collect_bib(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("bib") {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}
