use std::path::{Path, PathBuf};

use claude_scriptorium::transcript::Folio;

fn peek(fixture: &str) -> claude_scriptorium::transcript::SessionPeek {
    Folio::peek(Path::new("tests/fixtures").join(fixture).as_path())
}

#[test]
fn latest_ai_title_wins() {
    let peeked = peek("peek_titled.jsonl");

    assert_eq!(
        peeked.title.as_deref(),
        Some("Illuminate the psalter capitals")
    );
}

#[test]
fn cwd_is_recovered_from_the_transcript() {
    let peeked = peek("peek_titled.jsonl");

    assert_eq!(
        peeked.cwd,
        Some(PathBuf::from("/home/scribe/projects/psalter"))
    );
}

#[test]
fn title_falls_back_to_first_real_prompt_when_untitled() {
    let peeked = peek("peek_untitled.jsonl");

    assert_eq!(peeked.title.as_deref(), Some("Rubricate the marginalia."));
}

#[test]
fn malformed_lines_do_not_prevent_recovering_metadata() {
    let peeked = peek("peek_untitled.jsonl");

    assert_eq!(peeked.cwd, Some(PathBuf::from("/srv/vellum")));
}

#[test]
fn a_session_with_no_metadata_peeks_empty() {
    let peeked = Folio::peek(Path::new("tests/fixtures/does-not-exist.jsonl"));

    assert_eq!(
        peeked,
        claude_scriptorium::transcript::SessionPeek::default()
    );
}
