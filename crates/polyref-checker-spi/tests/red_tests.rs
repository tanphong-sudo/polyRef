//! TDD red-state checklist for `polyref-checker-spi`.
//!
//! Mirrors `claude/05-handoff-1-core-ir.md` §E-1 SPI tests.

use polyref_checker_spi::limits::SafePath;

#[test]
#[ignore = "§E-1: implement SafePath::parse"]
fn safe_path_rejects_absolute() {
    assert!(SafePath::parse("/etc/passwd").is_err());
}

#[test]
#[ignore = "§E-1: implement SafePath::parse"]
fn safe_path_rejects_parent_traversal() {
    assert!(SafePath::parse("foo/../../bar").is_err());
}

#[test]
#[ignore = "§E-1: implement SafePath::parse"]
fn safe_path_rejects_empty() {
    assert!(SafePath::parse("").is_err());
}

#[test]
#[ignore = "§E-1: implement SafePath::parse"]
fn safe_path_rejects_nul() {
    assert!(SafePath::parse("foo\u{0000}bar").is_err());
}

#[test]
#[ignore = "§E-1: implement SafePath::parse"]
fn safe_path_accepts_canonical_relative_path() {
    assert!(SafePath::parse("repo/src/users.ts").is_ok());
}
