//! Turning a parsed folio into a self-contained HTML document.

use comrak::{
    Options, markdown_to_html_with_plugins, options::Plugins, plugins::syntect::SyntectAdapter,
};
use jiff::{Timestamp, Zoned, tz::TimeZone};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use serde_json::Value;

use crate::transcript::{Block, Folio, ImageSource, Known, Panel, ToolResultContent};

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
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    title { (title) }
                    style { (PreEscaped(include_str!("illumination.css"))) }
                }
                body {
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
                    (json(input))
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

    fn colophon(&self, colophon: &Colophon) -> Markup {
        html! {
            footer .colophon {
                p {
                    "Written by " (colophon.tool) " " (colophon.version)
                    " on " (self.stamp(colophon.generated)) "."
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
