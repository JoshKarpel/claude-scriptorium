use std::path::Path;

use claude_scriptorium::{
    render::{Colophon, Scribe},
    transcript::{Content, Folio, Role},
};
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use jiff::{Timestamp, tz::TimeZone};

fn fixture() -> Folio {
    Folio::read(Path::new("tests/fixtures/session.jsonl")).expect("fixture parses")
}

fn highlighter() -> SyntectAdapter {
    SyntectAdapterBuilder::new()
        .css_with_class_prefix("ink-")
        .build()
}

fn render(folio: &Folio, highlighter: &SyntectAdapter) -> String {
    let scribe = Scribe::new(highlighter, TimeZone::UTC);
    let colophon = Colophon {
        generated: "2026-03-12T09:15:00Z".parse::<Timestamp>().unwrap(),
        tool: "claude-scriptorium",
        version: "0.1.0",
    };
    scribe.folio(folio, &colophon).into_string()
}

#[test]
fn bookkeeping_lines_are_not_turns() {
    let folio = fixture();

    assert_eq!(folio.turns.len(), 5);
}

#[test]
fn turn_roles_come_from_the_entry_tag() {
    let folio = fixture();

    let roles: Vec<Role> = folio.turns.iter().map(|turn| turn.role).collect();
    assert_eq!(
        roles,
        [
            Role::User,
            Role::Assistant,
            Role::User,
            Role::User,
            Role::Assistant
        ]
    );
}

#[test]
fn string_content_is_kept_whole() {
    let folio = fixture();

    let Content::Text(text) = &folio.turns[0].content else {
        panic!("expected the opening turn to carry plain string content");
    };
    assert_eq!(text, "Explain the **quire** layout, please.");
}

#[test]
fn subagent_turns_are_marked() {
    let folio = fixture();

    let sidechains: Vec<bool> = folio.turns.iter().map(|turn| turn.is_sidechain).collect();
    assert_eq!(sidechains, [false, false, false, true, false]);
}

#[test]
fn session_id_comes_from_the_file_name() {
    assert_eq!(fixture().session_id(), "session");
}

#[test]
fn the_effort_level_refines_the_model_name() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(
        r#"<span class="turn__model">claude-opus-4-8 <span class="turn__effort">(high)</span></span>"#
    ));
    // The turn the harness recorded no effort for names the model alone.
    assert!(html.contains(r#"<span class="turn__model">claude-opus-4-8</span>"#));
}

fn metered() -> Folio {
    Folio::read(Path::new("tests/fixtures/usage.jsonl")).expect("fixture parses")
}

#[test]
fn a_response_written_as_several_lines_is_counted_once() {
    let folio = metered();

    // Both lines of msg_quire report the response's usage; only the first
    // keeps it, so the second contributes nothing to the total.
    let counted: Vec<u64> = folio
        .turns
        .iter()
        .filter_map(|turn| turn.usage)
        .map(|usage| usage.output_tokens)
        .collect();
    assert_eq!(counted, [214, 31]);
}

#[test]
fn a_session_totals_its_output_but_takes_its_largest_input() {
    let folio = metered();

    assert_eq!(folio.output(), Some(245));
    assert_eq!(folio.largest_input(), Some(48_207));
}

#[test]
fn a_session_without_usage_reports_none() {
    assert!(fixture().output().is_none());
    assert!(fixture().largest_input().is_none());
}

#[test]
fn a_turn_shows_its_own_flux_and_the_folio_the_session_s() {
    let html = render(&metered(), &highlighter());

    // The turn that opens the response counts only the input it added...
    assert!(html.contains(
        r#"<span class="turn__usage" title="32,403 input this turn · 214 output this turn">↑ 32.4k ↓ 214</span>"#
    ));
    // ...while the plaque takes the largest single input the session saw.
    assert!(html.contains(
        r#"<dd title="48,207 input at its largest · 245 output in all">↑ 48.2k ↓ 245</dd>"#
    ));
}

#[test]
fn unrecognized_block_types_render_as_json_instead_of_failing() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("block--unknown"));
    assert!(html.contains("<summary>illumination</summary>"));
    assert!(html.contains("lapis lazuli"));
}

