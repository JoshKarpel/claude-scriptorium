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

  const markHits = (container, query) => {
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
      if (node.matches("[data-meta]")) {
        const reveal = document.getElementById("show-meta");
        if (reveal) reveal.checked = true;
      }
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
      hits = query ? markHits(container, query) : [];
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

  // --- Dock: jump between turns, fold every marginalia -------------------

  const wireDock = () => {
    const dock = document.querySelector(".dock");
    const container = document.querySelector("main.folio");
    if (!dock || !container) return;
    // Only visible turns: a hidden meta panel (display:none) reports top 0 and
    // would otherwise hijack the "current turn" search.
    const turns = () =>
      Array.from(container.querySelectorAll(".turn")).filter(
        (turn) => turn.getClientRects().length > 0,
      );

    const jump = (direction) => {
      // The turn at the top of the viewport is the last one whose top has
      // scrolled to or above the threshold; the threshold clears a turn's own
      // scroll-margin so the one just navigated to counts as current, not next.
      const threshold = 40;
      const panels = turns();
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
      const { nav, fold: foldTo } = button.dataset;
      if (nav === "prev") jump(-1);
      else if (nav === "next") jump(1);
      else if (foldTo === "expand") fold(true);
      else if (foldTo === "collapse") fold(false);
    });
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
