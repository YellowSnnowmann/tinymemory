//! Tests for source-tag preservation and atomic summary replacement.

use super::{augment_with_source_tag, write_atomically};

#[test]
fn source_front_matter_prepends_scope_tag_and_deduplicates_it() {
    let markdown = b"---\ntree_kind: source\ntree_scope: github/acme/widget\n---\nbody\n";
    let source = crate::store::content::compose::source_tag("github/acme/widget");
    let tags = vec!["entity/person/alice".to_string(), source.clone()];

    assert_eq!(
        augment_with_source_tag(markdown, &tags),
        vec![source, "entity/person/alice".to_string()]
    );
}

#[test]
fn source_tag_is_not_invented_for_invalid_or_non_source_documents() {
    let tags = vec!["entity/topic/rust".to_string()];
    for markdown in [
        b"not front matter".as_slice(),
        b"---\ntree_kind: summary\ntree_scope: scope\n---\nbody".as_slice(),
        b"---\ntree_kind: source\n---\nbody".as_slice(),
        &[0xff, 0xfe],
    ] {
        assert_eq!(augment_with_source_tag(markdown, &tags), tags);
    }
}

#[test]
fn atomic_write_replaces_existing_content_without_leaving_tempfiles() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("summary.md");
    std::fs::write(&path, b"old").expect("seed summary");

    write_atomically(&path, b"new content").expect("atomic replacement");

    assert_eq!(std::fs::read(&path).expect("read summary"), b"new content");
    let entries = std::fs::read_dir(directory.path())
        .expect("read directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("directory entries");
    assert_eq!(entries.len(), 1);
}

#[test]
fn atomic_write_reports_missing_parent_without_leaving_a_file() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("missing").join("summary.md");

    let error = write_atomically(&path, b"content").expect_err("missing parent must fail");

    assert!(error.to_string().contains("create tag tempfile"));
    assert!(!path.exists());
}
