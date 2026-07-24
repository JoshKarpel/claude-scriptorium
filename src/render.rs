//! Turning a parsed folio into a self-contained HTML document.

use std::{path::Path, sync::LazyLock};

use base64::{Engine, engine::general_purpose::STANDARD};
use comrak::{
    Options, markdown_to_html_with_plugins, options::Plugins, plugins::syntect::SyntectAdapter,
};
use jiff::{Timestamp, Zoned, tz::TimeZone};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use serde_json::Value;

use crate::transcript::{Block, Folio, ImageSource, Known, Panel, ToolResultContent};

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
/// single-weight blackletter used only for headings.
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

/// The generation metadata stamped at the foot of every folio.
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
                    // A fixed search widget: highlights matches and steps
                    // through them, wired by the app script.
                    div .search role="search" {
                        input .search__input type="search" placeholder="search folio" aria-label="search folio";
                        span .search__count aria-live="polite" {}
                        button .search__nav type="button" data-search-nav="prev" aria-label="previous match" { "‹" }
                        button .search__nav type="button" data-search-nav="next" aria-label="next match" { "›" }
                    }
                    // A fixed dock: jump between turns and fold every tool call
                    // open or shut. Wired by the app script.
                    nav .dock aria-label="folio navigation" {
                        button .dock__btn type="button" data-nav="prev" aria-label="previous turn" title="previous turn" { "▲" }
                        button .dock__btn type="button" data-nav="next" aria-label="next turn" title="next turn" { "▼" }
                        button .dock__btn type="button" data-fold="expand" aria-label="expand all" title="expand all" { "⊞" }
                        button .dock__btn type="button" data-fold="collapse" aria-label="collapse all" title="collapse all" { "⊟" }
                    }
                    header .folio__head {
                        h1 { (title) }
                        dl .folio__facts {
                            dt { "source" } dd { code { (folio.source.display().to_string()) } }
                            dt { "turns" } dd { (panels.len()) }
                            @if let Some(first) = panels.first() {
                                dt { "opened" } dd { (self.stamp(first.timestamp)) }
                            }
                        }
                        // Harness-injected panels are hidden by default; the
                        // checkbox drives a pure-CSS reveal, so only offer it
                        // when the folio actually carries any.
                        @if panels.iter().any(|panel| panel.is_meta) {
                            p .meta-toggle {
                                input #show-meta type="checkbox";
                                label for="show-meta" { "show harness notes" }
                            }
                        }
                        // Light / dark / system, wired by the app script and
                        // defaulting to the reader's system preference.
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
                    (self.colophon(colophon))
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
        html! {
            article class={ "turn turn--" (panel.role.as_str()) } data-kind=(kind.label())
                data-sidechain[panel.is_sidechain] data-meta[panel.is_meta] {
                header .turn__meta {
                    span .turn__role { (kind.label()) }
                    @if let Some(model) = &panel.model {
                        span .turn__model { (model) }
                    }
                    time .turn__time datetime=(panel.timestamp.to_string()) { (self.stamp(panel.timestamp)) }
                    span .turn__index { "#" (panel.turn_number) }
                }
                @for block in &panel.blocks {
                    (self.block(block))
                }
            }
        }
    }

    fn block(&self, block: &Block) -> Markup {
        let known = match block {
            Block::Known(known) => known,
            Block::Unknown(value) => return unknown(value),
        };
        match known {
            Known::Text { text } => html! { div .block.block--text { (self.markdown(text)) } },
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
                    }
                    (self.tool_body(name, input))
                }
            },
            Known::ToolResult { content, is_error } => html! {
                details .marginalia.marginalia--result data-error[*is_error] {
                    summary { @if *is_error { "error" } @else { "result" } }
                    @match content {
                        ToolResultContent::Text(text) => pre .marginalia__body { code { (text) } },
                        ToolResultContent::Blocks(blocks) => @for block in blocks { (self.block(block)) },
                    }
                }
            },
            Known::Image { source } => image(source),
        }
    }

    /// The body of a tool call. A handful of common tools get a bespoke view;
    /// everything else (and any call whose input doesn't match the shape this
    /// view expects) falls back to pretty-printed JSON, the same treatment an
    /// unrecognized block gets.
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
        let description = input.get("description").and_then(Value::as_str);
        Some(html! {
            div .tool.tool--bash {
                @if let Some(description) = description {
                    p .tool__caption { (description) }
                }
                (self.code_block("bash", command))
            }
        })
    }

    fn write_body(&self, input: &Value) -> Option<Markup> {
        let path = input.get("file_path")?.as_str()?;
        let content = input.get("content")?.as_str()?;
        Some(html! {
            div .tool.tool--write {
                (self.code_block(lang_for_path(path), content))
            }
        })
    }

    fn edit_body(&self, input: &Value) -> Option<Markup> {
        let old = input.get("old_string")?.as_str()?;
        let new = input.get("new_string")?.as_str()?;
        let replace_all = input
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Some(html! {
            div .tool.tool--edit {
                @if replace_all {
                    p .tool__caption { "replace all occurrences" }
                }
                (self.code_block("diff", &unified_diff(old, new)))
            }
        })
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

    fn colophon(&self, colophon: &Colophon) -> Markup {
        html! {
            footer .colophon {
                p {
                    "Written by " (colophon.tool) " " (colophon.version)
                    " on " (self.stamp(colophon.generated)) "."
                }
                p .colophon__fonts {
                    "Set in Junicode, Fira Code, and UnifrakturCook, under the "
                    a href="https://openfontlicense.org" { "SIL Open Font License" } "."
                }
            }
        }
    }
}

/// A one-line summary of a tool call, drawn from whichever field carries the
/// subject of the call.
fn gist(input: &Value) -> Option<&str> {
    ["command", "file_path", "pattern", "path", "url", "prompt"]
        .iter()
        .find_map(|field| input.get(field)?.as_str())
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
    html! { pre .marginalia__body { code { (pretty) } } }
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
