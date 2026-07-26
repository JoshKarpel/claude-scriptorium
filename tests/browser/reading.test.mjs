// The folio's controls, driven in a real browser.
//
// What is asserted here is what only a browser can answer: where a control puts
// the reader, what survives a reload, and what a live session does under one.
// The arithmetic behind each of these is pinned down without a browser in
// `tests/js`, and the markup they act on in `tests/render.rs`; this suite is for
// the seams between them, which is where every regression in these controls has
// actually been.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";

import { BAND, browsing, landed, openFolio, render, serve } from "./folio.mjs";

const browser = browsing();
before(() => browser.open());
after(() => browser.close());

// The gloss swatch holds one panel of every kind the folio can draw; the
// playground one call and result per built-in tool, so it is full of folds.
const swatch = render("glosses.jsonl");
const playground = render("playground.jsonl");

describe("the minimap", () => {
  it("draws a band for every panel", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);

    const panels = await page.locator("main.folio .turn").count();
    assert.equal(await page.locator(BAND).count(), panels);
  });

  it("gives a band the pigment of the panel it stands for", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);

    // The swatch reaches every kind, so this weighs the whole palette: one
    // whose pigment never reached the stylesheet comes back as the neutral ink
    // the rest fall back to, and collides with another kind.
    const hues = await page.evaluate(() => {
      const seen = {};
      document.querySelectorAll(".minimap__band").forEach((band) => {
        seen[band.dataset.kind] = getComputedStyle(band).backgroundColor;
      });
      return seen;
    });
    // `note` is the catch-all and keeps that neutral ink on purpose.
    const pigmented = Object.entries(hues).filter(([kind]) => kind !== "note");
    assert.equal(
      new Set(pigmented.map(([, hue]) => hue)).size,
      pigmented.length,
      `two kinds share a pigment: ${JSON.stringify(hues, null, 2)}`,
    );
  });

  it("lands a scrub on the panel under the pointer", async () => {
    const page = await browser.page();
    await openFolio(page, playground);

    // The tallest band, so the press is unambiguously inside it rather than a
    // pixel from a neighbour.
    const band = await page.evaluate(() => {
      const bands = [...document.querySelectorAll(".minimap__band")];
      const tallest = bands.reduce((a, b) =>
        b.getBoundingClientRect().height > a.getBoundingClientRect().height ? b : a,
      );
      const box = tallest.getBoundingClientRect();
      return {
        turn: tallest.dataset.turn,
        x: box.x + box.width / 2,
        y: box.y + box.height / 2,
      };
    });
    await page.mouse.click(band.x, band.y);

    // Named, marked, and scrolled to: a landing is all three.
    assert.equal(await page.evaluate(() => location.hash), `#turn-${band.turn}`);
    assert.equal(await landed(page), `turn-${band.turn}`);
    const distance = await page.evaluate(() =>
      Math.abs(document.querySelector(".turn[data-landed]").getBoundingClientRect().top),
    );
    assert.ok(distance < 60, `landed ${distance}px from the top of the leaf`);
  });

  it("zooms on the wheel, without moving the reader", async () => {
    const page = await browser.page();
    await openFolio(page, playground);
    const spread = () =>
      page.evaluate(() => {
        const bands = [...document.querySelectorAll(".minimap__band")];
        const tops = bands.map((band) => band.getBoundingClientRect().top);
        return Math.max(...tops) - Math.min(...tops);
      });
    const before = await spread();
    const scrolled = await page.evaluate(() => window.scrollY);

    const box = await page.locator(".minimap__track").boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, -600);
    await page.waitForSelector(".minimap[data-zoomed]");

    // The bands are further apart than the whole folio drew them, and the leaf
    // itself has not moved: the zoom is the map's own.
    assert.ok((await spread()) > before);
    assert.equal(await page.evaluate(() => window.scrollY), scrolled);
  });

  it("keeps a kind the key takes out of play in its place", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);
    const tops = () =>
      page.evaluate(() => [...document.querySelectorAll(".minimap__band")].map((b) => b.style.top));
    const before = await tops();

    await page.click('.key__chip[data-scope="tool"]');

    assert.ok(
      (await page.locator('.minimap__band[data-kind="tool"][data-in-play="false"]').count()) > 0,
    );
    // Still drawn where it was: the map would misstate where everything else
    // sits if a kind out of play were dropped from it.
    assert.deepEqual(await tops(), before);
  });
});

