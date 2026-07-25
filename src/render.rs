//! Turning a parsed folio into a self-contained HTML document.

use std::{path::Path, sync::LazyLock};

use base64::{Engine, engine::general_purpose::STANDARD};
use comrak::{
    Options, markdown_to_html_with_plugins, options::Plugins, plugins::syntect::SyntectAdapter,
};
use jiff::{Timestamp, Zoned, tz::TimeZone};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use serde_json::Value;

use crate::transcript::{
    Block, Folio, ImageSource, Known, Panel, PanelKind, ToolResultContent, Usage,
};

/// One embedded web font: the family and posture the stylesheet asks for, the
/// weight range the variable file covers, and the woff2 bytes themselves.
struct Face {
    family: &'static str,
    style: &'static str,
    weight: &'static str,
    woff2: &'static [u8],
}

/// The fonts are inlined so a folio stays self-contained (see the rendering
/// invariants in CLAUDE.md). `just fonts` vendors these from upstream.
///
/// Junicode 2 (serif body, roman + italic) and Fira Code (monospace) are
/// variable, so one file per posture spans every weight; UnifrakturCook is the
/// single-weight blackletter used for headings and dropped versals.
const FACES: [Face; 4] = [
    Face {
        family: "Junicode",
        style: "normal",
        weight: "300 700",
        woff2: include_bytes!("fonts/JunicodeVF-Roman.woff2"),
    },
    Face {
        family: "Junicode",
        style: "italic",
        weight: "300 700",
        woff2: include_bytes!("fonts/JunicodeVF-Italic.woff2"),
    },
    Face {
        family: "UnifrakturCook",
        style: "normal",
        weight: "400",
        woff2: include_bytes!("fonts/UnifrakturCook.woff2"),
    },
    Face {
        family: "Fira Code",
        style: "normal",
        weight: "300 700",
        woff2: include_bytes!("fonts/FiraCode-VF.woff2"),
    },
];

/// Copyright notices for the embedded fonts, emitted into every folio. The SIL
/// OFL requires each copy of the font to carry its copyright and license; the
/// woff2 metadata does for most faces, but this notice covers every artifact
/// uniformly. Full license texts are vendored under src/fonts/licenses.
const FONT_NOTICES: [(&str, &str); 3] = [
    ("Junicode", "Copyright 2025 Peter S. Baker"),
    ("Fira Code", "Copyright 2014 The Fira Code Project Authors"),
    (
        "UnifrakturCook",
        "Copyright 2010 j. 'mach' wust (Reserved Font Name UnifrakturCook), Copyright 2009 Peter Wiegel",
    ),
];

/// The font attribution as an HTML comment for the top of every folio.
fn font_notice() -> String {
    let mut notice = String::from(
        "<!-- Embedded fonts, SIL Open Font License 1.1 (https://openfontlicense.org):",
    );
    for (family, copyright) in FONT_NOTICES {
        notice.push_str(&format!("\n     {family}: {copyright}"));
    }
    notice.push_str("\n-->");
    notice
}

/// The `@font-face` block, built once: base64 is deterministic and the bytes
/// are compile-time constants, so encoding on every render would be wasted work
/// on the `serve` hot path.
static FONT_FACES: LazyLock<String> = LazyLock::new(|| {
    FACES
        .iter()
        .map(|face| {
            let data = STANDARD.encode(face.woff2);
            format!(
                "@font-face{{font-family:\"{}\";font-style:{};font-weight:{};font-display:swap;\
                 src:url(data:font/woff2;base64,{}) format(\"woff2\")}}",
                face.family, face.style, face.weight, data,
            )
        })
        .collect()
});

/// The generation metadata recorded in every folio's plaque.
pub struct Colophon {
    pub generated: Timestamp,
    pub tool: &'static str,
    pub version: &'static str,
}

/// Renders folios, carrying the decisions a render depends on: how markdown
/// becomes HTML, how code gets highlighted, and which zone timestamps read in.
pub struct Scribe<'a> {
    options: Options<'a>,
    plugins: Plugins<'a>,
    timezone: TimeZone,
}

impl<'a> Scribe<'a> {
    pub fn new(highlighter: &'a SyntectAdapter, timezone: TimeZone) -> Self {
        let mut options = Options::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.tasklist = true;
        options.extension.autolink = true;
        options.extension.footnotes = true;
        options.render.github_pre_lang = true;

        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(highlighter);

        Self {
            options,
            plugins,
            timezone,
        }
    }

