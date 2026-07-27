//! Turning a parsed folio into a self-contained HTML document.

use std::{cmp::Ordering, collections::BTreeMap, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use comrak::{
    Options, markdown_to_html_with_plugins, options::Plugins, plugins::syntect::SyntectAdapter,
};
use jiff::{Timestamp, Zoned, tz::TimeZone};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use rayon::prelude::*;
use serde_json::Value;

use crate::{
    gloss,
    tools::{self, Setting},
    transcript::{Block, Folio, GlossPanel, ImageSource, Known, Panel, PanelKind, Speech, Usage},
};

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

/// The `@font-face` blocks: the vendored woff2 files as data URIs, encoded by
/// `build.rs` so a render inlines a constant rather than base64'ing megabytes.
/// The fonts are inlined at all so a folio stays self-contained (see the
/// rendering invariants in CLAUDE.md); `just fonts` vendors them from upstream.
///
/// Two blocks, because the faces are almost the whole of a short folio: the cut
/// ones carry what a transcript sets and are a fifth the bytes, the whole ones
/// carry everything upstream shipped. [`Scribe::folio`] picks per folio.
const CUT_FACES: &str = include_str!(concat!(env!("OUT_DIR"), "/font-faces-cut.css"));
const WHOLE_FACES: &str = include_str!(concat!(env!("OUT_DIR"), "/font-faces-whole.css"));

include!(concat!(env!("OUT_DIR"), "/dropped.rs"));

/// How a folio reaches its reader, which decides whether it can gain a message
/// under them. `serve` re-reads the session and re-renders on every load, so a
/// served folio grows as the session is written; a written one is a fixed
/// artifact, and the file it was rendered from could be a year old.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Delivery {
    /// Written to a file or published as a gist: the reader holds a snapshot.
    #[default]
    Static,
    /// Served by `serve`, which re-renders the session on every page load.
    Served,
}

/// Which cut of the embedded faces a folio should carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fonts {
    /// The cut faces, unless the folio's own text reaches a character they
    /// dropped, in which case the whole ones. Never renders a character worse
    /// than upstream would, and is a fifth the bytes for a folio that stays
    /// inside what a transcript usually sets.
    #[default]
    Fitted,
    /// The whole faces, whatever this folio happens to set.
    Whole,
}

/// The characters in `text` that an embedded face carries whole but not cut,
/// tallied by how often each occurs.
///
/// This is the *regression* the cut introduced, not the faces' coverage: a
/// character no face ever carried (an emoji, a CJK ideograph) is absent, since
/// it falls back to the reader's own fonts either way and always did.
pub fn beyond_cut(text: &str) -> BTreeMap<char, usize> {
    let mut tally = BTreeMap::new();
    let bytes = text.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        // Nothing below U+0080 is ever dropped (the tests hold that), and a
        // folio is overwhelmingly ASCII: hundreds of kilobytes of base64 font
        // before a word of transcript. So skip to the next lead byte rather
        // than decoding every character. Everything skipped is single-byte, so
        // the index stays on a character boundary.
        let Some(offset) = bytes[index..].iter().position(|byte| *byte >= 0x80) else {
            break;
        };
        index += offset;
        let character = text[index..]
            .chars()
            .next()
            .expect("index lands on a character boundary");
        if dropped(character) {
            *tally.entry(character).or_insert(0) += 1;
        }
        index += character.len_utf8();
    }
    tally
}