#[test]
fn unrecognized_blocks_nested_in_tool_results_survive_too() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("<summary>tool_reference</summary>"));
    assert!(html.contains("Scriptorium"));
}

#[test]
fn markdown_becomes_html() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("<strong>quire</strong>"));
}

#[test]
fn fenced_code_is_highlighted_into_classed_spans() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"class="ink-source ink-rust""#));
}

#[test]
fn tool_calls_carry_a_gist_of_their_subject() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"<span class="marginalia__tool">Read</span>"#));
    assert!(html.contains("/scriptorium/quire.rs"));
}

fn tools() -> Folio {
    Folio::read(Path::new("tests/fixtures/tools.jsonl")).expect("fixture parses")
}

#[test]
fn bash_calls_show_their_description_and_highlighted_command() {
    let html = render(&tools(), &highlighter());

    // The description labels the fold; the command fills its body.
    assert!(html.contains(r#"<span class="marginalia__gist">Scaffold the crate</span>"#));
    // The command is a highlighted shell block, not raw JSON.
    assert!(html.contains("ink-shell"));
    assert!(html.contains("cargo"));
}

#[test]
fn write_calls_highlight_content_by_file_extension() {
    let html = render(&tools(), &highlighter());

    // main.rs resolves to the Rust lexer via the bare extension.
    assert!(html.contains("ink-rust"));
    assert!(html.contains("scriptorium"));
}

#[test]
fn edit_calls_render_before_and_after_as_a_diff() {
    let html = render(&tools(), &highlighter());

    assert!(html.contains(r#"<span class="marginalia__note">replace all</span>"#));
    // The old line is a deletion and the new line an insertion in the diff.
    assert!(html.contains("ink-deleted"));
    assert!(html.contains("ink-inserted"));
}

#[test]
fn todo_writes_render_as_a_status_marked_checklist() {
    let html = render(&tools(), &highlighter());

    assert!(
        html.contains(r#"<li class="tool__todo" data-status="completed">Scaffold the crate</li>"#)
    );
    assert!(html.contains(r#"data-status="in_progress">Wire the CLI</li>"#));
    assert!(html.contains(r#"data-status="pending">Add coverage</li>"#));
}

#[test]
fn failed_tool_results_are_flagged() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("data-error"));
    assert!(html.contains("vellum not found"));
}

#[test]
fn images_are_inlined_as_data_urls() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"src="data:image/png;base64,iVBORw0KGgo=""#));
}

#[test]
fn transcript_html_is_escaped_rather_than_executed() {
    let folio = Folio::read(Path::new("tests/fixtures/injection.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // The folio carries its own trusted app script, so "no <script> at all" is
    // no longer the invariant. What must hold is that transcript-provided
    // markup is inert: every <script> the session mentions is escaped to text,
    // and none survives as an executable tag.
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>alert"));
}

#[test]
fn the_folio_carries_its_trusted_app_script() {
    let html = render(&fixture(), &highlighter());

    // The app script is inlined in <head> (theme applies before first paint).
    assert!(html.contains("<script>"));
    assert!(html.contains("scriptorium-theme"));
}

#[test]
fn the_folio_carries_a_search_widget() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"class="search""#));
    assert!(html.contains(r#"class="search__input""#));
    assert!(html.contains(r#"data-search-nav="next""#));
}

#[test]
fn the_header_offers_a_light_dark_system_theme_toggle() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"class="theme-toggle""#));
    assert!(html.contains(r#"data-theme-choice="light""#));
    assert!(html.contains(r#"data-theme-choice="system""#));
    assert!(html.contains(r#"data-theme-choice="dark""#));
}

#[test]
fn a_turns_role_is_one_class_attribute_not_two() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"class="turn turn--assistant""#));
    assert!(!html.contains(r#"class="turn" class="#));
}

#[test]
fn thinking_blocks_render_their_markdown() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"<section class="block block--thinking">"#));
    assert!(html.contains("The gathering is four folded sheets."));
}

#[test]
fn redacted_thinking_blocks_are_marked_not_shown_as_an_empty_box() {
    let folio =
        Folio::read(Path::new("tests/fixtures/redacted_thinking.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    assert!(!html.contains(r#"<section class="block block--thinking">"#));
    assert!(html.contains("block--redacted"));
    assert!(html.contains("The reasoning was redacted."));
}

#[test]
fn tool_result_turns_fold_into_the_assistant_panel() {
    let html = render(&fixture(), &highlighter());

    // The opening user turn, the assistant panel that absorbs the tool
    // results, and the closing assistant panel: three articles, one "user".
    assert_eq!(html.matches("<article").count(), 3);
    assert_eq!(html.matches(r#"turn__role">user"#).count(), 1);
    assert!(html.contains("pub struct Quire"));
}

#[test]
fn panels_are_labelled_by_their_content_kind() {
    let folio = Folio::read(Path::new("tests/fixtures/kinds.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // A tool call plus its folded result reads as tool; bare reasoning as
    // thinking; prose keeps the speaker's name.
    assert!(html.contains(r#"turn__role">tool<"#));
    assert!(html.contains(r#"turn__role">thinking<"#));
    assert!(html.contains(r#"turn__role">assistant<"#));
}

#[test]
fn only_speech_panels_open_with_a_versal() {
    let folio = Folio::read(Path::new("tests/fixtures/kinds.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // Three panels: a tool exchange, bare reasoning, and one line of prose.
    // Only the prose panel earns a versal; tool and thinking panels get none.
    // (Match the emitted attribute, not the `[data-versal]` selectors the
    // inlined stylesheet also carries.)
    assert_eq!(html.matches("data-versal>").count(), 1);
    assert!(html.contains(r#"<div class="block block--text" data-versal>"#));
}

#[test]
fn a_versal_marks_only_the_opening_paragraph_of_a_panel() {
    // The assistant panel opens with a thinking block, then prose, then a tool
    // call and its folded result: the versal lands on the leading text and
    // nowhere else, so the session's two speaking panels carry exactly two.
    let html = render(&fixture(), &highlighter());

    assert_eq!(html.matches("data-versal>").count(), 2);
}

#[test]
fn each_panel_is_numbered_by_its_leading_turn() {
    let html = render(&fixture(), &highlighter());

    // Turns 1, 2, and 5 lead panels; turns 3 and 4 fold into turn 2's panel,
    // so their numbers never appear as labels.
    assert!(html.contains(r##"href="#turn-1">#1</a>"##));
    assert!(html.contains(r##"href="#turn-2">#2</a>"##));
    assert!(html.contains(r##"href="#turn-5">#5</a>"##));
    assert!(!html.contains(r##"href="#turn-3">#3</a>"##));
}

#[test]
fn each_panel_is_a_deep_link_target_its_number_points_to() {
    let html = render(&fixture(), &highlighter());

    // The panel carries an id its own number links to, so #turn-N in the URL
    // lands on the panel and the number is a shareable permalink to it.
    assert!(html.contains(r#"<article id="turn-1""#));
    assert!(html.contains(r#"<article id="turn-2""#));
    assert!(html.contains(r##"<a class="turn__index" href="#turn-1">#1</a>"##));
}

#[test]
fn meta_turns_are_marked_and_hidden_with_no_reveal_control() {
    let folio = Folio::read(Path::new("tests/fixtures/meta.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // Harness notes stay in the folio, marked and hidden by the stylesheet, but
    // carry no reader-facing control to reveal them.
    assert!(html.contains("data-meta"));
    assert!(html.contains("Base directory for this skill"));
    assert!(!html.contains(r#"id="show-meta""#));
}

#[test]
fn clear_command_turns_are_dropped() {
    let folio = Folio::read(Path::new("tests/fixtures/clear.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    assert!(!html.contains("command-name"));
    assert!(!html.contains("command-message"));
    assert!(html.contains("Explain the quire layout."));
    assert_eq!(html.matches("<article").count(), 1);
}

#[test]
fn a_message_queued_mid_response_becomes_a_user_turn() {
    let folio = Folio::read(Path::new("tests/fixtures/queued.jsonl")).expect("fixture parses");

    // The prompt the user typed while the assistant was working is recorded as
    // a `queued_command` attachment, not a `user` line, so it would vanish if
    // attachments were all dropped as bookkeeping.
    let queued = folio
        .turns
        .iter()
        .find(|turn| {
            turn.role == Role::User
                && matches!(&turn.content, Content::Text(text) if text.contains("above and below"))
        })
        .expect("the queued message is lifted into a user turn");
    assert!(!queued.is_meta);
    assert!(!queued.is_sidechain);
}

#[test]
fn a_queued_message_renders_as_its_own_user_panel() {
    let folio = Folio::read(Path::new("tests/fixtures/queued.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    assert!(html.contains("put the drolleries above and below the vine"));
    // Opening prompt and the mid-response interjection are two user panels; the
    // tool result folds into the assistant that called it rather than standing
    // between them.
    assert_eq!(html.matches(r#"turn__role">user"#).count(), 2);
}

#[test]
fn non_queued_attachments_stay_dropped() {
    let folio = Folio::read(Path::new("tests/fixtures/queued.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // A `task_reminder` attachment is scaffolding, not conversation: it must not
    // leak into the render the way the queued command does.
    assert!(!html.contains("You have not used the task tools"));
}

#[test]
fn search_offers_per_kind_scope_toggles() {
    let html = render(&fixture(), &highlighter());

    // The reader can restrict the search to chosen kinds of message; every
    // scope starts enabled.
    assert!(html.contains(r#"data-scope="user" aria-pressed="true""#));
    assert!(html.contains(r#"data-scope="assistant" aria-pressed="true""#));
    assert!(html.contains(r#"data-scope="tool" aria-pressed="true""#));
    assert!(html.contains(r#"data-scope="thinking" aria-pressed="true""#));
}

#[test]
fn panels_carry_their_turn_number_for_stable_fold_memory() {
    let html = render(&fixture(), &highlighter());

    // The per-message fold memory keys on this: a panel's turn number is stable
    // across live re-renders because the raw stream only ever appends.
    assert!(html.contains(r#"data-turn="1""#));
    assert!(html.contains(r#"data-turn="2""#));
}

#[test]
fn the_dock_offers_role_scoped_message_navigation() {
    let html = render(&fixture(), &highlighter());

    // The middle column steps between all messages; the flanking columns seek
    // one speaker, tagged so the app script and stylesheet can scope them.
    assert!(html.contains(r#"data-nav="prev" data-role="user""#));
    assert!(html.contains(r#"data-nav="next" data-role="assistant""#));
    assert!(html.contains(r#"class="dock__btn dock__btn--user""#));
    assert!(html.contains(r#"class="dock__btn dock__btn--assistant""#));
}

#[test]
fn the_stylesheet_is_inlined_not_linked() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("<style>"));
    assert!(html.contains(".ink-keyword"));
    assert!(!html.contains("<link"));
}

#[test]
fn fonts_are_embedded_not_linked() {
    let html = render(&fixture(), &highlighter());

    // Every face is inlined as a woff2 data URI, so the folio stays
    // self-contained: Junicode (roman + italic), UnifrakturCook, Fira Code.
    assert_eq!(html.matches("data:font/woff2;base64,").count(), 4);
    assert!(html.contains(r#"font-family:"Junicode""#));
    assert!(html.contains(r#"font-family:"UnifrakturCook""#));
    assert!(html.contains(r#"font-family:"Fira Code""#));
    assert!(!html.contains("fonts.googleapis.com"));
    assert!(!html.contains("fonts.gstatic.com"));
}

#[test]
fn embedded_fonts_carry_their_open_font_license_notice() {
    let html = render(&fixture(), &highlighter());

    // The SIL OFL requires every copy to carry the copyright and license; the
    // notice comment and colophon credit put both in every generated folio.
    assert!(html.contains("Copyright 2010 j. 'mach' wust"));
    assert!(html.contains("SIL Open Font License"));
}

#[test]
fn the_colophon_stamps_the_run() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("claude-scriptorium"));
    assert!(html.contains("2026-03-12 09:15:00 UTC"));
}
