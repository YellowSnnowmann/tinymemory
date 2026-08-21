//! Tests for the surrounding module.

use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn scans_codex_and_claude_sessions_and_filters_machine_content() {
    let temp = tempdir().unwrap();
    let claude = temp.path().join("claude");
    let codex = temp.path().join("codex/2026/07/14");
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        claude.join("session.jsonl"),
        concat!(
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"machine\"}]}}\n",
            "{\"type\":\"user\",\"sessionId\":\"c1\",\"cwd\":\"/repo\",\"timestamp\":\"2026-07-14T00:00:00Z\",\"message\":{\"content\":\"Prefer small modules\"}}\n"
        ),
    )
    .unwrap();
    fs::write(
        codex.join("rollout-test.jsonl"),
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"x1\",\"cwd\":\"/repo\"}}\n",
            "{\"type\":\"response_item\",\"timestamp\":\"2026-07-14T00:00:00Z\",\"payload\":{\"type\":\"message\",\"role\":\"developer\",\"content\":[{\"type\":\"input_text\",\"text\":\"secret scaffolding\"}]}}\n",
            "{\"type\":\"response_item\",\"timestamp\":\"2026-07-14T00:00:01Z\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Run focused tests first\"}]}}\n"
        ),
    )
    .unwrap();

    let statuses = coding_session_status_for_roots(&claude, &temp.path().join("codex"));
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].session_files, 1);
    assert_eq!(statuses[0].evidence_units, 1);
    assert_eq!(statuses[1].session_files, 1);
    assert_eq!(statuses[1].evidence_units, 1);
    assert_eq!(statuses[0].invalid_files + statuses[1].invalid_files, 0);
}

#[test]
fn status_scan_stops_parsing_at_the_configured_limit() {
    let paths = [PathBuf::from("one"), PathBuf::from("two")];
    let reads = std::cell::Cell::new(0);
    let status = source_status(
        "fixture",
        Path::new("."),
        1,
        |_, max_files| (paths[..max_files].to_vec(), paths.len() > max_files),
        |_| {
            reads.set(reads.get() + 1);
            Ok(RawSession::new(
                tinycortex::memory::persona::types::EvidenceSource::new(
                    tinycortex::memory::persona::types::PersonaSourceKind::Codex,
                ),
            ))
        },
    );

    assert_eq!(reads.get(), 1);
    assert_eq!(status.session_files, 1);
    assert!(status.scan_truncated);
}

#[test]
fn bounded_discovery_stops_after_finding_one_extra_candidate_without_ordering() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("a.jsonl"), "").unwrap();
    fs::write(temp.path().join("b.jsonl"), "").unwrap();
    fs::write(temp.path().join("ignored.txt"), "").unwrap();

    let (files, truncated) = discover_claude_sessions(temp.path(), 1);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension().unwrap(), "jsonl");
    assert!(truncated);
}

#[test]
fn status_scan_skips_oversized_sessions_without_parsing_them() {
    let temp = tempdir().unwrap();
    let oversized = temp.path().join("oversized.jsonl");
    let small = temp.path().join("small.jsonl");
    let file = fs::File::create(&oversized).unwrap();
    file.set_len(MAX_STATUS_SESSION_FILE_BYTES + 1).unwrap();
    fs::write(&small, "{}\n").unwrap();
    let reads = std::cell::Cell::new(0);

    let status = source_status(
        "fixture",
        temp.path(),
        2,
        |_, _| (vec![oversized.clone(), small.clone()], false),
        |_| {
            reads.set(reads.get() + 1);
            Ok(RawSession::new(
                tinycortex::memory::persona::types::EvidenceSource::new(
                    tinycortex::memory::persona::types::PersonaSourceKind::Codex,
                ),
            ))
        },
    );

    assert_eq!(reads.get(), 1);
    assert_eq!(status.session_files, 2);
    assert_eq!(status.invalid_files, 0);
    assert!(status.scan_truncated);
}

#[test]
fn status_scan_enforces_the_aggregate_byte_budget() {
    let temp = tempdir().unwrap();
    let paths = (0..5)
        .map(|index| {
            let path = temp.path().join(format!("session-{index}.jsonl"));
            let file = fs::File::create(&path).unwrap();
            file.set_len(MAX_STATUS_SESSION_FILE_BYTES).unwrap();
            path
        })
        .collect::<Vec<_>>();
    let reads = std::cell::Cell::new(0);

    let status = source_status(
        "fixture",
        temp.path(),
        paths.len(),
        |_, _| (paths.clone(), false),
        |_| {
            reads.set(reads.get() + 1);
            Ok(RawSession::new(
                tinycortex::memory::persona::types::EvidenceSource::new(
                    tinycortex::memory::persona::types::PersonaSourceKind::Codex,
                ),
            ))
        },
    );

    assert_eq!(reads.get(), 4);
    assert_eq!(status.session_files, 5);
    assert!(status.scan_truncated);
}