    fn markdown(&self, source: &str) -> Markup {
        PreEscaped(markdown_to_html_with_plugins(
            source,
            &self.options,
            &self.plugins,
        ))
    }

    fn zoned(&self, timestamp: Timestamp) -> Zoned {
        timestamp.to_zoned(self.timezone.clone())
    }

    pub fn folio(&self, folio: &Folio, colophon: &Colophon) -> Markup {
        let title = format!("folio {}", folio.session_id());
        let panels = folio.panels();
        let left_border = margin_strip(border_seed(folio.session_id(), "left"));
        let right_border = margin_strip(border_seed(folio.session_id(), "right"));
        html! {
            (DOCTYPE)
            (PreEscaped(font_notice()))
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    title { (title) }
                    style {
                        (PreEscaped(FONT_FACES.as_str()))
                        (PreEscaped(include_str!("illumination.css")))
                    }
                    // The folio's own behaviour (theme, and more to come). It
                    // sits in <head> so the stored theme applies before the
                    // body paints, avoiding a flash of the wrong scheme.
                    script { (PreEscaped(include_str!("illumination.js"))) }
                }
                body {
                    // Illuminated borders down each outer margin: a per-session
                    // strip of vine sections with drolleries seated among them,
                    // tiled by the stylesheet. Purely decorative, so hidden from
                    // assistive tech.
                    div .margin.margin--left style=(format!("background-image:url({left_border})")) aria-hidden="true" {}
                    div .margin.margin--right style=(format!("background-image:url({right_border})")) aria-hidden="true" {}
                    // The folio's plaque: title, facts, and colophon, tucked
                    // into the top corner so the reading column is pure
                    // transcript. A pure-CSS hover/focus disclosure (no script):
                    // the panel shows while the seal is hovered or focused, and
                    // a click focuses the seal to hold it open.
                    div .plaque {
                        button .plaque__seal type="button" aria-label="folio details" title="folio details" { "❦" }
                        div .plaque__panel {
                            h1 .plaque__title { (title) }
                            dl .plaque__facts {
                                dt { "source" } dd { code { (folio.source.display().to_string()) } }
                                dt { "turns" } dd { (panels.len()) }
                                @if let Some(first) = panels.first() {
                                    dt { "opened" } dd { (self.stamp(first.timestamp)) }
                                }
                                @if let Some(usage) = folio.usage() {
                                    dt { "tokens" } dd title=(breakdown(&usage)) { (tally(&usage)) }
                                }
                            }
                            p .plaque__colophon {
                                "Written by " (colophon.tool) " " (colophon.version)
                                " on " (self.stamp(colophon.generated)) "."
                            }
                            p .plaque__colophon {
                                "Set in Junicode, Fira Code, and UnifrakturCook, under the "
                                a href="https://openfontlicense.org" { "SIL Open Font License" } "."
                            }
                        }
                    }
                    // A fixed search widget: highlights matches and steps
                    // through them, wired by the app script.
                    div .search role="search" {
                        div .search__bar {
                            input .search__input type="search" placeholder="search folio" aria-label="search folio";
                            span .search__count aria-live="polite" {}
                            button .search__nav type="button" data-search-nav="prev" aria-label="previous match" { "‹" }
                            button .search__nav type="button" data-search-nav="next" aria-label="next match" { "›" }
                        }
                        // Restrict the search to chosen kinds of message, so a
                        // query need not wade through tool output or reasoning.
                        div .search__scopes role="group" aria-label="search in" {
                            button .search__scope.search__scope--user type="button" data-scope="user" aria-pressed="true" { "user" }
                            button .search__scope.search__scope--assistant type="button" data-scope="assistant" aria-pressed="true" { "assistant" }
                            button .search__scope type="button" data-scope="tool" aria-pressed="true" { "tool" }
                            button .search__scope type="button" data-scope="thinking" aria-pressed="true" { "thinking" }
                        }
                    }
                    // A fixed dock: jump between messages, leap to the end and
                    // follow new ones (tail -f), and fold every tool call open
                    // or shut. Wired by the app script. The nav grid is three
                    // columns of up/down arrows: the middle steps between all
                    // messages, flanked by a user (blue) and an assistant
                    // (orange) column that seek only that speaker.
                    nav .dock aria-label="folio navigation" {
                        div .dock__nav {
                            button .dock__btn .dock__btn--user type="button" data-nav="prev" data-role="user" aria-label="previous user message" title="previous user message" { "▲" }
                            button .dock__btn type="button" data-nav="prev" aria-label="previous message" title="previous message" { "▲" }
                            button .dock__btn .dock__btn--assistant type="button" data-nav="prev" data-role="assistant" aria-label="previous assistant message" title="previous assistant message" { "▲" }
                            button .dock__btn .dock__btn--user type="button" data-nav="next" data-role="user" aria-label="next user message" title="next user message" { "▼" }
                            button .dock__btn type="button" data-nav="next" aria-label="next message" title="next message" { "▼" }
                            button .dock__btn .dock__btn--assistant type="button" data-nav="next" data-role="assistant" aria-label="next assistant message" title="next assistant message" { "▼" }
                        }
                        // Jump to the last message, and a follow toggle that
                        // re-pins the newest message's start on every reload
                        // until the reader scrolls away.
                        div .dock__tail {
                            button .dock__btn type="button" data-nav="end" aria-label="jump to end" title="jump to end" { "⤓" }
                            button .dock__btn .dock__btn--tail type="button" data-tail="toggle" aria-pressed="false" aria-label="follow new messages" title="follow new messages, like tail -f" { "⇊" }
                        }
                        div .dock__fold {
                            button .dock__btn .dock__btn--fold type="button" data-fold="expand" aria-label="expand all" title="expand all" { span .dock__chevron { "⌃" } span .dock__chevron { "⌄" } }
                            button .dock__btn .dock__btn--fold type="button" data-fold="collapse" aria-label="collapse all" title="collapse all" { span .dock__chevron { "⌄" } span .dock__chevron { "⌃" } }
                        }
                    }
                    // Presentation controls, opposite the navigation dock:
                    // light / dark / system, wired by the app script and
                    // defaulting to the reader's system preference.
                    div .controls {
                        div .theme-toggle role="group" aria-label="colour theme" {
                            button type="button" data-theme-choice="light" { "light" }
                            button type="button" data-theme-choice="system" aria-pressed="true" { "system" }
                            button type="button" data-theme-choice="dark" { "dark" }
                        }
                    }
                    main .folio {
                        @for panel in &panels {
                            (self.panel(panel))
                        }
                    }
                }
            }
        }
    }

    fn stamp(&self, timestamp: Timestamp) -> String {
        self.zoned(timestamp)
            .strftime("%Y-%m-%d %H:%M:%S %Z")
            .to_string()
    }

    fn panel(&self, panel: &Panel) -> Markup {
        let kind = panel.kind();
        // A speaker's leading paragraph opens with a rubricated versal (a
        // dropped blackletter initial the stylesheet draws): tag the block that
        // carries it so only that one is decorated, and not any prose that
        // resumes after a tool call. Tool and thinking panels carry no versal.
        let opening = matches!(kind, PanelKind::User | PanelKind::Assistant)
            .then(|| panel.blocks.iter().position(Block::is_visible_text))
            .flatten();
        html! {
            article id={ "turn-" (panel.turn_number) }
                class={ "turn turn--" (panel.role.as_str()) } data-kind=(kind.label())
                data-turn=(panel.turn_number)
                data-sidechain[panel.is_sidechain] data-meta[panel.is_meta] {
                header .turn__meta {
                    span .turn__role { (kind.label()) }
                    @if let Some(model) = &panel.model {
                        span .turn__model {
                            (model)
                            @if let Some(effort) = &panel.effort {
                                " " span .turn__effort { "(" (effort) ")" }
                            }
                        }
                    }
                    @if let Some(usage) = &panel.usage {
                        span .turn__usage title=(breakdown(usage)) { (tally(usage)) }
                    }
                    time .turn__time datetime=(panel.timestamp.to_string()) { (self.stamp(panel.timestamp)) }
                    a .turn__index href={ "#turn-" (panel.turn_number) } { "#" (panel.turn_number) }
                }
                @for (index, block) in panel.blocks.iter().enumerate() {
                    (self.block(block, Some(index) == opening))
                }
            }
        }
    }

    fn block(&self, block: &Block, versal: bool) -> Markup {
        let known = match block {
            Block::Known(known) => known,
            Block::Unknown(value) => return unknown(value),
        };
        match known {
            Known::Text { text } => {
                html! { div .block.block--text data-versal[versal] { (self.markdown(text)) } }
            }
            // Redacted thinking arrives as an empty string with only a
            // signature: the reasoning happened but wasn't recorded. Mark it
            // rather than dropping it to nothing (which leaves a bare turn).
            Known::Thinking { thinking } if thinking.trim().is_empty() => html! {
                p .block.block--redacted { "reasoning redacted" }
            },
            Known::Thinking { thinking } => html! {
                section .block.block--thinking {
                    (self.markdown(thinking))
                }
            },
            Known::ToolUse { name, input } => html! {
                details .marginalia.marginalia--use {
                    summary {
                        span .marginalia__tool { (name) }
                        @if let Some(gist) = gist(input) {
                            span .marginalia__gist { (gist) }
                        }
                        @if let Some(note) = note(input) {
                            span .marginalia__note { (note) }
                        }
                    }
                    (self.tool_body(name, input))
                }
            },
            Known::ToolResult { content, is_error } => html! {
                details .marginalia.marginalia--result data-error[*is_error] {
                    summary { @if *is_error { "error" } @else { "result" } }
                    @match content {
                        ToolResultContent::Text(text) => pre { code { (text) } },
                        ToolResultContent::Blocks(blocks) => @for block in blocks { (self.block(block, false)) },
                    }
                }
            },
            Known::Image { source } => image(source),
        }
    }

    /// The body of a tool call, which fills the fold the way a result's body
    /// does: the summary line carries the labelling, the body carries only the
    /// call's subject. A handful of common tools get a bespoke view; everything
    /// else (and any call whose input doesn't match the shape this view expects)
    /// falls back to pretty-printed JSON, the same treatment an unrecognized
    /// block gets.
    fn tool_body(&self, name: &str, input: &Value) -> Markup {
        let special = match name {
            "Bash" => self.bash_body(input),
            "Write" => self.write_body(input),
            "Edit" => self.edit_body(input),
            "TodoWrite" => self.todo_body(input),
            _ => None,
        };
        special.unwrap_or_else(|| json(input))
    }

    fn bash_body(&self, input: &Value) -> Option<Markup> {
        let command = input.get("command")?.as_str()?;
        Some(self.code_block("bash", command))
    }

    fn write_body(&self, input: &Value) -> Option<Markup> {
        let path = input.get("file_path")?.as_str()?;
        let content = input.get("content")?.as_str()?;
        Some(self.code_block(lang_for_path(path), content))
    }

    fn edit_body(&self, input: &Value) -> Option<Markup> {
        let old = input.get("old_string")?.as_str()?;
        let new = input.get("new_string")?.as_str()?;
        Some(self.code_block("diff", &unified_diff(old, new)))
    }

    fn todo_body(&self, input: &Value) -> Option<Markup> {
        let todos = input.get("todos")?.as_array()?;
        Some(html! {
            ul .tool.tool--todos {
                @for todo in todos {
                    @let content = todo.get("content").and_then(Value::as_str).unwrap_or("");
                    @let status = todo.get("status").and_then(Value::as_str).unwrap_or("pending");
                    li .tool__todo data-status=(status) { (content) }
                }
            }
        })
    }

    /// A fenced code block run through the markdown path so it picks up syntax
    /// highlighting. The fence is grown past the longest backtick run in the
    /// source, so a body that itself contains backticks can't break out of it.
    fn code_block(&self, lang: &str, code: &str) -> Markup {
        let fence = "`".repeat(longest_backtick_run(code).max(2) + 1);
        self.markdown(&format!("{fence}{lang}\n{code}\n{fence}"))
    }
}

