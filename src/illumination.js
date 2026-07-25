// illumination: the folio's own behaviour, inlined into every written file.
//
// This is the trusted app script (distinct from the dev-only live-reload
// snippet `serve` injects). Transcript content is always escaped, never run;
// this code is the one script the artifact carries deliberately. Keep it small
// and dependency-free so a folio stays a single portable file.
(() => {
  "use strict";

  // --- Theme: light / dark / system -------------------------------------
  //
  // Every colour is a light-dark() pair resolved by `color-scheme`. Forcing a
  // side is just pinning color-scheme via [data-theme] on <html>; "system"
  // clears the attribute and falls back to the OS preference. The choice
  // persists in localStorage and is applied before first paint (this script
  // sits in <head>) so a forced theme never flashes the system one.

  const THEME_KEY = "scriptorium-theme";
  const THEMES = ["system", "light", "dark"];
  const FOLD_KEY = "scriptorium-folds";

  const readTheme = () => {
    try {
      const stored = localStorage.getItem(THEME_KEY);
      return THEMES.includes(stored) ? stored : "system";
    } catch {
      return "system";
    }
  };

  const applyTheme = (theme) => {
    const root = document.documentElement;
    if (theme === "system") {
      delete root.dataset.theme;
    } else {
      root.dataset.theme = theme;
    }
  };

  applyTheme(readTheme());

  const wireThemeToggle = () => {
    const toggle = document.querySelector(".theme-toggle");
    if (!toggle) return;
    const buttons = toggle.querySelectorAll("[data-theme-choice]");
    const sync = (theme) => {
      buttons.forEach((button) => {
        const active = button.dataset.themeChoice === theme;
        button.setAttribute("aria-pressed", String(active));
      });
    };
    sync(readTheme());
    buttons.forEach((button) => {
      button.addEventListener("click", () => {
        const theme = button.dataset.themeChoice;
        applyTheme(theme);
        try {
          localStorage.setItem(THEME_KEY, theme);
        } catch {}
        sync(theme);
      });
    });
  };

  // --- Search: highlight every match, step through with next / prev -------
  //
  // Non-destructive to the layout: nothing is hidden. Each query marks all
  // matches inside the conversation, keeps a running index, and scrolls the
  // current one into view, opening any collapsed marginalia (and revealing the
  // meta panels) that hold it so the match is actually visible.

  const HIT = "search__hit";
  const CURRENT = "is-current";

  const clearHits = (container) => {
    container.querySelectorAll("mark." + HIT).forEach((mark) => {
      mark.replaceWith(document.createTextNode(mark.textContent));
    });
    container.normalize();
  };

  // Which kind of message a text node belongs to, judged by the block it sits
  // in rather than its panel's label: a tool call folded into an assistant
  // panel is still "tool", and reasoning is "thinking", so scoping is precise.
  const scopeOf = (node) => {
    const el = node.parentElement;
    if (!el) return "assistant";
    if (el.closest(".marginalia")) return "tool";
    if (el.closest(".block--thinking")) return "thinking";
    const turn = el.closest(".turn");
    if (turn && turn.classList.contains("turn--user")) return "user";
    return "assistant";
  };

  const markHits = (container, query, scopes) => {
    const needle = query.toLowerCase();
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    const nodes = [];
    while (walker.nextNode()) {
      const node = walker.currentNode;
      if (!node.nodeValue.trim()) continue;
      // Copy-button labels are chrome, not transcript; don't match them.
      if (node.parentElement && node.parentElement.closest(".copy-button")) {
        continue;
      }
      // Harness-note panels are hidden with no way to reveal them, so a hit
      // inside one could never be scrolled into view; skip them.
      if (node.parentElement && node.parentElement.closest("[data-meta]")) {
        continue;
      }
      // Restrict to the kinds of message the reader left enabled.
      if (scopes && !scopes.has(scopeOf(node))) {
        continue;
      }
      nodes.push(node);
    }
    const hits = [];
    for (const node of nodes) {
      const text = node.nodeValue;
      const hay = text.toLowerCase();
      let from = hay.indexOf(needle);
      if (from === -1) continue;
      const frag = document.createDocumentFragment();
      let pos = 0;
      while (from !== -1) {
        if (from > pos) {
          frag.appendChild(document.createTextNode(text.slice(pos, from)));
        }
        const mark = document.createElement("mark");
        mark.className = HIT;
        mark.textContent = text.slice(from, from + query.length);
        frag.appendChild(mark);
        hits.push(mark);
        pos = from + query.length;
        from = hay.indexOf(needle, pos);
      }
      if (pos < text.length) {
        frag.appendChild(document.createTextNode(text.slice(pos)));
      }
      node.parentNode.replaceChild(frag, node);
    }
    return hits;
  };

  const revealHit = (hit) => {
    for (let node = hit.parentElement; node; node = node.parentElement) {
      if (node.tagName === "DETAILS") node.open = true;
    }
  };

  const wireSearch = () => {
    const search = document.querySelector(".search");
    const container = document.querySelector("main.folio");
    if (!search || !container) return;
    const input = search.querySelector(".search__input");
    const count = search.querySelector(".search__count");
    const prev = search.querySelector('[data-search-nav="prev"]');
    const next = search.querySelector('[data-search-nav="next"]');
    const scopeButtons = search.querySelectorAll(".search__scope");

    const enabledScopes = () => {
      const scopes = new Set();
      scopeButtons.forEach((button) => {
        if (button.getAttribute("aria-pressed") === "true") {
          scopes.add(button.dataset.scope);
        }
      });
      return scopes;
    };

    let hits = [];
    let index = -1;

    const paint = () => {
      hits.forEach((hit) => hit.classList.remove(CURRENT));
      const nav = hits.length > 0;
      prev.disabled = !nav;
      next.disabled = !nav;
      if (!input.value) {
        count.textContent = "";
        return;
      }
      if (!nav) {
        count.textContent = "no matches";
        return;
      }
      const hit = hits[index];
      hit.classList.add(CURRENT);
      revealHit(hit);
      hit.scrollIntoView({ block: "center", behavior: "smooth" });
      count.textContent = index + 1 + "/" + hits.length;
    };

    const run = () => {
      clearHits(container);
      const query = input.value;
      hits = query ? markHits(container, query, enabledScopes()) : [];
      index = hits.length ? 0 : -1;
      paint();
    };

    const step = (delta) => {
      if (!hits.length) return;
      index = (index + delta + hits.length) % hits.length;
      paint();
    };

    input.addEventListener("input", run);
    input.addEventListener("keydown", (event) => {
      if (event.key !== "Enter") return;
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    });
    prev.addEventListener("click", () => step(-1));
    next.addEventListener("click", () => step(1));
    scopeButtons.forEach((button) => {
      button.addEventListener("click", () => {
        const active = button.getAttribute("aria-pressed") === "true";
        button.setAttribute("aria-pressed", String(!active));
        run();
      });
    });
  };

  // --- Copy: a button on every code block and every message --------------

  const copyToClipboard = async (text) => {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // file:// or a denied permission: fall back to a throwaway textarea.
    }
    const scratch = document.createElement("textarea");
    scratch.value = text;
    scratch.style.position = "fixed";
    scratch.style.opacity = "0";
    document.body.appendChild(scratch);
    scratch.select();
    try {
      document.execCommand("copy");
    } catch {}
    scratch.remove();
  };

  const makeCopyButton = (getText) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "copy-button";
    button.textContent = "copy";
    button.setAttribute("aria-label", "copy to clipboard");
    button.addEventListener("click", async (event) => {
      event.stopPropagation();
      await copyToClipboard(getText());
      button.textContent = "copied";
      button.classList.add("is-done");
      setTimeout(() => {
        button.textContent = "copy";
        button.classList.remove("is-done");
      }, 1200);
    });
    return button;
  };

  const wireCopy = () => {
    const container = document.querySelector("main.folio");
    if (!container) return;

    // Every code / diff / JSON / output block copies its own text.
    container.querySelectorAll("pre").forEach((pre) => {
      const code = pre.querySelector("code") || pre;
      pre.appendChild(makeCopyButton(() => code.textContent));
    });

    // Every turn copies its readable prose (text and thinking, not tool JSON).
    container.querySelectorAll(".turn").forEach((turn) => {
      const prose = turn.querySelectorAll(".block--text, .block--thinking");
      if (!prose.length) return;
      const meta = turn.querySelector(".turn__meta");
      if (!meta) return;
      const button = makeCopyButton(() =>
        Array.from(prose)
          .map((block) => block.textContent.trim())
          .filter(Boolean)
          .join("\n\n"),
      );
      button.classList.add("copy-button--meta");
      meta.appendChild(button);
    });
  };

  // --- Dock: jump between messages, fold every marginalia ---------------

  const wireDock = () => {
    const dock = document.querySelector(".dock");
    const container = document.querySelector("main.folio");
    if (!dock || !container) return;
    // Step between the substantive messages, skipping tool-call and thinking
    // panels: those are the noise a reader wants to jump over. Only visible
    // ones, since a hidden meta panel reports top 0 and would hijack "current".
    // Scoped to one speaker when a role is given, otherwise every message.
    const messages = (role) =>
      Array.from(
        container.querySelectorAll(
          role
            ? `.turn[data-kind="${role}"]`
            : '.turn[data-kind="user"], .turn[data-kind="assistant"]',
        ),
      ).filter((turn) => turn.getClientRects().length > 0);

    const jump = (direction, role) => {
      // The message at the top of the viewport is the last one whose top has
      // scrolled to or above the threshold; the threshold clears a turn's own
      // scroll-margin so the one just navigated to counts as current, not next.
      const threshold = 40;
      const panels = messages(role);
      let current = -1;
      panels.forEach((turn, index) => {
        if (turn.getBoundingClientRect().top <= threshold) current = index;
      });
      const next = Math.min(Math.max(current + direction, 0), panels.length - 1);
      const target = panels[next];
      if (target) target.scrollIntoView({ behavior: "smooth", block: "start" });
    };

    const fold = (open) => {
      container.querySelectorAll("details").forEach((details) => {
        details.open = open;
      });
    };

    dock.addEventListener("click", (event) => {
      const button = event.target.closest("button");
      if (!button) return;
      const { nav, role, fold: foldTo } = button.dataset;
      if (nav === "prev") jump(-1, role);
      else if (nav === "next") jump(1, role);
      else if (foldTo === "expand") fold(true);
      else if (foldTo === "collapse") fold(false);
    });

    // Remember each marginalia's open/closed state across reloads, keyed per
    // message so a live session that grows keeps the folds a reader set: the
    // raw stream is append-only, so a panel's turn number and a marginalia's
    // index within it stay stable as new turns arrive. Only open keys are
    // stored; the markup default is collapsed.
    const foldKey = (details) => {
      const turn = details.closest(".turn");
      const marginalia = turn ? turn.querySelectorAll("details") : [details];
      const index = Array.prototype.indexOf.call(marginalia, details);
      return `${turn ? turn.dataset.turn : "?"}:${index}`;
    };

    const readOpenFolds = () => {
      try {
        const stored = JSON.parse(localStorage.getItem(FOLD_KEY) || "[]");
        return new Set(Array.isArray(stored) ? stored : []);
      } catch {
        return new Set();
      }
    };

    const open = readOpenFolds();
    container.querySelectorAll("details").forEach((details) => {
      if (open.has(foldKey(details))) details.open = true;
    });

    // `toggle` fires on both a reader's click and the fold-all buttons, and does
    // not bubble, so listen in the capture phase.
    container.addEventListener(
      "toggle",
      (event) => {
        const details = event.target;
        if (!(details instanceof HTMLDetailsElement)) return;
        const folds = readOpenFolds();
        if (details.open) folds.add(foldKey(details));
        else folds.delete(foldKey(details));
        try {
          localStorage.setItem(FOLD_KEY, JSON.stringify([...folds]));
        } catch {}
      },
      true,
    );
  };

  const onReady = (fn) => {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", fn);
    } else {
      fn();
    }
  };

  onReady(() => {
    wireThemeToggle();
    wireSearch();
    wireCopy();
    wireDock();
  });
})();