describe("the dock", () => {
  it("names the turn a step lands on", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);

    await page.click('[data-nav="next"]:not([data-side])');

    const hash = await page.evaluate(() => location.hash);
    assert.match(hash, /^#turn-\d+$/);
    assert.equal(await landed(page), hash.slice(1));
  });

  it("seeks one side of the exchange from a flanking arrow", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);

    await page.click('[data-nav="next"][data-side="model"]');

    const side = await page.evaluate(
      () => document.querySelector(".turn[data-landed]").dataset.side,
    );
    assert.equal(side, "model");
  });

  it("steps only through the kinds the key leaves in play", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);
    for (const kind of ["assistant", "tool", "plan"]) {
      await page.click(`.key__chip[data-scope="${kind}"]`);
    }

    await page.click('[data-nav="next"][data-side="model"]');

    const kind = await page.evaluate(
      () => document.querySelector(".turn[data-landed]").dataset.kind,
    );
    assert.equal(kind, "thinking");
  });

  it("folds every marginalia open and shut", async () => {
    const page = await browser.page();
    await openFolio(page, playground);
    const folds = page.locator("main.folio details");

    await page.click('[data-fold="expand"]');
    assert.ok(await folds.evaluateAll((nodes) => nodes.every((node) => node.open)));

    await page.click('[data-fold="collapse"]');
    assert.ok(await folds.evaluateAll((nodes) => nodes.every((node) => !node.open)));
  });
});

describe("the search", () => {
  it("opens a fold that holds the hit it steps to", async () => {
    const page = await browser.page();
    await openFolio(page, playground);

    await page.fill(".search__input", "PATH");
    await page.press(".search__input", "Enter");

    const current = page.locator("mark.search__hit.is-current");
    assert.equal(await current.count(), 1);
    assert.ok(
      await current.evaluate((node) => {
        const fold = node.closest("details");
        return fold === null || fold.open;
      }),
    );
  });

  it("looks only where the key leaves in play", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);
    // A phrase the swatch sets in one kind of gloss alone, so taking that kind
    // out of play must leave the search nothing to find.
    await page.fill(".search__input", "aggressively");
    assert.ok((await page.locator("mark.search__hit").count()) > 0);

    await page.click('.key__chip[data-scope="rule"]');

    assert.equal(await page.locator("mark.search__hit").count(), 0);
  });
});