/// One border cell's coordinate box: the composed strip is one cell wide, and
/// each vine or drollery is authored to sit in this 90x210 space.
const CELL_WIDTH: u32 = 90;
const CELL_HEIGHT: u32 = 210;

/// Cells the border is stitched from before the strip repeats down a long
/// folio. Long enough that a full bestiary's worth of drolleries appears before
/// the strip recurs, at the cost of a larger data URI.
const STRIP_CELLS: usize = 48;

/// The vine cell, the border's default section.
const VINE_CELL: &str = include_str!("drolleries/vine.svg");

/// A vine stub that eases the border into a drollery: baked above each creature
/// and mirrored below, its stroke fading to transparent (via the `vinefade`
/// gradient) as it nears the beast, so the vine dissolves in and coalesces back
/// rather than stopping dead at a gap.
const TRAIL: &str = include_str!("drolleries/trail.svg");

/// The fade the trail's stroke draws with: opaque vine gold at the seam,
/// transparent by the time it reaches the creature. `userSpaceOnUse` resolves it
/// in each trail's own (possibly mirrored) coordinate space, so one definition
/// serves every drollery and its mirror.
const VINE_FADE: &str = "<defs><linearGradient id=\"vinefade\" gradientUnits=\"userSpaceOnUse\" \
     x1=\"0\" y1=\"0\" x2=\"0\" y2=\"54\">\
     <stop offset=\"0\" stop-color=\"#c1912f\"/>\
     <stop offset=\"1\" stop-color=\"#c1912f\" stop-opacity=\"0\"/></linearGradient></defs>";

