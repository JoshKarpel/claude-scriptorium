use std::{collections::BTreeMap, path::Path, time::Duration};

use claude_scriptorium::{
    render,
    render::{Colophon, Delivery, Fonts, Labour, Scribe},
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
    set(folio, highlighter, Fonts::Fitted, Delivery::Static).0
}

/// Sets a folio, handing back both the markup and the characters that drove it
/// onto the whole faces.
fn set(
    folio: &Folio,
    highlighter: &SyntectAdapter,
    fonts: Fonts,
    delivery: Delivery,
) -> (String, BTreeMap<char, usize>) {
    let scribe = Scribe::new(highlighter, TimeZone::UTC, fonts, delivery);
    let colophon = Colophon {
        generated: "2026-03-12T09:15:00Z".parse::<Timestamp>().unwrap(),
        tool: "claude-scriptorium",
        version: "0.1.0",
        home: "https://example.invalid/scriptorium",
    };
    // A folio states what its own render cost, which is only known once the
    // markup exists; fixed values stand in for a real run's here.
    let labour = Labour {
        took: Duration::from_millis(412),
        bytes: 2_947_312,
    };
    let (markup, reached) = scribe.folio(folio, &colophon);
    (render::inscribe(markup.into_string(), &labour), reached)
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
fn a_turn_shows_the_input_it_added_and_the_output_it_drew() {
    let html = render(&metered(), &highlighter());

    assert!(html.contains(
        r#"<span class="turn__usage" title="32,403 input this turn · 214 output this turn">↑ 32.4k ↓ 214</span>"#
    ));
}

#[test]
fn the_plaque_shows_the_largest_input_and_the_total_output() {
    let html = render(&metered(), &highlighter());

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

fn playground() -> Folio {
    Folio::read(Path::new("tests/fixtures/playground.jsonl")).expect("fixture parses")
}

#[test]
fn a_call_its_summary_states_in_full_has_no_fold_to_open() {
    let html = render(&playground(), &highlighter());

    // A read names a file and the lines it took, and both fit on the line, so
    // there is no subject left for a body to hold.
    assert!(html.contains(r#"<div class="marginalia marginalia--use marginalia--flat">"#));
    assert!(html.contains(r#"<span class="marginalia__note">lines 105–108</span>"#));
}

#[test]
fn a_reads_result_is_set_as_the_language_of_the_file_it_read() {
    let html = render(&playground(), &highlighter());

    // The harness numbers the lines it returns; those come off, and what is
    // left is highlighted as the Rust the file holds.
    assert!(!html.contains("1\t//! Parsing of Claude Code"));
    assert!(html.contains(r#"class="ink-source ink-rust""#));
}

#[test]
fn a_plan_is_set_as_the_markdown_document_it_is() {
    let html = render(&playground(), &highlighter());

    assert!(
        html.contains(r#"<span class="marginalia__gist">Pretty-print every built-in tool</span>"#)
    );
    assert!(html.contains("<h1>Pretty-print every built-in tool</h1>"));
    // The prompts the plan asks to be pre-approved sit under it, not in it.
    assert!(html.contains(r#"<li class="tool__prompt">"#));
}

#[test]
fn a_question_shows_every_option_it_offered() {
    let html = render(&playground(), &highlighter());

    assert!(html.contains(r#"<span class="tool__header">Read result</span>"#));
    assert!(html.contains(r#"<span class="tool__label">Highlight by extension</span>"#));
    assert!(html.contains("Keep the numbers"));
    // Every question a call asked is laid out at once, rather than one screen
    // at a time behind a control the reader has to work through.
    assert_eq!(
        html.matches(r#"<section class="tool__question">"#).count(),
        2
    );
    assert!(html.contains(r#"<span class="tool__header">Scope</span>"#));
}

#[test]
fn a_web_search_sets_the_links_it_found_as_links() {
    let html = render(&playground(), &highlighter());

    assert!(
        html.contains(r#"<a href="https://openfontlicense.org/">SIL Open Font License 1.1</a>"#)
    );
}

#[test]
fn a_background_tasks_answer_splits_into_its_facts_and_its_output() {
    let html = render(&playground(), &highlighter());

    assert!(html.contains(r#"<dl class="tool tool--facts">"#));
    assert!(html.contains("<dt>status</dt><dd>completed</dd>"));
    // The output is prose, so it is set as prose rather than as a terminal's.
    assert!(html.contains("<li>164 sessions</li>"));
}

fn answers() -> Folio {
    Folio::read(Path::new("tests/fixtures/answers.jsonl")).expect("fixture parses")
}

#[test]
fn an_answer_is_recovered_from_the_sentence_the_harness_buries_it_in() {
    let html = render(&answers(), &highlighter());

    assert!(html.contains(r#"<p class="tool__chosen">All projects, two-stage</p>"#));
    assert!(html.contains(r#"<p class="tool__chosen">Time + first prompt</p>"#));
    // Nothing of the framing sentence survives into the folio.
    assert!(!html.contains("Your questions have been answered"));
    assert!(!html.contains("You can now continue with these answers in mind"));
}

#[test]
fn a_question_quoting_code_does_not_break_the_pairs_apart() {
    let html = render(&answers(), &highlighter());

    // The question carries `raise ValueError("...")`, so the quotes the
    // sentence delimits its values with also appear inside one.
    assert!(html.contains(r#"<p class="tool__chosen">Leave it</p>"#));
}

#[test]
fn a_chosen_options_preview_is_shown_against_the_option_not_the_answer() {
    let html = render(&answers(), &highlighter());

    // The mockup is what the reader compared, so it belongs to the option in
    // the call; the answer names the option and no more.
    assert!(html.contains(r#"<p class="tool__chosen">In the meta line</p>"#));
    assert!(!html.contains("selected preview"));
    assert_eq!(html.matches("┌───────────────┐").count(), 1);
}

#[test]
fn several_selections_arrive_as_the_one_answer_they_were_given_as() {
    let html = render(&answers(), &highlighter());

    assert!(html.contains(
        r#"<p class="tool__chosen">Default policy by role, Valid-optional vs malformed, Derive, don't restate</p>"#
    ));
}

#[test]
fn an_answer_the_reader_typed_is_kept_whole() {
    let html = render(&answers(), &highlighter());

    // Typed instead of chosen, so it arrives unquoted under the other opening
    // and with no closing sentence.
    // The harness's own framing around it goes; that it was typed rather than
    // chosen is kept, as a mark on the answer.
    assert!(html.contains(
        r#"<p class="tool__chosen" data-typed>I want copy buttons, jump, and fancy collapse."#
    ));
    assert!(!html.contains("The user answered:"));
    assert!(!html.contains("no option selected"));
    // And a typed answer may quote something of its own.
    assert!(html.contains("it keeps matching items instead of dropping them"));
}

#[test]
fn a_question_that_was_never_answered_stands_as_the_note_it_is() {
    let html = render(&answers(), &highlighter());

    // A timeout is not an answer, so there is no pair to find and the harness's
    // own words are shown rather than forced into the shape of one.
    assert!(html.contains("No response after 60s"));
}

#[test]
fn a_result_that_only_says_the_call_worked_is_dropped() {
    let html = render(&playground(), &highlighter());

    // The write and the edit above them already show the file and the change;
    // an acknowledgement per file touched would crowd out the conversation.
    assert!(!html.contains("src/render.rs has been updated successfully"));
    assert!(!html.contains("File created successfully at"));
    assert!(!html.contains("Todos have been modified"));
    assert!(!html.contains("Launching skill"));
    // Entering plan mode answers with instructions meant for the model, so the
    // call stands alone as the one line it is.
    assert!(html.contains(r#"<span class="marginalia__tool">EnterPlanMode</span>"#));
    assert!(!html.contains("Entered plan mode"));
}

#[test]
fn a_result_written_down_as_text_blocks_is_weighed_like_a_plain_one() {
    let html = render(&playground(), &highlighter());

    // The harness records a background agent's launch as text blocks rather
    // than as a plain string, but that is how it was written down and not a
    // difference in what came back: the id and output file it names are for the
    // model to reach the agent again, and the call above it already shows what
    // the agent was sent.
    assert!(!html.contains("Async agent launched successfully"));
    assert!(html.contains("Survey the drollery bestiary"));
}

#[test]
fn a_result_that_warns_alongside_the_acknowledgement_is_kept() {
    let html = render(&playground(), &highlighter());

    // The match is on the acknowledgement itself, not on the tool, so an edit
    // that also reports the file changed underneath it still reaches a reader.
    assert!(html.contains("the file had been modified on disk"));
}

#[test]
fn a_terminals_colour_survives_into_the_folio() {
    let html = render(&playground(), &highlighter());

    // The named colours are the folio's to grind, so they arrive as classes the
    // stylesheet resolves against the parchment.
    assert!(html.contains(r#"<span class="ansi ansi--green">ok</span>"#));
    assert!(html.contains(r#"<span class="ansi ansi--red ansi--bold">FAILED</span>"#));
    assert!(html.contains(r#"<span class="ansi ansi--bright-black">"#));
    // No escape reaches the page, whether it carried colour or drove the
    // terminal.
    assert!(!html.contains('\u{1b}'));
}

#[test]
fn a_colour_a_tool_states_outright_is_carried_as_its_own_value() {
    let html = render(&playground(), &highlighter());

    // No palette token can stand for a 256-colour index or a 24-bit colour, so
    // these are the one place a pigment is set on the element itself.
    assert!(html.contains(r#"style="color:#ff8700""#));
    assert!(html.contains(r#"style="color:#785ac8""#));
}

#[test]
fn a_bodys_trailing_newline_is_not_set_as_a_line() {
    let html = render(&playground(), &highlighter());

    // A file's own trailing newline would otherwise leave an empty line against
    // the bottom edge of the fold, reading as content that isn't there.
    assert!(!html.contains("\n</code></pre>"));
}

#[test]
fn a_failure_sheds_the_tag_the_harness_wraps_it_in() {
    let html = render(&playground(), &highlighter());

    assert!(html.contains("String to replace not found in file."));
    assert!(!html.contains("tool_use_error"));
}

#[test]
fn an_answer_that_is_json_is_pretty_printed() {
    let html = render(&playground(), &highlighter());

    // Several tools answer with a JSON object on a single line.
    assert!(html.contains(r#"class="ink-source ink-json""#));
}

#[test]
fn a_tool_search_lists_the_tools_it_found() {
    let html = render(&playground(), &highlighter());

    assert!(html.contains(r#"<li class="tool__reference">WebSearch</li>"#));
    assert!(html.contains(r#"<li class="tool__reference">WebFetch</li>"#));
}

#[test]
fn a_tool_with_no_view_of_its_own_falls_back_to_json() {
    let html = render(&playground(), &highlighter());

    // An MCP tool is named by its server, and its input has whatever shape that
    // server gave it: no view can know it, so the call shows what it was sent.
    assert!(
        html.contains(r#"<span class="marginalia__tool">mcp__scriptorium__list_quires</span>"#)
    );
    assert!(html.contains("newest_first"));
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
fn the_appearance_corner_carries_a_candle_and_a_sun() {
    let html = render(&fixture(), &highlighter());

    // Both are set into every folio, and the scheme decides which is lit: each
    // pigment is a light-dark() pair whose off-scheme half is transparent, so
    // no folio is rendered for one scheme alone.
    assert!(html.contains(r#"class="luminary__candle""#));
    assert!(html.contains(r#"class="luminary__sun""#));
    assert!(html.contains("--flame: light-dark(transparent,"));
    assert!(html.contains("--sun-disc: light-dark(#c98a1c, transparent)"));
    // Decoration, so it is kept from assistive tech entirely.
    assert!(html.contains(r#"<div class="lamp" aria-hidden="true">"#));
    assert!(html.contains(r#"<svg class="luminary" viewBox="0 0 40 56" aria-hidden="true""#));
    // The light it casts over the leaf sits inside the lamp, so it stays
    // centred on the flame rather than on a corner guessed in the stylesheet.
    assert!(html.contains(r#"<span class="lamp__radiance"></span>"#));
}

#[test]
fn only_a_served_folio_offers_to_follow_the_session() {
    let highlighter = highlighter();
    let (statik, _) = set(&fixture(), &highlighter, Fonts::Fitted, Delivery::Static);
    let (served, _) = set(&fixture(), &highlighter, Fonts::Fitted, Delivery::Served);

    // The button itself, not `data-tail`: the app script carries that as a
    // selector, so a looser match finds the folio's own JS in either folio.
    const TAIL: &str = r#"<button class="dock__btn dock__btn--tail""#;
    // Only `serve` re-reads the session, so only a served folio can gain a
    // message to follow. Both still jump to the end, which is just a jump.
    assert!(!statik.contains(TAIL));
    assert!(served.contains(TAIL));
    assert!(statik.contains(r#"data-nav="end""#));
    assert!(served.contains(r#"data-nav="end""#));
}

#[test]
fn the_body_names_the_session_so_stored_state_is_scoped_to_this_folio() {
    let html = render(&fixture(), &highlighter());

    // Fold keys are a turn number and a position within it, so they name a
    // different marginalia in every session; without this the app script would
    // key them under one shared store and open panels across folios.
    assert!(html.contains(r#"<body data-folio="session">"#));
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
    // The read's result is set as the Rust the file holds, so the type it
    // declares arrives wrapped in the highlighter's spans rather than bare.
    assert!(html.contains("Quire"));
    assert!(html.contains("ink-storage"));
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
fn the_dock_leaps_to_either_end_of_the_folio() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"data-nav="top" aria-label="jump to top""#));
    assert!(html.contains(r#"data-nav="end" aria-label="jump to end""#));
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

#[test]
fn the_colophon_states_what_the_render_cost() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("taking 412 ms to set 2.9 MB."));
}

#[test]
fn an_inscribed_folio_keeps_no_placeholder() {
    let html = render(&fixture(), &highlighter());

    assert!(
        !html.contains("<!--folio:"),
        "a placeholder outlived inscription"
    );
}

#[test]
fn the_colophon_links_the_tool_to_its_home() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(
        r#"Written by <a href="https://example.invalid/scriptorium">claude-scriptorium</a> 0.1.0"#
    ));
}

/// The three faces' data URIs, whichever cut a folio carries. Their combined
/// length is what distinguishes one cut from the other.
fn embedded_font_bytes(html: &str) -> usize {
    html.match_indices("data:font/woff2;base64,")
        .map(|(start, marker)| {
            let rest = &html[start + marker.len()..];
            rest.find(')').expect("a data URI closes")
        })
        .sum()
}

#[test]
fn a_folio_inside_the_cut_faces_carries_only_them() {
    let (html, reached) = set(&fixture(), &highlighter(), Fonts::Fitted, Delivery::Static);

    assert!(reached.is_empty(), "nothing in the fixture is dropped");
    assert!(
        embedded_font_bytes(&html) < 700_000,
        "the cut faces are a fifth the whole ones, so a folio that needs \
         nothing more should be far under this"
    );
}

#[test]
fn asking_for_the_whole_faces_embeds_them_whatever_the_folio_sets() {
    let highlighter = highlighter();
    let (cut, _) = set(&fixture(), &highlighter, Fonts::Fitted, Delivery::Static);
    let (whole, reached) = set(&fixture(), &highlighter, Fonts::Whole, Delivery::Static);

    assert!(reached.is_empty(), "the folio itself still needs nothing");
    assert!(embedded_font_bytes(&whole) > embedded_font_bytes(&cut) * 3);
}

fn beyond_cut() -> Folio {
    Folio::read(Path::new("tests/fixtures/beyond_cut.jsonl")).expect("fixture parses")
}

#[test]
fn a_character_the_cut_faces_dropped_pulls_in_the_whole_ones() {
    let highlighter = highlighter();
    let (html, reached) = set(&beyond_cut(), &highlighter, Fonts::Fitted, Delivery::Static);
    let (whole, _) = set(&beyond_cut(), &highlighter, Fonts::Whole, Delivery::Static);

    // Junicode carries Cyrillic upstream, and the cut faces drop it, so setting
    // it in the cut faces would render worse than before they were cut.
    assert_eq!(reached.get(&'ч'), Some(&1));
    assert_eq!(
        embedded_font_bytes(&html),
        embedded_font_bytes(&whole),
        "a folio that reaches past the cut faces carries the whole ones"
    );
}

#[test]
fn a_character_no_face_ever_carried_is_not_a_reason_to_grow() {
    let (_, reached) = set(
        &beyond_cut(),
        &highlighter(),
        Fonts::Fitted,
        Delivery::Static,
    );

    // The fixture also sets CJK and an emoji. No embedded face has ever carried
    // either, so both fall back to the reader's own fonts exactly as they did
    // before the faces were cut: growing the folio would buy nothing.
    for character in ['日', '本', '語', '🎉'] {
        assert!(
            !reached.contains_key(&character),
            "{character} was never in any face, so it is not a regression"
        );
    }
}

#[test]
fn nothing_below_the_ascii_boundary_is_ever_dropped() {
    // `beyond_cut` skips whole runs of bytes under 0x80 without decoding them,
    // which is only sound while no such codepoint can be dropped.
    let ascii: String = (0..0x80u8).map(char::from).collect();

    assert!(render::beyond_cut(&ascii).is_empty());
}

#[test]
fn the_folios_own_chrome_stays_inside_the_cut_faces() {
    // The scan weighs the transcript and the source path, not this crate's own
    // markup, so a glyph added to the stylesheet or app script could otherwise
    // go quietly missing in every folio.
    for (name, source) in [
        ("illumination.css", include_str!("../src/illumination.css")),
        ("illumination.js", include_str!("../src/illumination.js")),
    ] {
        let reached = render::beyond_cut(source);
        assert!(
            reached.is_empty(),
            "{name} sets {reached:?}, which the cut faces drop: widen KEEP in \
             scripts/subset_fonts.py and re-run `just fonts`"
        );
    }
}