describe("following the end of a live session", () => {
  it("is not offered by a written folio", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);

    // A written folio is a snapshot of a session that may have ended a year
    // ago: there is nothing to follow, and the control's absence is what tells
    // the app script so.
    assert.equal(await page.locator('[data-tail="toggle"]').count(), 0);
  });

  it("holds across the reloads a growing session drives", async () => {
    const page = await browser.page();
    const served = await serve();
    try {
      await openFolio(page, served.url);
      await page.click('[data-tail="toggle"]');

      for (const note of ["the first turn appended", "the second turn appended"]) {
        const before = await page.locator("main.folio .turn").count();
        served.grow(note);
        await page.waitForFunction(
          (count) => document.querySelectorAll("main.folio .turn").length > count,
          before,
          { timeout: 20000 },
        );
        await page.waitForSelector(BAND);

        // Still following, and the permalink has moved on to the new end: the
        // URL names where the reader is, so the next reload resumes there and a
        // link copied out of a followed folio names what was on the screen.
        assert.equal(
          await page.getAttribute('[data-tail="toggle"]', "aria-pressed"),
          "true",
          "following was released by a reload it drove itself",
        );
        const newest = await page.evaluate(
          () => [...document.querySelectorAll("main.folio .turn")].pop().id,
        );
        assert.equal(await page.evaluate(() => location.hash), `#${newest}`);
        assert.equal(await landed(page), newest);
        // The browser must not restore the position it recorded before the
        // reload, which lands after this script runs and would undo the snap.
        assert.equal(await page.evaluate(() => history.scrollRestoration), "manual");
      }
    } finally {
      served.stop();
    }
  });

  it("is released by a deep link the reader arrives with", async () => {
    const page = await browser.page();
    const served = await serve();
    try {
      await openFolio(page, served.url);
      await page.click('[data-tail="toggle"]');

      await page.goto(`${served.url}#turn-2`);
      await page.waitForSelector(BAND);

      assert.equal(await page.getAttribute('[data-tail="toggle"]', "aria-pressed"), "false");
      assert.equal(await page.evaluate(() => location.hash), "#turn-2");
    } finally {
      served.stop();
    }
  });

  it("is released by the reader scrolling, and then forgotten", async () => {
    const page = await browser.page();
    const served = await serve();
    try {
      await openFolio(page, served.url);
      await page.click('[data-tail="toggle"]');
      assert.equal(await page.getAttribute('[data-tail="toggle"]', "aria-pressed"), "true");

      await page.mouse.wheel(0, -400);

      // Waited for rather than read straight back: the wheel listener is passive
      // and runs on its own frame, which is the whole point of listening for one.
      await page.waitForSelector('[data-tail="toggle"][aria-pressed="false"]');
      const remembered = await page.evaluate(() =>
        localStorage.getItem("scriptorium-tail:" + document.body.dataset.folio),
      );
      assert.equal(remembered, null, "a released follow would snap back on the next reload");
    } finally {
      served.stop();
    }
  });
});

describe("what a folio remembers", () => {
  it("keeps the reader's chosen scheme across a reload", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);

    await page.click('[data-theme-choice="dark"]');
    await page.reload();
    await page.waitForSelector(BAND);

    assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), "dark");
    assert.equal(await page.getAttribute('[data-theme-choice="dark"]', "aria-pressed"), "true");
  });

  it("keeps the kinds the reader set aside across a reload", async () => {
    const page = await browser.page();
    await openFolio(page, swatch);

    await page.click('.key__chip[data-scope="tool"]');
    await page.reload();
    await page.waitForSelector(BAND);

    assert.equal(
      await page.getAttribute('.key__chip[data-scope="tool"]', "aria-pressed"),
      "false",
    );
    // And everything that answers to the key comes back narrowed with it.
    assert.ok(
      (await page.locator('.minimap__band[data-kind="tool"][data-in-play="false"]').count()) > 0,
    );
  });

  it("keeps the map's zoom across a reload", async () => {
    const page = await browser.page();
    await openFolio(page, playground);
    const box = await page.locator(".minimap__track").boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, -600);
    await page.waitForSelector(".minimap[data-zoomed]");

    await page.reload();
    await page.waitForSelector(BAND);

    // Framed by the reader, so it is theirs to keep: the map comes back on the
    // stretch they had opened up rather than on the whole folio.
    await page.waitForSelector(".minimap[data-zoomed]");
  });

  it("keeps a fold the reader opened across a reload", async () => {
    const page = await browser.page();
    await openFolio(page, playground);
    const fold = page.locator("main.folio details").first();
    await fold.evaluate((node) => (node.open = true));
    const turn = await fold.evaluate((node) => node.closest(".turn").dataset.turn);

    await page.reload();
    await page.waitForSelector(BAND);

    assert.ok(
      await page.evaluate(
        (which) => document.querySelector(`.turn[data-turn='${which}'] details`).open,
        turn,
      ),
    );
  });
});