/// The bestiary a border seats between vines, each paired with the `(dx, dy)`
/// nudge that centres it in its cell: `dx` on the vine's centreline (x=45), and
/// `dy` in the gap between the fading trail above and its mirror below (the
/// creatures are drawn low in the 90x210 box, so most lift toward the gap's
/// centre at y=105). Several carry a tail or ear that pulls the bounding box off
/// centre, so the nudges are measured, not zero. Each is line-and-pigment art
/// authored in that cell (see CLAUDE.md); a background-image SVG can't reach the
/// palette variables, so the colours are baked to read on either parchment.
const DROLLERIES: [(&str, i32, i32); 10] = [
    (include_str!("drolleries/snail.svg"), 0, -18),
    (include_str!("drolleries/budgie.svg"), -7, -11),
    (include_str!("drolleries/cockatiel.svg"), -8, 2),
    (include_str!("drolleries/cardinal.svg"), -6, -3),
    (include_str!("drolleries/fish.svg"), -8, 0),
    (include_str!("drolleries/butterfly.svg"), 0, -23),
    (include_str!("drolleries/frog.svg"), 0, -32),
    (include_str!("drolleries/cat.svg"), -5, -23),
    (include_str!("drolleries/hare.svg"), 0, -14),
    (include_str!("drolleries/stag.svg"), -2, -21),
];