fn dropped(character: char) -> bool {
    let codepoint = character as u32;
    DROPPED
        .binary_search_by(|(low, high)| {
            if codepoint < *low {
                Ordering::Greater
            } else if codepoint > *high {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// The generation metadata recorded in every folio's plaque.
pub struct Colophon {
    pub generated: Timestamp,
    pub tool: &'static str,
    pub version: &'static str,
    pub home: &'static str,
}

/// What a render cost: how long the scribe took, and how large the folio came
/// out. Neither can be known while the markup is being written, so the plaque
/// carries a placeholder for each and [`inscribe`] fills them in afterwards.
pub struct Labour {
    pub took: Duration,
    pub bytes: usize,
}

/// The placeholders the plaque carries until the render's own cost is known.
/// Comments, so an uninscribed folio still reads correctly, and so no transcript
/// can forge one: raw HTML in a transcript is escaped, and only the scribe's own
/// markup reaches the document unescaped.
const TOOK_MARK: &str = "<!--folio:took-->";
const SIZE_MARK: &str = "<!--folio:size-->";

/// The lights a folio can be read by, which are also the controls that choose
/// between them, and the turn of the ring that hands the choice back to the
/// reader's system. Inline SVG, so they take the folio's own pigments.
const SUN: &str = include_str!("luminary/sun.svg");
const CANDLE: &str = include_str!("luminary/candle.svg");
const SYSTEM: &str = include_str!("luminary/system.svg");

/// Fills a finished folio's own cost into its plaque.
///
/// The numbers displace the placeholders they replace, so the size a folio
/// states is the markup it was measured from rather than the document it ends
/// up as. The difference is a few dozen bytes against a folio of megabytes, and
/// the human-readable figure rounds it away.
pub fn inscribe(markup: String, labour: &Labour) -> String {
    markup
        .replace(TOOK_MARK, &elapsed(labour.took))
        .replace(SIZE_MARK, &size(labour.bytes))
}

/// A duration in the coarsest unit that still says something: whole
/// milliseconds under a second, then seconds to one decimal place.
pub fn elapsed(took: Duration) -> String {
    let seconds = took.as_secs_f64();
    if seconds < 1.0 {
        format!("{:.0} ms", seconds * 1_000.0)
    } else {
        format!("{seconds:.1} s")
    }
}

/// A byte count as a size a reader can hold in mind, in the decimal units file
/// sizes are quoted in.
pub fn size(bytes: usize) -> String {
    match bytes {
        ..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{:.0} kB", bytes as f64 / 1_000.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_000_000.0),
    }
}

/// Renders folios, carrying the decisions a render depends on: how markdown
/// becomes HTML, how code gets highlighted, which zone timestamps read in,
/// which cut of the embedded faces to carry, and how the folio will reach its
/// reader.
pub struct Scribe<'a> {
    options: Options<'a>,
    /// The same options with hard breaks, for text a program printed.
    printed: Options<'a>,
    plugins: Plugins<'a>,
    timezone: TimeZone,
    fonts: Fonts,
    delivery: Delivery,
}

impl<'a> Scribe<'a> {
    pub fn new(
        highlighter: &'a SyntectAdapter,
        timezone: TimeZone,
        fonts: Fonts,
        delivery: Delivery,
    ) -> Self {
        let mut options = Options::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.tasklist = true;
        options.extension.autolink = true;
        options.extension.footnotes = true;
        options.render.github_pre_lang = true;

        // The same reading, with the source's own line breaks kept. See
        // `markdown_printed`.
        let mut printed = options.clone();
        printed.render.hardbreaks = true;

        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(highlighter);

        Self {
            options,
            printed,
            plugins,
            timezone,
            fonts,
            delivery,
        }
    }

    pub(crate) fn markdown(&self, source: &str) -> Markup {
        PreEscaped(markdown_to_html_with_plugins(
            source,
            &self.options,
            &self.plugins,
        ))
    }

    /// Markdown for text a *program* printed rather than composed: a hook's
    /// output is the case, and it is genuinely between the two readings.
    ///
    /// Markdown folds a single newline into a space, which is right for prose
    /// someone wrapped by hand and wrong for a list of things a script emitted
    /// one to a line: `M  CLAUDE.md\nM  src/gloss.rs` came out as one run of
    /// filenames. Keeping the breaks costs a hand-wrapped paragraph its reflow,
    /// so it goes slightly ragged, and that is much the smaller loss. Setting
    /// such output as preformatted text instead is worse than either: it wraps
    /// twice, once where the source wrapped and again at the box, *and* throws
    /// away the headings and lists these mostly carry.
    pub(crate) fn markdown_printed(&self, source: &str) -> Markup {
        PreEscaped(markdown_to_html_with_plugins(
            source,
            &self.printed,
            &self.plugins,
        ))
    }

    fn zoned(&self, timestamp: Timestamp) -> Zoned {
        timestamp.to_zoned(self.timezone.clone())
    }

    /// Sets a folio, and reports which characters (if any) drove it onto the
    /// whole faces. An empty tally means the cut faces served it.
    pub fn folio(&self, folio: &Folio, colophon: &Colophon) -> (Markup, BTreeMap<char, usize>) {
        let title = format!("folio {}", folio.session_id());
        let panels = folio.panels();
        let source = folio.source.display().to_string();
        // A panel's cost is dominated by highlighting its tool bodies, where a
        // syntax's regexes compile the first time that language is met, so the
        // panels are set concurrently and several languages compile at once
        // rather than each waiting on the last. Collected in order, so the
        // folio reads as the session ran.
        //
        // Each panel is also weighed against the cut faces as it is set, which
        // rides along on the threads already running rather than costing a
        // second pass over the finished markup.
        let (rendered_panels, reaches): (Vec<Markup>, Vec<BTreeMap<char, usize>>) = panels
            .par_iter()
            .map(|panel| {
                let markup = self.panel(panel);
                let reach = beyond_cut(&markup.0);
                (markup, reach)
            })
            .unzip();

        // The panels are the transcript; the source path is the only other
        // place a folio sets text it didn't choose. The rest of the chrome is
        // this crate's own markup, which the tests hold inside the cut faces.
        let mut reached = beyond_cut(&source);
        for reach in reaches {
            for (character, count) in reach {
                *reached.entry(character).or_insert(0) += count;
            }
        }
        let faces = match self.fonts {
            Fonts::Whole => WHOLE_FACES,
            Fonts::Fitted if !reached.is_empty() => WHOLE_FACES,
            Fonts::Fitted => CUT_FACES,
        };

        let left_border = margin_strip(border_seed(folio.session_id(), "left"));
        let right_border = margin_strip(border_seed(folio.session_id(), "right"));
        let document = html! {
            (DOCTYPE)
            (PreEscaped(font_notice()))
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    title { (title) }
                    style {
                        (PreEscaped(faces))
                        (PreEscaped(include_str!("illumination.css")))
                    }
                    // The folio's own behaviour: theme, search, copy, the
                    // navigation dock, and the minimap. It sits in <head> so the
                    // stored theme applies before the body paints, avoiding a
                    // flash of the wrong scheme, which is also why it stays a
                    // classic script rather than a module (a module is deferred,
                    // and would paint first). The core is inlined ahead of the
                    // shell, in the same script, so the shell closes over it:
                    // two files because one is pure and testable without a
                    // browser, one script because a folio is one file.
                    script {
                        (PreEscaped(include_str!("illumination.core.js")))
                        (PreEscaped(include_str!("illumination.shell.js")))
                    }
                }
                // The session the folio was set from, so the app script can key
                // what it remembers about this folio to this folio: a fold's
                // own key is a turn number and a position within it, which name
                // a different marginalia in every session.
                body data-folio=(folio.session_id()) {
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
                                dt { "source" } dd { code { (source) } }
                                dt { "turns" } dd { (panels.len()) }
                                @if let Some(first) = panels.first() {
                                    dt { "opened" } dd { (self.stamp(first.timestamp())) }
                                }
                                // The session's flux: how big the conversation
                                // ever got, against all the output. The input
                                // is the largest single turn's rather than a
                                // sum, since every turn is sent the whole
                                // conversation.
                                @if let (Some(input), Some(output)) = (folio.largest_input(), folio.output()) {
                                    dt { "tokens" } dd title=(folio_flux(input, output)) { (tally(input, output)) }
                                }
                            }
                            // The render's own cost is stated here rather than
                            // among the facts above, which are the session's.
                            // Both figures arrive after the markup exists, as
                            // placeholders `inscribe` fills in.
                            p .plaque__colophon {
                                "Written by " a href=(colophon.home) { (colophon.tool) } " " (colophon.version)
                                " on " (self.stamp(colophon.generated)) ", taking "
                                (PreEscaped(TOOK_MARK)) " to set " (PreEscaped(SIZE_MARK)) "."
                            }
                            p .plaque__colophon {
                                "Set in Junicode, Fira Code, and UnifrakturCook, under the "
                                a href="https://openfontlicense.org" { "SIL Open Font License" } "."
                            }
                        }
                    }
                    // The reading rail: the key, and stacked under it the search,
                    // the dock, and the minimap, all of which answer to it.
                    // Standing them in one column is what says they are tied
                    // together, without a word of explanation.
                    div .rail {
                        // The folio's key leads the rail, because everything
                        // under it answers to it: which kinds are in play, and,
                        // since each chip carries its own kind's pigment, what
                        // every edge in the margin means. It is a control rather
                        // than a legend alone, and the *only* one of its sort:
                        // the search and the dock both read it, so a reader says
                        // once what they are looking through rather than once per
                        // panel that looks. A column per side of the exchange, in
                        // the order `PanelKind::EVERY` declares, so no list of
                        // kinds is restated in the markup.
                        div .key role="group" aria-label="kinds of message to show" {
                            @for kind in PanelKind::EVERY {
                                button .key__chip type="button"
                                    data-scope=(kind.label()) data-side=(kind.side().label())
                                    aria-pressed="true" { (kind.label()) }
                            }
                        }
                        // Highlights matches and steps through them, wired by
                        // the app script. Looks only at what the key leaves in.
                        // Two rows: the field takes a whole one, and the count
                        // and step arrows share the next. Sharing a single row
                        // left the field a fraction of the rail's width, which
                        // was both the reason the rail had to be wide and the
                        // reason the placeholder was cut off in it.
                        div .search role="search" {
                            input .search__input type="search" placeholder="search folio" aria-label="search folio";
                            div .search__bar {
                                span .search__count aria-live="polite" {}
                                button .search__nav type="button" data-search-nav="prev" aria-label="previous match" { "‹" }
                                button .search__nav type="button" data-search-nav="next" aria-label="next match" { "›" }
                            }
                        }
                    // A dock: jump between messages, leap to either end
                    // and follow new ones (tail -f), and fold every tool call open
                    // or shut. Wired by the app script. The nav grid is three
                    // columns of up/down arrows: the middle steps between every
                    // message, flanked by a column per side of the exchange, so
                    // the cool arrows seek what reached the model (the reader's
                    // own words, their commands, skills, and hooks) and the warm
                    // ones seek what it produced (its replies, reasoning, and
                    // tool calls). The dock therefore steps along the same axis
                    // the palette is pitched on and the search box is grouped by.
                    nav .dock aria-label="folio navigation" {
                        div .dock__nav {
                            button .dock__btn .dock__btn--entered type="button" data-nav="prev" data-side="entered" aria-label="previous message that reached the model" title="previous message that reached the model" { "▲" }
                            button .dock__btn type="button" data-nav="prev" aria-label="previous message" title="previous message" { "▲" }
                            button .dock__btn .dock__btn--model type="button" data-nav="prev" data-side="model" aria-label="previous message the model produced" title="previous message the model produced" { "▲" }
                            button .dock__btn .dock__btn--entered type="button" data-nav="next" data-side="entered" aria-label="next message that reached the model" title="next message that reached the model" { "▼" }
                            button .dock__btn type="button" data-nav="next" aria-label="next message" title="next message" { "▼" }
                            button .dock__btn .dock__btn--model type="button" data-nav="next" data-side="model" aria-label="next message the model produced" title="next message the model produced" { "▼" }
                        }
                        // Jump to the first or last message, and, where the
                        // session can still grow, a follow toggle that re-pins
                        // the newest message's start on every reload until the
                        // reader scrolls away. Only a served folio is re-read
                        // as the session is written, so only it offers the
                        // toggle: the control's presence is what tells the app
                        // script this folio can follow.
                        div .dock__leap {
                            button .dock__btn type="button" data-nav="top" aria-label="jump to top" title="jump to top" { "⤒" }
                            button .dock__btn type="button" data-nav="end" aria-label="jump to end" title="jump to end" { "⤓" }
                            @if self.delivery == Delivery::Served {
                                button .dock__btn .dock__btn--tail type="button" data-tail="toggle" aria-pressed="false" aria-label="follow new messages" title="follow new messages, like tail -f" { "⇊" }
                            }
                        }
                        div .dock__fold {
                            button .dock__btn .dock__btn--fold type="button" data-fold="expand" aria-label="expand all" title="expand all" { span .dock__chevron { "⌃" } span .dock__chevron { "⌄" } }
                            button .dock__btn .dock__btn--fold type="button" data-fold="collapse" aria-label="collapse all" title="collapse all" { span .dock__chevron { "⌄" } span .dock__chevron { "⌃" } }
                        }
                        }
                        // The folio seen edge-on, under the controls that steer
                        // it: a band per panel in that panel's own pigment,
                        // sized to the share of the document the panel takes,
                        // with the reader's own view of the leaf drawn over
                        // them. A drag along it scrubs the folio, landing on
                        // whichever panel the key leaves in play, so it steps
                        // through the same kinds the dock above it does, and the
                        // wheel zooms the map alone, so a stretch of a
                        // thousand-panel folio can be opened up and picked
                        // through without leaving the place being read.
                        //
                        // The bands are recovered from the panels themselves
                        // rather than written out here, since what a band states
                        // is the share of the document its panel takes, which
                        // only the browser knows and which changes every time a
                        // fold opens. The empty track is what tells the app
                        // script to draw them, exactly as the follow button's
                        // presence tells it a folio can be followed.
                        //
                        // Hidden from assistive tech: every band is a second way
                        // to a panel the dock already steps to and the panel's
                        // own number already links to, and a stop per panel in
                        // the tab order would bury both.
                        div .minimap aria-hidden="true" {
                            div .minimap__track title="drag to scrub, scroll to zoom" {
                                div .minimap__view {}
                            }
                        }
                    }
                    // Presentation controls, opposite the navigation dock: the
                    // lights the folio can be read by, which are also the
                    // controls that choose between them. There is no separate
                    // toggle to label, because the reader presses the light they
                    // want: the sun for day, the candle for after dark.
                    //
                    // Which of them is *lit* is the scheme's to say, not the
                    // press's: by day the sun burns and the candle stands
                    // smoking, after dark the moon hangs and the candle is lit.
                    // That is one set of `light-dark()` pigments (see the
                    // stylesheet), so a folio still reads either way with no
                    // second set of rules and nothing rendered for one scheme
                    // alone. What the press changes is which scheme is in force.
                    //
                    // Each carries its own radiance, the light it throws across
                    // the leaf, so the glow comes from whichever is burning.
                    // Drawn rather than lettered, so each needs the name it used
                    // to spell out: a figure says nothing to a reader who cannot
                    // see it.
                    div .controls {
                        div .luminaries role="group" aria-label="colour theme" {
                            button .luminary type="button" data-theme-choice="light"
                                aria-label="read by daylight" title="read by daylight" {
                                span .luminary__radiance .luminary__radiance--day {}
                                (PreEscaped(SUN))
                            }
                            button .luminary type="button" data-theme-choice="dark"
                                aria-label="read by candlelight" title="read by candlelight" {
                                span .luminary__radiance .luminary__radiance--night {}
                                (PreEscaped(CANDLE))
                            }
                            // Only of use once a light has been chosen, so the
                            // stylesheet shows it only then, keyed off the
                            // `data-theme` the choice sets on the document.
                            button .theme-reset type="button" data-theme-choice="system"
                                aria-pressed="true" aria-label="follow the system"
                                title="follow the system" { (PreEscaped(SYSTEM)) }
                        }
                    }
                    main .folio {
                        // The only text in the reading column this crate wrote
                        // rather than set from the session: a session file is
                        // not everything the model was told, and a reader who
                        // takes it for one misreads it.
                        //
                        // It carries no `.turn` class, which is what every
                        // part of the app script keys on, so the dock never
                        // steps to it, the minimap draws no band for it, the
                        // key cannot set it aside, and the search never counts
                        // it as a hit. It is the folio's own voice, not one of
                        // the session's panels.
                        aside .caveat {
                            span .caveat__lead { "Caveat lector." }
                            " A session file is not everything the model was told. The system "
                            "prompt, the tool descriptions, and the " code { "CLAUDE.md" } " and "
                            "rule files loaded when a session starts are sent with every request "
                            "but never recorded, so they can't appear here. What the harness "
                            em { "did" } " write down (hook output, a skill's instructions, a file "
                            "pulled into context mid-session) is here."
                        }
                        @for panel in &rendered_panels {
                            (panel)
                        }
                    }
                }
            }
        };
        (document, reached)
    }

    fn stamp(&self, timestamp: Timestamp) -> String {
        self.zoned(timestamp)
            .strftime("%Y-%m-%d %H:%M:%S %Z")
            .to_string()
    }

    fn panel(&self, panel: &Panel) -> Markup {
        match panel {
            Panel::Speech(speech) => self.speech(speech),
            Panel::Gloss(gloss) => self.gloss(gloss),
        }
    }

    fn speech(&self, panel: &Speech) -> Markup {
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
                data-side=(kind.side().label())
                data-turn=(panel.turn_number)
                data-sidechain[panel.is_sidechain] {
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
                        span .turn__usage title=(turn_flux(usage)) {
                            (tally(usage.uncached_input(), usage.output_tokens))
                        }
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

    /// A note the harness wrote into the session: a panel of its own, since it
    /// happened at a point in the conversation rather than inside anyone's
    /// turn, but with no speaker, no model, and no cost of its own to state.
    /// What it says is folded away behind its summary line, the way a tool
    /// call's subject is: it is context a reader reaches for, not prose they
    /// read through.
    fn gloss(&self, panel: &GlossPanel) -> Markup {
        let kind = PanelKind::Gloss(panel.gloss.kind);
        html! {
            article id={ "turn-" (panel.turn_number) }
                class="turn turn--gloss" data-kind=(kind.label())
                data-side=(kind.side().label())
                data-turn=(panel.turn_number)
                data-sidechain[panel.is_sidechain] {
                header .turn__meta {
                    span .turn__role { (kind.label()) }
                    time .turn__time datetime=(panel.timestamp.to_string()) { (self.stamp(panel.timestamp)) }
                    a .turn__index href={ "#turn-" (panel.turn_number) } { "#" (panel.turn_number) }
                }
                (marginalia(
                    "marginalia--gloss",
                    None,
                    gloss::setting(self, &panel.gloss),
                    Outcome::Fine,
                ))
            }
        }
    }

    pub(crate) fn block(&self, block: &Block, versal: bool) -> Markup {
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
            Known::ToolUse { name, input, .. } => self.tool_call(name, input),
            // A result sits in the panel holding the call it answers, so the box
            // above it already names the tool and its subject and the line has
            // no need to say either again. What it shows instead is the first
            // thing that came back, which is the one thing the call cannot show.
            Known::ToolResult {
                content,
                is_error,
                answers,
                ..
            } => marginalia(
                "marginalia--result",
                None,
                Setting::new()
                    .maybe_gist(tools::hint(answers.as_ref(), content, *is_error))
                    .body(tools::result(self, answers.as_ref(), content, *is_error)),
                if *is_error {
                    Outcome::Failed
                } else {
                    Outcome::Fine
                },
            ),
            Known::Image { source } => image(source),
        }
    }

    /// A tool call: its summary line says what the call is, and its fold holds
    /// the subject. A call the line already states in full (a read of a named
    /// file, a query) has no subject left to hold, so it is set as one flat line
    /// with nothing to open.
    fn tool_call(&self, name: &str, input: &Value) -> Markup {
        marginalia(
            "marginalia--use",
            Some(name),
            tools::call(self, name, input),
            Outcome::Fine,
        )
    }

    /// A fenced code block run through the markdown path so it picks up syntax
    /// highlighting. The fence is grown past the longest backtick run in the
    /// source, so a body that itself contains backticks can't break out of it.
    pub(crate) fn code_block(&self, lang: &str, code: &str) -> Markup {
        // A file's own trailing newline is a fact about the file, not a line of
        // it, so it isn't set as one: an empty line before the fold's bottom
        // edge reads as content that isn't there.
        let code = code.trim_end();
        let fence = "`".repeat(longest_backtick_run(code).max(2) + 1);
        self.markdown(&format!("{fence}{lang}\n{code}\n{fence}"))
    }
}

