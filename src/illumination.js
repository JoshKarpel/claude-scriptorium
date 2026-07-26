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

  // The theme above is the reader's, and holds across everything they open.
  // What follows is about one folio and is stored under the session the markup
  // names: which marginalia stand open, and whether the reader is following the
  // end of the session. A fold's own key is a turn number and a position within
  // that turn, which names a different marginalia in every session, and
  // following a session still being written says nothing about a folio finished
  // months ago. Every folio a reader opens from disk shares the `file://`
  // origin, as does every folio served through one viewer, so an unscoped store
  // is one folio's state imposed on all of them.
  const FOLDS = "scriptorium-folds";
  const TAIL = "scriptorium-tail";
  const perFolio = (store) =>
    store + ":" + (document.body.dataset.folio || "?");

  // Keys whose default action scrolls the page, so pressing one counts as the
  // reader taking over from follow mode (unless focus is in a control).
  const SCROLL_KEYS = new Set([
    "ArrowUp",
    "ArrowDown",
    "PageUp",
    "PageDown",
    "Home",
    "End",
    " ",
  ]);

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

  // --- The nib: a quill's scratch as a copy is taken ---------------------
  //
  // Synthesized rather than sampled, for the same reason everything else here
  // is inlined: a folio carries every byte it needs, and a recording of a pen
  // would cost tens of kilobytes where this costs a few lines. A nib doesn't
  // glide over parchment, it catches and releases dozens of times a stroke,
  // and that stick-slip is what an ear hears as a scratch rather than a hiss,
  // so the noise is cut into grains that each bite their own amount, and then
  // trimmed to the band a dry point on paper actually sounds in. What is
  // written is a word rather than a mark: strokes of their own length, weight,
  // and tone, scheduled one after another with the pen lifted between them.

  let quill = null;

  // Built on the first copy rather than at load: a context made with no
  // gesture behind it starts suspended, and browsers count it against the page.
  const nib = () => {
    if (quill) return quill;
    const Context = window.AudioContext || window.webkitAudioContext;
    if (!Context) return null;
    try {
      quill = new Context();
    } catch {
      return null;
    }
    return quill;
  };

  // One pull of the nib: how long it is down, how hard it is pressed, how
  // dark it sounds, how coarsely it catches, and how long the pen is off the
  // page after. A word is a handful of these, and no two of them match: a
  // stem is a flick where a bowl is a long pull, the hand leans in and eases
  // off, and the nib's angle changes what each one sounds like. Skewing the
  // length keeps most strokes short, so the occasional long one tells.
  const strokesOfAWord = () => {
    const strokes = [];
    const letters = 5 + Math.floor(Math.random() * 4);
    for (let n = 0; n < letters; n += 1) {
      strokes.push({
        down: 0.018 + Math.random() ** 1.6 * 0.1,
        // No lift after the last: the word ends when the pen does.
        lifted: n === letters - 1 ? 0 : 0.015 + Math.random() * 0.05,
        press: 0.35 + Math.random() * 0.65,
        floor: 600 + Math.random() * 800,
        ceiling: 2400 + Math.random() * 2400,
        catches: 0.0004 + Math.random() * 0.0035,
      });
    }
    return strokes;
  };

  // One stroke, sounded at `when`: grains of noise under the pressure of a
  // hand, cut at either end to the band this pull sounds in. Below is a rumble
  // the pen doesn't have; above is the sizzle that makes the same grains read
  // as static rather than as a nib.
  const drawStroke = (pen, stroke, when) => {
    const length = Math.floor(pen.sampleRate * stroke.down);
    const buffer = pen.createBuffer(1, length, pen.sampleRate);
    const samples = buffer.getChannelData(0);
    let caught = 0;
    let bite = 0;
    for (let i = 0; i < length; i += 1) {
      if (i >= caught) {
        caught = i + pen.sampleRate * stroke.catches * (0.5 + Math.random());
        bite = 0.25 + Math.random() * 0.75;
      }
      // The stroke's own pressure: on as the hand commits, off as it lifts, so
      // every stroke ends at silence and the gap after it is a clean one.
      const weight = Math.sin((Math.PI * i) / length) ** 0.7;
      samples[i] = (Math.random() * 2 - 1) * bite * weight * stroke.press;
    }
    const source = pen.createBufferSource();
    source.buffer = buffer;
    const rumble = pen.createBiquadFilter();
    rumble.type = "highpass";
    rumble.frequency.value = stroke.floor;
    const sizzle = pen.createBiquadFilter();
    sizzle.type = "lowpass";
    sizzle.frequency.value = stroke.ceiling;
    sizzle.Q.value = 0.9;
    const quiet = pen.createGain();
    // Most strokes are pressed well under full, so the word wants a little
    // more gain than one flat stroke would to land as loudly.
    quiet.gain.value = 0.07;
    source
      .connect(rumble)
      .connect(sizzle)
      .connect(quiet)
      .connect(pen.destination);
    source.start(when);
  };

  const scratch = () => {
    const pen = nib();
    if (!pen) return;
    if (pen.state === "suspended") pen.resume();
    // A lead on the first stroke, so scheduling the word can't run late into
    // its own opening.
    let when = pen.currentTime + 0.01;
    for (const stroke of strokesOfAWord()) {
      drawStroke(pen, stroke, when);
      when += stroke.down + stroke.lifted;
    }
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
    // A hover is the reader announcing the click a moment early, so the pen is
    // readied then: building the context is the one costly step, and doing it
    // here leaves the click with only the stroke to draw. It starts suspended,
    // since a hover carries no user gesture to unblock audio, and the click
    // that follows resumes it. Once, because the context outlives the hover.
    button.addEventListener("pointerenter", nib, { once: true });
    button.addEventListener("click", async (event) => {
      event.stopPropagation();
      // The copy goes first and the pen follows in the same click. The hover
      // above usually spares this click the context, but a reader who reached
      // the button by keyboard pays tens of milliseconds to build one here, and
      // the clipboard is what they pressed the button for.
      const copied = copyToClipboard(getText());
      scratch();
      await copied;
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
    // The unscoped stores these replaced imposed one folio's state on every
    // other; drop them rather than leave them to sit unread forever.
    try {
      localStorage.removeItem(FOLDS);
      localStorage.removeItem(TAIL);
    } catch {}
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

    // --- Follow (tail -f): keep the newest message's start pinned across the
    // reloads a live session drives, until the reader scrolls away.
    const tailButton = dock.querySelector('[data-tail="toggle"]');

    const visible = () =>
      Array.from(container.querySelectorAll(".turn")).filter(
        (turn) => turn.getClientRects().length > 0,
      );

    const firstMessage = () => visible()[0] || null;

    const lastMessage = () => {
      const panels = visible();
      return panels[panels.length - 1] || null;
    };

    const readTail = () => {
      try {
        return localStorage.getItem(perFolio(TAIL)) === "1";
      } catch {
        return false;
      }
    };

    let tailing = readTail();

    const paintTail = () => {
      if (tailButton) tailButton.setAttribute("aria-pressed", String(tailing));
    };

    const scrollToEnd = (behavior) => {
      const target = lastMessage();
      if (target) target.scrollIntoView({ behavior, block: "start" });
    };

    const scrollToTop = (behavior) => {
      const target = firstMessage();
      if (target) target.scrollIntoView({ behavior, block: "start" });
    };

    const setTail = (on, behavior) => {
      tailing = on;
      try {
        localStorage.setItem(perFolio(TAIL), on ? "1" : "0");
      } catch {}
      paintTail();
      if (on) scrollToEnd(behavior);
    };

    // A programmatic scrollIntoView never emits wheel/touch/keydown, so those
    // are unambiguous reader input: any of them hands control back.
    const releaseTail = () => {
      if (tailing) setTail(false);
    };
    window.addEventListener("wheel", releaseTail, { passive: true });
    window.addEventListener("touchmove", releaseTail, { passive: true });
    window.addEventListener("keydown", (event) => {
      const tag = event.target.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "BUTTON") return;
      if (SCROLL_KEYS.has(event.key)) releaseTail();
    });

    // A deep link (#turn-N) names the panel the reader came for, so it releases
    // follow like a wheel or arrow key does, rather than losing the landing to
    // a snap to the end. Releasing (not just skipping this load's snap) because
    // the hash survives the reloads a live session drives, and a suppression
    // that didn't persist would fight the anchor on every one.
    //
    // The hash is arbitrary text off the end of a shared URL, so it need be
    // neither a valid selector (hence getElementById, as querySelector throws)
    // nor validly escaped (hence the guard, as a stray "%" throws).
    const anchorId = () => {
      const raw = location.hash.slice(1);
      try {
        return decodeURIComponent(raw);
      } catch {
        return raw;
      }
    };
    const anchored = location.hash ? document.getElementById(anchorId()) : null;
    const deepLink = anchored && container.contains(anchored) ? anchored : null;
    if (deepLink) releaseTail();

    // On load, if still following, snap to the newest message at once; a second
    // pass after layout settles (web fonts shift it) lands it precisely. A
    // deep-linked turn needs that second pass too, since the browser's own
    // anchor scroll happens before the fonts land.
    paintTail();
    if (tailing) {
      scrollToEnd("auto");
      requestAnimationFrame(() => {
        if (tailing) scrollToEnd("auto");
      });
    } else if (deepLink) {
      requestAnimationFrame(() => {
        deepLink.scrollIntoView({ behavior: "auto", block: "start" });
      });
    }

    dock.addEventListener("click", (event) => {
      const button = event.target.closest("button");
      if (!button) return;
      const { nav, role, fold: foldTo, tail } = button.dataset;
      if (nav === "prev") jump(-1, role);
      else if (nav === "next") jump(1, role);
      // Leaping to the top is the reader taking control, so it releases follow
      // the way a wheel or arrow key does: otherwise the next reload of a live
      // session would snap straight back to the end.
      else if (nav === "top") {
        releaseTail();
        scrollToTop("smooth");
      } else if (nav === "end") setTail(true, "smooth");
      else if (tail === "toggle") setTail(!tailing, "smooth");
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
        const stored = JSON.parse(localStorage.getItem(perFolio(FOLDS)) || "[]");
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
          localStorage.setItem(perFolio(FOLDS), JSON.stringify([...folds]));
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