/// A small deterministic PRNG (SplitMix64) so a border's section layout is
/// stable for a seed but varies cell to cell.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// A stable seed for one border of a folio: the session id folded with a
/// per-side salt (FNV-1a), so the two borders differ but each is reproducible.
/// FNV-1a keeps the mapping self-contained rather than leaning on stdlib hash
/// output, whose value isn't guaranteed across Rust releases.
fn border_seed(session_id: &str, salt: &str) -> u64 {
    session_id
        .bytes()
        .chain(salt.bytes())
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

/// A section of a border: a plain vine cell, or a drollery with the offset that
/// centres it.
#[derive(Clone, Copy)]
enum BorderCell {
    Vine,
    Drollery {
        svg: &'static str,
        dx: i32,
        dy: i32,
        flip: bool,
    },
}

/// The whole bestiary in a fresh random order (Fisher-Yates). Drawing drolleries
/// from a bag that refills when drained means every creature appears before any
/// repeats, so a border cycles through all of them rather than a fixed few.
fn shuffled_bag(rng: &mut Rng) -> Vec<usize> {
    let mut bag: Vec<usize> = (0..DROLLERIES.len()).collect();
    for i in (1..bag.len()).rev() {
        bag.swap(i, rng.next() as usize % (i + 1));
    }
    bag
}

/// One border as a sequence of cells: mostly vine, with drolleries seated at
/// intervals. The seam cells stay vine so the strip tiles cleanly, and no two
/// drolleries sit adjacent so each creature reads on its own.
fn border_cells(seed: u64) -> Vec<BorderCell> {
    let mut rng = Rng(seed);
    let mut bag = shuffled_bag(&mut rng);
    let mut cells = Vec::with_capacity(STRIP_CELLS);
    let mut previous_was_drollery = false;
    for index in 0..STRIP_CELLS {
        let seam = index == 0 || index == STRIP_CELLS - 1;
        let drollery = !seam && !previous_was_drollery && rng.next().is_multiple_of(3);
        if drollery {
            if bag.is_empty() {
                bag = shuffled_bag(&mut rng);
            }
            let (svg, dx, dy) = DROLLERIES[bag.pop().expect("bag refilled when empty")];
            let flip = rng.next().is_multiple_of(2);
            cells.push(BorderCell::Drollery { svg, dx, dy, flip });
        } else {
            cells.push(BorderCell::Vine);
        }
        previous_was_drollery = drollery;
    }
    cells
}

/// A tiling border strip for one side of a folio as a base64 SVG data URI. The
/// renderer sets it as a `background-image`; the stylesheet repeats it to fill
/// the leaf, however tall (see the border invariants in CLAUDE.md).
fn margin_strip(seed: u64) -> String {
    let height = STRIP_CELLS as u32 * CELL_HEIGHT;
    let mut inner = String::new();
    for (index, cell) in border_cells(seed).iter().enumerate() {
        let y = index as u32 * CELL_HEIGHT;
        // Frame a drollery with the fading trail above and its vertical mirror
        // below, nudging the creature to centre it between them; vine cells
        // stand alone.
        let content = match cell {
            BorderCell::Vine => VINE_CELL.to_string(),
            BorderCell::Drollery { svg, dx, dy, flip } => {
                // Mirror a flipped creature about the cell's centreline (x=45)
                // so it stays seated on the vine; the trail frames it either way.
                let place = if *flip {
                    format!("translate(90,0) scale(-1,1) translate({dx},{dy})")
                } else {
                    format!("translate({dx},{dy})")
                };
                format!(
                    "{TRAIL}<g transform=\"{place}\">{svg}</g>\
                     <g transform=\"translate(0,{CELL_HEIGHT}) scale(1,-1)\">{TRAIL}</g>"
                )
            }
        };
        inner.push_str(&format!("<g transform=\"translate(0,{y})\">{content}</g>"));
    }
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{CELL_WIDTH}\" \
         height=\"{height}\" viewBox=\"0 0 {CELL_WIDTH} {height}\">{VINE_FADE}{inner}</svg>"
    );
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg))
}

