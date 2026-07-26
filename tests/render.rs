use std::{collections::BTreeMap, path::Path, time::Duration};

use claude_scriptorium::{
    gloss,
    gloss::GlossKind,
    render,
    render::{Colophon, Delivery, Fonts, Labour, Scribe},
    transcript::{Content, Folio, Panel, PanelKind, Role, Side},
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

    assert_eq!(folio.turns().count(), 5);
}

#[test]
fn turn_roles_come_from_the_entry_tag() {
    let folio = fixture();

    let roles: Vec<Role> = folio.turns().map(|turn| turn.role).collect();
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

    let first = folio.turns().next().expect("the fixture opens with a turn");
    let Content::Text(text) = &first.content else {
        panic!("expected the opening turn to carry plain string content");
    };
    assert_eq!(text, "Explain the **quire** layout, please.");
}

#[test]
fn subagent_turns_are_marked() {
    let folio = fixture();

    let sidechains: Vec<bool> = folio.turns().map(|turn| turn.is_sidechain).collect();
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
        .turns()
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
fn a_tool_searchs_result_previews_the_tools_it_found() {
    let html = render(&playground(), &highlighter());

    // A search answers with references rather than text, so its summary line
    // had nothing to preview and came back blank: the one line a reader sees of
    // the answer must show what the fold below it lists.
    assert!(html.contains(r#"<span class="marginalia__gist">WebSearch, WebFetch</span>"#));
    assert!(html.contains(r#"<li class="tool__reference">WebSearch</li>"#));
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
fn the_lights_a_folio_is_read_by_are_the_control_that_chooses_them() {
    let html = render(&fixture(), &highlighter());

    // No separate toggle: the reader presses the light they want. The system is
    // offered as a reset rather than as a third light, since it names no light
    // of its own.
    assert!(html.contains(r#"<div class="luminaries" role="group""#));
    assert!(html.contains(r#"data-theme-choice="light""#));
    assert!(html.contains(r#"data-theme-choice="dark""#));
    assert!(html.contains(r#"class="theme-reset" type="button" data-theme-choice="system""#));
}

#[test]
fn each_light_carries_the_name_its_figure_cannot_say() {
    let html = render(&fixture(), &highlighter());

    for (choice, label) in [
        ("light", "read by daylight"),
        ("dark", "read by candlelight"),
        ("system", "follow the system"),
    ] {
        let button = html
            .split(&format!(r#"data-theme-choice="{choice}""#))
            .nth(1)
            .and_then(|rest| rest.split("</button>").next())
            .unwrap_or_else(|| panic!("the {choice} control is in the corner"));
        assert!(
            button.contains(&format!(r#"aria-label="{label}""#)),
            "the {choice} control has no name, and its figure cannot supply one"
        );
    }
}

#[test]
fn every_folio_carries_both_lights_and_lets_the_scheme_choose() {
    let html = render(&fixture(), &highlighter());

    // Sun and moon in one figure, a candle lit and smoking in the other, and
    // each pigment a light-dark() pair whose off-scheme half is transparent: one
    // set of rules, and no folio rendered for a single scheme.
    assert!(html.contains(r#"class="luminary__sun""#));
    assert!(html.contains(r#"class="luminary__moon""#));
    assert!(html.contains(r#"class="luminary__flame""#));
    assert!(html.contains(r#"class="luminary__smoke""#));
    assert!(html.contains("--flame: light-dark(transparent,"));
    assert!(html.contains("--smoke: light-dark(#9c9384, transparent)"));
    assert!(html.contains("--sun-disc: light-dark(#c98a1c, transparent)"));
    assert!(html.contains("--moon: light-dark(transparent, #cfc6ac)"));
    // The figures are decoration inside a named control, so they say nothing of
    // their own to assistive tech.
    assert!(
        html.contains(r#"<svg class="luminary__figure" viewBox="0 0 40 56" aria-hidden="true""#)
    );
    // The light each throws sits inside the figure that throws it, so the glow
    // comes from whichever is burning rather than from a corner the stylesheet
    // guessed at.
    assert!(html.contains(r#"<span class="luminary__radiance luminary__radiance--day"></span>"#));
    assert!(html.contains(r#"<span class="luminary__radiance luminary__radiance--night"></span>"#));
}

#[test]
fn the_reset_is_offered_only_once_a_light_has_been_chosen() {
    let html = render(&fixture(), &highlighter());

    // Keyed off the attribute the choice writes on the document, so a folio read
    // by whatever the system asks for carries no control saying so, and the
    // script needs to know nothing about it.
    assert!(html.contains(
        r#".theme-reset {
  display: none;"#
    ));
    assert!(html.contains(
        r#":root[data-theme] .theme-reset {
  display: grid;
}"#
    ));
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
fn a_skill_the_harness_injected_is_set_as_a_gloss_rather_than_a_user_turn() {
    let folio = Folio::read(Path::new("tests/fixtures/meta.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // The skill's own name labels the note; the base-directory line that
    // carried it is scaffolding and doesn't reach the page.
    assert!(html.contains(r#"class="turn turn--gloss" data-kind="skill""#));
    assert!(html.contains(r#"<span class="marginalia__gist">review-config</span>"#));
    assert!(!html.contains("Base directory for this skill"));
    // Nothing is hidden any more, so nothing is marked as hidden.
    assert!(!html.contains("data-meta"));
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
        .turns()
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
fn the_key_offers_a_chip_for_every_kind_of_panel() {
    let html = render(&fixture(), &highlighter());

    // Built from `PanelKind::EVERY` rather than a list in the markup, so a kind
    // enters the key by being declared. Weighed against the enum here for the
    // same reason: a hardcoded list in the test would drift the same way.
    for kind in PanelKind::EVERY {
        assert!(
            html.contains(&format!(
                r#"data-scope="{}" data-side="{}" aria-pressed="true""#,
                kind.label(),
                kind.side().label()
            )),
            "{} has no chip in the key, or is not grouped by its side",
            kind.label()
        );
    }
}

#[test]
fn the_key_is_its_own_panel_rather_than_the_searchs_control() {
    let html = render(&fixture(), &highlighter());

    // The key governs the search and the dock alike, so it belongs to neither.
    // A minimap added to the rail reads the same one. If it were nested back
    // inside `.search`, the dock's `.key__chip` lookup would still find it, so
    // this asserts the structure rather than trusting behaviour to catch it.
    let rail = html
        .split(r#"<div class="rail">"#)
        .nth(1)
        .expect("the rail holds the panels that answer to the key");
    let search = rail
        .split(r#"<div class="search""#)
        .nth(1)
        .and_then(|rest| rest.split(r#"<div class="key""#).next())
        .expect("the search sits in the rail");
    assert!(
        !search.contains("key__chip"),
        "the key is nested inside the search panel it is supposed to govern"
    );
    assert!(html.contains(r#"<div class="key" role="group""#));
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
fn the_dock_steps_along_the_sides_of_the_exchange() {
    let html = render(&fixture(), &highlighter());

    // The middle column steps between every message; the flanking columns seek
    // one side of the exchange, so the cool arrows find what reached the model
    // and the warm ones what it produced. They key on `data-side`, which the
    // renderer derives from `PanelKind::side`, so the dock cannot drift from the
    // classification the panels carry.
    assert!(html.contains(r#"data-nav="prev" data-side="entered""#));
    assert!(html.contains(r#"data-nav="next" data-side="model""#));
    assert!(html.contains(r#"class="dock__btn dock__btn--entered""#));
    assert!(html.contains(r#"class="dock__btn dock__btn--model""#));
}

#[test]
fn every_panel_carries_the_side_of_the_exchange_it_is_on() {
    let html = render(&glossed(), &highlighter());

    // `data-side` is the one place the dock and the stylesheet read the
    // classification from, so every panel must carry it and it must agree with
    // what the code says. A kind that reaches the page without one is a kind the
    // dock silently cannot step to.
    for kind in PanelKind::EVERY {
        let marked = format!(
            r#"data-kind="{}" data-side="{}""#,
            kind.label(),
            kind.side().label()
        );
        let present = html.contains(&format!(r#"data-kind="{}""#, kind.label()));
        assert!(
            !present || html.contains(&marked),
            "{} panels reach the page without the side the code gives them",
            kind.label()
        );
    }
    // The swatch reaches both sides and the asides, so this is not vacuous.
    for side in ["entered", "model", "aside"] {
        assert!(
            html.contains(&format!(r#"data-side="{side}""#)),
            "the swatch has no {side} panel to weigh"
        );
    }
}

#[test]
fn the_dock_leaps_to_either_end_of_the_folio() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains(r#"data-nav="top" aria-label="jump to top""#));
    assert!(html.contains(r#"data-nav="end" aria-label="jump to end""#));
}

#[test]
fn the_minimap_is_the_last_card_in_the_rail() {
    let html = render(&fixture(), &highlighter());

    // It answers to the key exactly as the search and the dock do, so it stands
    // in the same column, under the controls it navigates alongside.
    let rail = html
        .split(r#"<div class="rail">"#)
        .nth(1)
        .expect("the rail holds the cards that answer to the key");
    let dock = rail
        .find("<nav class=\"dock\"")
        .expect("the dock is in the rail");
    let minimap = rail
        .find(r#"<div class="minimap""#)
        .expect("the minimap is in the rail");
    assert!(
        dock < minimap,
        "the minimap is above the dock it sits under"
    );
    // An empty track: a band states the share of the document its panel takes,
    // which only the browser knows, so the bands are drawn from the panels
    // themselves rather than written out here.
    assert!(html.contains(r#"<div class="minimap__view"></div>"#));
    assert!(html.contains(r#"title="drag to scrub, scroll to zoom""#));
}

#[test]
fn every_kind_of_panel_is_pigmented_wherever_it_is_stood_for() {
    let html = render(&fixture(), &highlighter());

    // A chip in the key and a band in the minimap both stand for panels rather
    // than being one, and both take the pigment of the kind they answer for.
    // The two are declared together, so a kind cannot reach one and miss the
    // other; `note` is the catch-all and deliberately keeps the neutral ink.
    for kind in PanelKind::EVERY {
        let label = kind.label();
        if label == "note" {
            continue;
        }
        assert!(
            html.contains(&format!(
                r#".key__chip[data-scope="{label}"],
.minimap__band[data-kind="{label}"] {{"#
            )),
            "{label} has no pigment of its own in the key or the minimap"
        );
    }
}

#[test]
fn the_stylesheet_is_inlined_not_linked() {
    let html = render(&fixture(), &highlighter());

    assert!(html.contains("<style>"));
    assert!(html.contains(".ink-keyword"));
    assert!(!html.contains("<link"));
}

#[test]
fn the_scroll_thumb_yields_to_the_user_agent_under_forced_colours() {
    let html = render(&fixture(), &highlighter());

    // A styled ::-webkit-scrollbar is not repainted under forced colours, it
    // is left blank, so the scroll must sit inside the query that hands the
    // bar back to the UA. Losing this guard costs a high-contrast reader the
    // scrollbar entirely, which no visual test would catch.
    let rules: String = html
        .split("/*")
        .map(|c| c.split_once("*/").map_or(c, |(_, r)| r))
        .collect();
    let guard = rules
        .find("@media not (forced-colors: active)")
        .expect("the scroll is drawn inside a forced-colours guard");
    assert!(rules.contains("::-webkit-scrollbar-thumb"));
    assert!(
        rules
            .match_indices("::-webkit-scrollbar")
            .all(|(at, _)| at > guard),
        "every scrollbar pseudo-element rule sits after the guard opens"
    );

    // Firefox has no such pseudo-element and takes the standard properties
    // instead; Blink must not see them, or it discards the drawing above.
    assert!(rules.contains("@supports not selector(::-webkit-scrollbar)"));
    assert!(rules.contains("scrollbar-color:"));

    // Nothing may hide the bar or shrink it below the platform default: it is
    // both the position indicator and the drag target.
    assert!(!rules.contains("scrollbar-width:"));
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
fn the_symbols_real_sessions_write_stay_inside_the_cut() {
    // Angle-bracket placeholders (`⟨relative time⟩`) and an up arrow turned up
    // in the corpus and pushed those folios onto the whole faces, quadrupling
    // them. Their blocks cost under a kilobyte, so `KEEP` carries them; without
    // this a re-cut could quietly drop them again.
    for character in ['⟨', '⟩', '⬆'] {
        assert!(
            render::beyond_cut(&character.to_string()).is_empty(),
            "{character} is written by real sessions and must stay in the cut"
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
        (
            "illumination.core.js",
            include_str!("../src/illumination.core.js"),
        ),
        (
            "illumination.shell.js",
            include_str!("../src/illumination.shell.js"),
        ),
    ] {
        let reached = render::beyond_cut(source);
        assert!(
            reached.is_empty(),
            "{name} sets {reached:?}, which the cut faces drop: widen KEEP in \
             scripts/subset_fonts.py and re-run `just fonts`"
        );
    }
}

fn glossed() -> Folio {
    Folio::read(Path::new("tests/fixtures/glosses.jsonl")).expect("fixture parses")
}

#[test]
fn what_the_harness_wrote_into_the_session_is_set_as_its_own_panel() {
    let html = render(&glossed(), &highlighter());

    // A hook, a rule pulled into context, a skill, a slash command, and the
    // plan-mode boundaries each get a panel labelled by what wrote it.
    for kind in ["hook", "rule", "skill", "command", "plan"] {
        assert!(
            html.contains(&format!(r#"class="turn turn--gloss" data-kind="{kind}""#)),
            "no {kind} gloss was set"
        );
    }
    assert!(html.contains("No journal yet for branch"));
    assert!(html.contains("Derive aggressively rather than implementing by hand"));
    assert!(html.contains("The objective is to"));
}

#[test]
fn the_gloss_fixture_carries_every_kind_of_panel_to_compare() {
    // The fixture is the swatch a styling change is looked at against, so it
    // has to hold a speech, tool, and thinking panel beside the glosses: the
    // edges are only worth comparing in one document.
    let html = render(&glossed(), &highlighter());

    for kind in ["user", "assistant", "tool", "thinking"] {
        assert!(
            html.contains(&format!(r#"data-kind="{kind}""#)),
            "the swatch is missing a {kind} panel to compare the glosses against"
        );
    }
}

#[test]
fn a_built_in_skill_is_named_by_the_command_that_ran_it() {
    let html = render(&glossed(), &highlighter());

    // A skill with a directory on disk opens on it, which is how `gloss::meta`
    // knows one. A built-in has no such directory, so its instructions arrive as
    // bare prose and would read as a passing note; the command in front of them
    // is what says otherwise. Both must reach the page as skills, because the
    // model loads skills with no command at all and those already do.
    assert!(html.contains(r#"<span class="marginalia__gist">review</span>"#));
    assert!(html.contains("Gather this target's diff with"));
    let skills = html
        .matches(r#"class="turn turn--gloss" data-kind="skill""#)
        .count();
    assert_eq!(skills, 2, "a built-in skill is not set as a skill");
}

#[test]
fn a_hook_keeps_the_line_breaks_it_printed() {
    let html = render(&glossed(), &highlighter());

    // A hook's injected context is between the two readings: a program printed
    // it, so its breaks are its own, but it carries enough markdown that
    // preformatted text would throw away headings and lists. Set as ordinary
    // markdown, a list of files folds into one run of filenames, which is the
    // shape a hook reporting on a working tree always takes.
    assert!(html.contains("M  rustfmt.toml<br />\nM  src/gloss.rs"));
    assert!(!html.contains("M  rustfmt.toml\nM  src/gloss.rs</p>"));
    // Its markdown structure survives, which is what setting it as plain text
    // would have cost.
    assert!(html.contains("<p>You made these changes this session:</p>"));
}

#[test]
fn output_that_redrew_itself_keeps_only_what_the_terminal_was_left_showing() {
    let html = render(&glossed(), &highlighter());

    // A spinner emits one frame per carriage return, overwriting the line each
    // time, so a reader watching saw only the last. Set as they came the frames
    // have no newlines between them and run together into one line dozens of
    // frames long, which is the shape a build log most often takes.
    assert!(html.contains("⠹ Generating mutants"));
    assert!(!html.contains("⠋ Generating mutants"));
    assert!(!html.contains("⠙ Generating mutants"));
    // The lines around it are untouched: only the redrawn one collapses.
    assert!(html.contains("Checking rustfmt"));
    assert!(html.contains("Diff in src/main.rs at line 12"));
}

#[test]
fn inline_code_breaks_rather_than_running_out_of_the_fold_that_holds_it() {
    let html = render(&glossed(), &highlighter());

    // A fold's body *is* its box, so an unbreakable token in inline code (a
    // flag's comma-separated values, a deep path) runs straight out of it.
    // Inline code has no scroller of its own the way a block does.
    assert!(html.contains(
        ".folio :not(pre) > code {\n  padding: 0.1em 0.35em;\n  background: var(--leaf-sunk);"
    ));
    let inline = html
        .split(".folio :not(pre) > code {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("the inline-code rule is in the stylesheet");
    assert!(
        inline.contains("overflow-wrap: break-word"),
        "inline code can overflow whatever holds it"
    );
    // A block scrolls instead: breaking its lines would misreport the source.
    let block = html
        .split(".folio pre > code {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("the code-block rule is in the stylesheet");
    assert!(block.contains("overflow-x: auto"));
    assert!(!block.contains("overflow-wrap"));
}

#[test]
fn a_command_takes_the_speakers_pigment_rather_than_the_skills() {
    let html = render(&glossed(), &highlighter());

    // A command is a gloss by where it sits, not by whose it is: the user typed
    // it. Sharing the skill's ochre said the opposite, most loudly where a
    // command stands directly in front of the skill it loaded.
    assert!(html.contains("--command-edge: color-mix(in srgb, var(--user-edge)"));
    assert!(html.contains(
        r#".turn--gloss[data-kind="command"] {
  --gloss-edge: var(--command-edge);"#
    ));
}

#[test]
fn a_slash_command_is_set_as_a_command_rather_than_its_wrapper() {
    let html = render(&glossed(), &highlighter());

    assert!(html.contains(r#"<span class="marginalia__gist">/debug-gha</span>"#));
    assert!(html.contains(r#"<span class="marginalia__note">run 4812</span>"#));
    // The wrapper tags the harness records the command as never reach the page.
    assert!(!html.contains("command-name"));
    assert!(!html.contains("command-args"));
    // Neither does the caveat standing in front of it, which is a turn of its
    // own and would otherwise be set as the user speaking.
    assert!(!html.contains("Caveat: The messages below"));
    // A command that works the harness rather than the conversation is left
    // out entirely, along with what it printed.
    assert!(!html.contains("/copy"));
    assert!(!html.contains("Copied to clipboard"));
}

#[test]
fn one_firing_of_a_hook_is_one_panel_however_many_lines_it_wrote() {
    let folio = glossed();
    let html = render(&folio, &highlighter());

    // The Stop hook writes what it decided and what it injected as separate
    // lines sharing a `toolUseID`. They are one event, so the decision labels
    // the panel and the injected context fills its fold, rather than standing
    // as two panels saying half of it each.
    let stop = folio
        .panels()
        .into_iter()
        .find_map(|panel| match panel {
            Panel::Gloss(gloss)
                if gloss.gloss.gist.as_deref()
                    == Some("[claude-stop-finish] finishing pass triggered") =>
            {
                Some(gloss.gloss)
            }
            _ => None,
        })
        .expect("the hook's decision labels a panel");

    // The lines the hook wrote through the harness carry no command of their
    // own, so they name only the event; the hook's own line names its command
    // too, and still belongs to the same firing. Everything each had to say is
    // in the one fold, in the order it was said.
    assert_eq!(stop.notes, ["Stop", "added context", "claude-stop"]);
    // The injected context keeps the line breaks the hook printed; the stderr
    // beside it is output with no shape of its own.
    assert!(matches!(
        stop.body.as_slice(),
        [gloss::Body::Printed(_), gloss::Body::Plain(_)]
    ));
    assert!(html.contains("You made these changes this session"));
    assert!(html.contains("claude-stop: 2 files staged"));
}

#[test]
fn a_results_line_previews_what_came_back_rather_than_repeating_the_call() {
    let folio = Folio::read(Path::new("tests/fixtures/playground.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // The call sits in the same panel, so naming it again says nothing, and a
    // word saying the call was answered says no more. A success is therefore the
    // hint alone; only a failure is named, because only a failure is the
    // exception. The hint goes in the gist, which is the only part of the line
    // that can shrink (`.marginalia__note` and `.marginalia__outcome` are
    // `flex: none`), so a long first line ellipsises rather than running out.
    assert!(
        html.contains(r#"<span class="marginalia__gist">running 41 tests</span>"#),
        "a successful result's line is not its hint alone"
    );
    assert!(!html.contains(r#"<span class="marginalia__outcome">result</span>"#));
    assert!(html.contains(concat!(
        r#"<span class="marginalia__outcome">error</span>"#,
        r#"<span class="marginalia__gist">Exit code 127</span>"#
    )));
    // Neither the tool nor its path is repeated onto the result's own line.
    assert!(!html.contains(r#"<span class="marginalia__note">/home/scribe"#));
}

#[test]
fn a_reads_hint_sheds_the_line_numbering_the_way_its_fold_does() {
    let folio = Folio::read(Path::new("tests/fixtures/playground.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // A hint must show what the fold shows. `source` takes the harness's
    // numbering off the listing, so a hint drawn off the raw text would open
    // the line on a bare `1` that the fold below it never shows.
    assert!(html.contains(r#"<span class="marginalia__gist">//! Parsing of Claude Code"#));
    // Scoped to the gutter's own shape (a number against a tab) rather than to
    // a leading digit: a `TaskGet` call's gist is the task's id, and a looser
    // assertion catches that instead of the bug.
    assert!(!html.contains("<span class=\"marginalia__gist\">1\t"));
}

#[test]
fn a_results_hint_is_its_first_line_with_the_terminals_colour_taken_out() {
    // `playground.jsonl` is the fixture that carries ANSI escapes at all;
    // `session.jsonl` has none, so asserting this against it proves nothing.
    let folio = Folio::read(Path::new("tests/fixtures/playground.jsonl")).expect("fixture parses");

    let html = render(&folio, &highlighter());

    // The colour is a fact about the body, which grinds it into `ansi--`
    // classes. A hint carrying the escapes would set their bytes as text.
    assert!(html.contains(r#"<span class="marginalia__gist">running 3 tests</span>"#));
    assert!(
        !html.contains('\u{1b}'),
        "an escape reached the folio as a byte rather than a class"
    );
}

#[test]
fn plan_mode_states_both_boundaries_and_whether_a_plan_was_written() {
    let html = render(&glossed(), &highlighter());

    assert!(html.contains(r#"<span class="marginalia__gist">entered plan mode</span>"#));
    assert!(html.contains(r#"<span class="marginalia__note">left, plan written</span>"#));
    // The reminder that plan mode is still on says nothing that has changed.
    assert_eq!(html.matches(">entered plan mode<").count(), 1);
}

#[test]
fn scaffolding_and_duplicated_hook_output_stay_out_of_the_folio() {
    let folio = glossed();
    let panels = folio.panels();

    // The tool and skill inventories, the checklist reminder, and the harness's
    // own timings are not notes a reader can act on. Neither is a hook's
    // control-protocol JSON, whose payload arrives as the notes beside it.
    let html = render(&folio, &highlighter());
    assert!(!html.contains("distil the branch's working memory"));
    assert!(!html.contains("hookSpecificOutput"));
    assert!(!html.contains("15086"));
    // Every panel that is a gloss says something: none is a bare line
    // reporting that a thing ran.
    for panel in &panels {
        if let Panel::Gloss(gloss) = panel {
            assert!(
                gloss.gloss.gist.is_some() || !gloss.gloss.body.is_empty(),
                "a gloss with nothing to say reached the folio: {gloss:?}"
            );
        }
    }
}

#[test]
fn only_the_kinds_off_the_axis_are_skipped_by_the_navigation_dock() {
    let html = render(&glossed(), &highlighter());

    // The dock steps along `data-side`, so a note the user themselves put into
    // the session (a command they typed, a skill they wrote, a hook they
    // installed) is a stop like any other: it reached the model, and the reader
    // wants it. What it skips is the kinds deliberately off the axis, which are
    // context reached for rather than stopped at.
    for kind in PanelKind::EVERY {
        let stepped = kind.side() != Side::Aside;
        let expected = match kind.side() {
            Side::Entered => "entered",
            Side::Model => "model",
            Side::Aside => "aside",
        };
        assert_eq!(
            stepped,
            expected != "aside",
            "{} is classified inconsistently",
            kind.label()
        );
    }
    // A rule is the user's own writing pulled in, so it is theirs; a plan
    // boundary is the model reporting on its own working, so it is its. Only the
    // catch-all is genuinely off the axis, and only it is skipped.
    assert_eq!(PanelKind::Gloss(GlossKind::Rule).side(), Side::Entered);
    assert_eq!(PanelKind::Gloss(GlossKind::Plan).side(), Side::Model);
    assert_eq!(PanelKind::Gloss(GlossKind::Note).side(), Side::Aside);
    // The panels that carry them reach the page marked as such, so the dock's
    // selector actually finds and misses the right ones.
    assert!(html.contains(r#"data-kind="plan" data-side="model""#));
    assert!(html.contains(r#"data-kind="rule" data-side="entered""#));
    assert!(html.contains(r#"data-kind="hook" data-side="entered""#));
}

#[test]
fn hooks_sharing_one_event_stay_their_own_panels() {
    let folio = glossed();

    // A `toolUseID` names the event, not the hook, and one `SessionStart` runs
    // every hook matching it. Keying the bundle on the id alone folds them all
    // into a single panel claiming to be one hook that ran four commands.
    let commands: Vec<String> = folio
        .panels()
        .into_iter()
        .filter_map(|panel| match panel {
            Panel::Gloss(gloss) if gloss.gloss.gist.as_deref() == Some("SessionStart:clear") => {
                Some(gloss.gloss.notes.join(", "))
            }
            _ => None,
        })
        .collect();

    assert_eq!(commands, ["claude-branch-journal", "claude-git-status"]);
}

#[test]
fn each_result_joins_the_panel_holding_the_call_it_answers() {
    let folio = glossed();

    // Calls issued together are written as one assistant line each, so they
    // become several panels. Taking the newest panel for every result piles
    // them all onto the last call and leaves its siblings showing none.
    let paired: Vec<(usize, usize)> = folio
        .panels()
        .iter()
        .filter_map(|panel| match panel {
            Panel::Speech(speech) if speech.kind() == PanelKind::Tool => Some((
                speech.blocks.iter().filter(|b| b.is_call()).count(),
                speech.blocks.iter().filter(|b| b.is_result()).count(),
            )),
            _ => None,
        })
        .collect();

    assert_eq!(paired, [(1, 1), (1, 1)], "each call keeps its own result");
}
