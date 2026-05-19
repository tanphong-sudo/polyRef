//! Integration tests for `polyref-checker-spi`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use polyref_checker_spi::limits::SafePath;

#[test]
fn safe_path_rejects_absolute() {
    assert!(SafePath::parse("/etc/passwd").is_err());
}

#[test]
fn safe_path_rejects_parent_traversal() {
    assert!(SafePath::parse("foo/../../bar").is_err());
}

#[test]
fn safe_path_rejects_empty() {
    assert!(SafePath::parse("").is_err());
}

#[test]
fn safe_path_rejects_nul() {
    assert!(SafePath::parse("foo\u{0000}bar").is_err());
}

#[test]
fn safe_path_rejects_bidi_override() {
    assert!(SafePath::parse("foo/\u{202E}bar").is_err());
}

#[test]
fn safe_path_rejects_zero_width() {
    assert!(SafePath::parse("foo/\u{200B}bar").is_err());
}

#[test]
fn safe_path_accepts_canonical_relative_path() {
    let p = SafePath::parse("repo/src/users.ts").expect("should parse");
    assert_eq!(p.as_str(), "repo/src/users.ts");
}

#[test]
fn safe_path_accepts_dotfile() {
    assert!(SafePath::parse(".config/settings.json").is_ok());
}

#[test]
fn safe_path_rejects_oversize() {
    let long = "a/".repeat(2100); // > 4 KiB
    assert!(SafePath::parse(&long).is_err());
}