/// Whether a fold reports a failure, which the stylesheet marks and the summary
/// line names.
///
/// Success is deliberately unmarked. A result sits in the panel holding the call
/// it answers, so the box below the call is already what says it answered, and a
/// word saying so on every one of them is the noise a reader reads past. A
/// failure is the exception, so it is the one that gets named; marking only the
/// exception is what makes the mark worth anything.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Fine,
    Failed,
}

impl Outcome {
    fn word(self) -> Option<&'static str> {
        match self {
            Outcome::Fine => None,
            Outcome::Failed => Some("error"),
        }
    }

    fn failed(self) -> bool {
        self == Outcome::Failed
    }
}

/// A summary line and the fold under it: the shape a tool call and a harness
/// note share. The line carries the labelling (what is being shown, a gist of
/// its subject, and any qualifier), and the fold carries the subject itself. A
/// setting whose line already states the whole of it has nothing left to hold,
/// so it is set as one flat line with nothing to open.
///
/// The subject belongs in the setting's *gist* and never in a note: the gist is
/// the one part of the line that can shrink, and a note holding a path drives
/// the line out of the fold entirely.
fn marginalia(variant: &str, label: Option<&str>, setting: Setting, outcome: Outcome) -> Markup {
    let head = html! {
        @if let Some(word) = outcome.word() {
            span .marginalia__outcome { (word) }
        }
        @if let Some(label) = label {
            span .marginalia__tool { (label) }
        }
        @if let Some(gist) = &setting.gist {
            @match &setting.href {
                Some(href) => a .marginalia__gist href=(href) { (gist) },
                None => span .marginalia__gist { (gist) },
            }
        }
        @for note in &setting.notes {
            span .marginalia__note { (note) }
        }
    };
    let failed = outcome.failed();
    match &setting.body {
        Some(body) => html! {
            details class={ "marginalia " (variant) } data-error[failed] {
                summary .marginalia__head { (head) }
                (body)
            }
        },
        None => html! {
            div class={ "marginalia " (variant) " marginalia--flat" } data-error[failed] {
                div .marginalia__head { (head) }
            }
        },
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

/// Token flux at a glance: what went in, then what came out.
fn tally(input: u64, output: u64) -> String {
    format!("↑ {} ↓ {}", compact(input), compact(output))
}

/// A turn's figures unrounded, for the title a reader hovers. Its input is what
/// the turn added, so it stands beside the turn's own output rather than
/// restating the whole conversation the request re-sent.
fn turn_flux(usage: &Usage) -> String {
    format!(
        "{} input this turn · {} output this turn",
        separated(usage.uncached_input()),
        separated(usage.output_tokens),
    )
}

/// The folio's figures unrounded. Its input is the largest single turn's, not a
/// sum, so the title says which.
fn folio_flux(largest_input: u64, output: u64) -> String {
    format!(
        "{} input at its largest · {} output in all",
        separated(largest_input),
        separated(output),
    )
}

/// A count grouped in thousands, so an exact figure stays readable.
fn separated(tokens: u64) -> String {
    let digits = tokens.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// A token count short enough to sit in a line of chrome: exact below a
/// thousand, then one decimal place per magnitude, with a bare `.0` trimmed.
fn compact(tokens: u64) -> String {
    // A magnitude ends where rounding would carry into the next one: 999,950
    // would otherwise print as `1000k`, which reads as having crossed the
    // boundary without switching suffix.
    let (scaled, suffix) = match tokens {
        ..1_000 => return tokens.to_string(),
        1_000..999_950 => (tokens as f64 / 1_000.0, "k"),
        _ => (tokens as f64 / 1_000_000.0, "M"),
    };
    let rounded = format!("{scaled:.1}");
    format!("{}{suffix}", rounded.strip_suffix(".0").unwrap_or(&rounded))
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

pub(crate) fn json(value: &Value) -> Markup {
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
    fn a_count_that_rounds_up_a_magnitude_takes_the_next_suffix() {
        assert_eq!(compact(999_949), "999.9k");
        assert_eq!(compact(999_950), "1M");
        assert_eq!(compact(1_000_000), "1M");
    }

    #[test]
    fn a_turn_counts_only_the_input_it_added() {
        // The cached prefix is the conversation the request re-sent, so it is
        // no part of what this turn contributed.
        let usage = Usage {
            input_tokens: 3,
            output_tokens: 214,
            cache_creation_input_tokens: 32_400,
            cache_read_input_tokens: 15_200,
        };

        assert_eq!(
            tally(usage.uncached_input(), usage.output_tokens),
            "↑ 32.4k ↓ 214"
        );
        assert_eq!(
            turn_flux(&usage),
            "32,403 input this turn · 214 output this turn"
        );
    }

    #[test]
    fn a_folio_names_its_input_as_the_largest_rather_than_a_sum() {
        assert_eq!(
            folio_flux(60_867, 48_923),
            "60,867 input at its largest · 48,923 output in all"
        );
    }

    #[test]
    fn exact_counts_group_in_thousands() {
        assert_eq!(separated(7), "7");
        assert_eq!(separated(942), "942");
        assert_eq!(separated(1_206), "1,206");
        assert_eq!(separated(47_603), "47,603");
        assert_eq!(separated(7_643_812), "7,643,812");
    }

    #[test]
    fn a_render_reads_in_milliseconds_until_it_takes_a_second() {
        assert_eq!(elapsed(Duration::from_micros(412_400)), "412 ms");
        assert_eq!(elapsed(Duration::from_millis(999)), "999 ms");
        assert_eq!(elapsed(Duration::from_millis(1_000)), "1.0 s");
        assert_eq!(elapsed(Duration::from_millis(3_260)), "3.3 s");
    }

    #[test]
    fn a_folio_is_sized_in_the_units_files_are_quoted_in() {
        assert_eq!(size(742), "742 B");
        assert_eq!(size(6_140), "6 kB");
        assert_eq!(size(812_600), "813 kB");
        assert_eq!(size(2_947_312), "2.9 MB");
    }

    #[test]
    fn a_folio_states_the_render_it_came_out_of() {
        let markup = format!("<p>taking {TOOK_MARK} to set {SIZE_MARK}.</p>");
        let labour = Labour {
            took: Duration::from_millis(412),
            bytes: 2_947_312,
        };

        assert_eq!(
            inscribe(markup, &labour),
            "<p>taking 412 ms to set 2.9 MB.</p>"
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