/// A one-line summary of a tool call, drawn from whichever field carries the
/// subject of the call. A call that describes itself is taken at its word: the
/// description says what the call is *for*, which reads better folded than the
/// command or prompt it stands in front of, and the body shows that anyway.
fn gist(input: &Value) -> Option<&str> {
    [
        "description",
        "command",
        "file_path",
        "pattern",
        "path",
        "url",
        "prompt",
    ]
    .iter()
    .find_map(|field| input.get(field)?.as_str())
}

/// A qualifier on a tool call: an option that changes what the call does, which
/// the body it acts on doesn't show.
fn note(input: &Value) -> Option<&'static str> {
    input
        .get("replace_all")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        .then_some("replace all")
}

/// Token usage at a glance: what the model read, then what it wrote.
fn tally(usage: &Usage) -> String {
    format!(
        "↑ {} ↓ {}",
        compact(usage.context()),
        compact(usage.output_tokens)
    )
}

/// The same usage spelled out, for the title a reader hovers to see where the
/// context came from: a cache read costs a fraction of fresh input, so the
/// split is the interesting part.
fn breakdown(usage: &Usage) -> String {
    format!(
        "{} input · {} cache write · {} cache read · {} output",
        compact(usage.input_tokens),
        compact(usage.cache_creation_input_tokens),
        compact(usage.cache_read_input_tokens),
        compact(usage.output_tokens),
    )
}

/// A token count short enough to sit in a line of chrome: exact below a
/// thousand, then one decimal place per magnitude, with a bare `.0` trimmed.
fn compact(tokens: u64) -> String {
    let (scaled, suffix) = match tokens {
        ..1_000 => return tokens.to_string(),
        1_000..1_000_000 => (tokens as f64 / 1_000.0, "k"),
        _ => (tokens as f64 / 1_000_000.0, "M"),
    };
    let rounded = format!("{scaled:.1}");
    format!("{}{suffix}", rounded.strip_suffix(".0").unwrap_or(&rounded))
}

/// The language token for a path, taken from its extension. syntect resolves it
/// by extension or name, so the bare extension is enough; an empty token (no
/// extension) highlights as plain text.
fn lang_for_path(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
}

/// An `Edit`'s before/after rendered as a diff body: the old lines removed, the
/// new lines added. The `diff` lexer colours each line by its leading marker,
/// reusing the inserted/deleted scope palette the stylesheet already defines.
fn unified_diff(old: &str, new: &str) -> String {
    let mut diff = String::new();
    for line in old.lines() {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in new.lines() {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

fn longest_backtick_run(source: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for byte in source.bytes() {
        if byte == b'`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

fn json(value: &Value) -> Markup {
    let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    html! { pre { code { (pretty) } } }
}

fn unknown(value: &Value) -> Markup {
    let label = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unrecognized");
    html! {
        details .block.block--unknown {
            summary { (label) }
            (json(value))
        }
    }
}

fn image(source: &ImageSource) -> Markup {
    let src = format!("data:{};base64,{}", source.media_type, source.data);
    html! { figure .block.block--image { img src=(src) alt="pasted image"; } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_counts_shorten_to_one_decimal_place_per_magnitude() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(847), "847");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_000), "1k");
        assert_eq!(compact(47_612), "47.6k");
        assert_eq!(compact(999_400), "999.4k");
        assert_eq!(compact(7_643_000), "7.6M");
    }

    #[test]
    fn a_tally_reads_context_in_then_tokens_out() {
        let usage = Usage {
            input_tokens: 3,
            output_tokens: 214,
            cache_creation_input_tokens: 32_400,
            cache_read_input_tokens: 15_200,
        };

        assert_eq!(tally(&usage), "↑ 47.6k ↓ 214");
        assert_eq!(
            breakdown(&usage),
            "3 input · 32.4k cache write · 15.2k cache read · 214 output"
        );
    }

    #[test]
    fn border_strip_is_stable_per_seed() {
        let seed = border_seed("3f9c-a17b-session", "left");
        assert_eq!(margin_strip(seed), margin_strip(seed));
    }

    #[test]
    fn the_two_borders_of_a_folio_differ() {
        let session = "3f9c-a17b-session";
        assert_ne!(
            margin_strip(border_seed(session, "left")),
            margin_strip(border_seed(session, "right"))
        );
    }

    fn is_drollery(cell: &BorderCell) -> bool {
        matches!(cell, BorderCell::Drollery { .. })
    }

    #[test]
    fn borders_are_mostly_vine_with_non_adjacent_drolleries() {
        // Seams stay vine so the strip tiles cleanly, drolleries never sit
        // adjacent, and vine dominates. Exercise many seeds for confidence.
        let mut total_drolleries = 0;
        for salt in [
            "one", "two", "three", "four", "five", "six", "seven", "eight",
        ] {
            let cells = border_cells(border_seed("a-session", salt));
            assert_eq!(cells.len(), STRIP_CELLS);
            assert!(matches!(cells[0], BorderCell::Vine));
            assert!(matches!(cells[STRIP_CELLS - 1], BorderCell::Vine));
            let drolleries = cells.iter().filter(|cell| is_drollery(cell)).count();
            assert!(
                drolleries < STRIP_CELLS / 2,
                "too many drolleries: {drolleries}"
            );
            let adjacent = cells
                .windows(2)
                .any(|pair| is_drollery(&pair[0]) && is_drollery(&pair[1]));
            assert!(!adjacent, "two drolleries sat adjacent for salt {salt:?}");
            total_drolleries += drolleries;
        }
        // A border of pure vine would defeat the point; the generator must seat
        // creatures.
        assert!(total_drolleries > 0, "generator produced no drolleries");
    }

    #[test]
    fn drolleries_face_both_ways_across_a_border() {
        // Each seated creature is mirrored or not at random, so a full strip
        // shows both facings rather than one consistent direction.
        let cells = border_cells(border_seed("flip-variety-session", "left"));
        let flips: Vec<bool> = cells
            .iter()
            .filter_map(|cell| match cell {
                BorderCell::Drollery { flip, .. } => Some(*flip),
                BorderCell::Vine => None,
            })
            .collect();
        assert!(flips.iter().any(|&flip| flip), "no creature was flipped");
        assert!(
            flips.iter().any(|&flip| !flip),
            "no creature kept its facing"
        );
    }

    #[test]
    fn a_border_cycles_through_the_whole_bestiary() {
        // The shuffled bag draws every creature before repeating any, so a
        // border with a full bag's worth of drolleries shows all of them, and a
        // shorter one still never repeats before the bag drains.
        let cells = border_cells(border_seed("variety-session", "left"));
        let used: std::collections::HashSet<&str> = cells
            .iter()
            .filter_map(|cell| match cell {
                BorderCell::Drollery { svg, .. } => Some(*svg),
                BorderCell::Vine => None,
            })
            .collect();
        let count = cells.iter().filter(|cell| is_drollery(cell)).count();
        let expected = count.min(DROLLERIES.len());
        assert_eq!(
            used.len(),
            expected,
            "creatures repeated before the bag drained"
        );
    }
}
