// The folio's functional core, exercised without a browser. What each of these
// drives is exercised in one under `tests/browser`; this is where the arithmetic
// and its edges are pinned down, since a browser test can reach a case like an
// empty folio or a malformed hash only by contriving a fixture for it.
import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { core } from "./core.mjs";

describe("what the reader is remembered by", () => {
  it("scopes a store to the folio the markup names", () => {
    assert.equal(core.perFolio("scriptorium-folds", "abc-123"), "scriptorium-folds:abc-123");
  });

  it("scopes to a placeholder rather than sharing one store between folios", () => {
    // Every folio opened from disk shares the `file://` origin, so an unscoped
    // key is one folio's state imposed on all of them.
    assert.equal(core.perFolio("scriptorium-tail", undefined), "scriptorium-tail:?");
    assert.equal(core.perFolio("scriptorium-tail", ""), "scriptorium-tail:?");
  });

  it("keeps a stored theme only if it is one the stylesheet has rules for", () => {
    assert.equal(core.theme("dark"), "dark");
    assert.equal(core.theme("light"), "light");
    assert.equal(core.theme("sepia"), "system");
    assert.equal(core.theme(null), "system");
  });

  it("keys a fold by its turn and its place within that turn", () => {
    assert.equal(core.foldKey("12", 3), "12:3");
  });

  it("keys a fold outside any turn without losing its place", () => {
    assert.equal(core.foldKey(undefined, 0), "?:0");
  });
});

describe("the hash, which two hands write", () => {
  it("reads a turn's id out of a permalink", () => {
    assert.equal(core.anchorId("#turn-42"), "turn-42");
  });

  it("decodes an escaped hash", () => {
    assert.equal(core.anchorId("#turn%2D7"), "turn-7");
  });

  it("hands back a malformed hash rather than throwing", () => {
    // Arbitrary text off the end of a shared URL: a stray "%" is not an escape.
    assert.equal(core.anchorId("#100%-done"), "100%-done");
  });

  it("takes a hash naming a turn other than the pin as the reader's", () => {
    assert.equal(core.readersHash("#turn-3", "turn-9"), true);
  });

  it("takes the hash following itself wrote as the folio's own", () => {
    // The pin is what lets following survive a reload: a live session grows
    // between loads, so the hash it wrote no longer names the end.
    assert.equal(core.readersHash("#turn-9", "turn-9"), false);
  });

  it("takes no hash at all as nobody's", () => {
    assert.equal(core.readersHash("", null), false);
  });
});

describe("stepping between panels", () => {
  const tops = [-800, -120, 260, 900];

  it("counts the panel at the top of the viewport as the current one", () => {
    assert.equal(core.currentIndex(tops, 40), 1);
  });

  it("counts a panel just under the threshold as current, not the next one", () => {
    // The threshold clears a turn's own scroll-margin, so the panel just
    // navigated to is where the reader is rather than one step behind it.
    assert.equal(core.currentIndex([12, 380], 40), 0);
  });

  it("has no current panel when every one of them is below the fold", () => {
    assert.equal(core.currentIndex([90, 400], 40), -1);
  });

  it("steps forward and back from wherever the reader is", () => {
    assert.equal(core.stepIndex(tops, 1, 40), 2);
    assert.equal(core.stepIndex(tops, -1, 40), 0);
  });

  it("clamps at either end rather than wrapping round the folio", () => {
    assert.equal(core.stepIndex([-40, -20, -10], 1, 40), 2);
    assert.equal(core.stepIndex([90, 400], -1, 40), 0);
  });

  it("finds nothing to step to when the key leaves no panel in play", () => {
    assert.equal(core.stepIndex([], 1, 40), -1);
  });
});

describe("the minimap's geometry", () => {
  const whole = core.lens({ leaf: 10000, track: 600 });

  it("shows the whole folio until it is zoomed", () => {
    assert.equal(whole.zoom, 1);
    assert.equal(whole.span, 10000);
    assert.equal(whole.origin, 0);
  });

  it("gives a band the share of the track its panel takes of the document", () => {
    assert.deepEqual(core.bandBox(2000, 500, whole, 2), { top: 120, height: 30 });
  });

  it("floors a band so a one-line note is still somewhere to aim at", () => {
    const long = core.lens({ leaf: 100000, track: 600 });
    assert.equal(core.bandBox(0, 20, long, 2).height, 2);
  });

  it("draws the reader's view where it sits in the document", () => {
    assert.deepEqual(core.viewBox(5000, 1000, whole, 12), { top: 300, height: 60 });
  });

  it("floors the view, which a long folio would otherwise draw as a pixel", () => {
    const long = core.lens({ leaf: 400000, track: 600 });
    assert.equal(core.viewBox(0, 900, long, 12).height, 12);
  });

  it("measures against a leaf of no height without dividing by zero", () => {
    const empty = core.lens({ leaf: 0, track: 600 });
    assert.equal(Number.isFinite(core.viewBox(0, 900, empty, 12).height), true);
  });

  it("draws a zoomed band larger and offset by what the lens shows", () => {
    const close = core.lens({ leaf: 10000, track: 600, zoom: 4, origin: 2000 });
    // A quarter of the document fills the track, so a panel takes four times
    // the band it did, measured from the top of the stretch on show.
    assert.deepEqual(core.bandBox(2500, 500, close, 2), { top: 120, height: 120 });
  });

  it("holds still whatever the pointer is over while zooming", () => {
    const under = whole.origin + 300 / whole.scale;

    const closer = core.zoomedAbout(whole, 300, 2, 64);

    assert.equal(closer.zoom, 2);
    assert.equal(closer.origin + 300 / closer.scale, under);
  });

  it("never zooms out past the whole folio, or in past the limit", () => {
    assert.equal(core.zoomedAbout(whole, 300, 0.1, 64).zoom, 1);
    assert.equal(core.zoomedAbout(whole, 300, 1000, 64).zoom, 64);
  });

  it("never points the lens off either end of the document", () => {
    const closer = core.zoomedAbout(whole, 0, 4, 64);
    assert.ok(closer.origin >= 0);

    const atTheEnd = core.lens({ leaf: 10000, track: 600, zoom: 4, origin: 9999 });
    assert.equal(atTheEnd.origin + atTheEnd.span, 10000);
  });

  it("leaves the lens alone while the reader is inside what it shows", () => {
    const close = core.lens({ leaf: 10000, track: 600, zoom: 2, origin: 2000 });

    assert.equal(core.followed(close, 3000, 900), close);
  });

  it("brings the lens back to the reader when they scroll out of it", () => {
    // Zoom is the map's own, but a reader who has moved on wants the map to
    // have moved with them.
    const close = core.lens({ leaf: 10000, track: 600, zoom: 2, origin: 0 });

    const moved = core.followed(close, 8000, 900);

    assert.ok(moved.origin <= 8000 && 8900 <= moved.origin + moved.span);
    assert.equal(moved.zoom, 2);
  });

  it("lands a scrub on the band under the pointer", () => {
    const bands = [
      { top: 0, height: 40, inPlay: true },
      { top: 40, height: 40, inPlay: true },
      { top: 80, height: 40, inPlay: true },
    ];
    assert.equal(core.nearestIndex(95, bands), 2);
  });

  it("passes over a kind the key has taken out of play", () => {
    const bands = [
      { top: 0, height: 40, inPlay: true },
      { top: 40, height: 40, inPlay: false },
      { top: 80, height: 40, inPlay: true },
    ];
    assert.equal(core.nearestIndex(55, bands), 0);
  });

  it("finds nowhere to land when nothing is in play", () => {
    assert.equal(core.nearestIndex(55, [{ top: 0, height: 40, inPlay: false }]), -1);
  });
});

describe("searching", () => {
  it("finds every occurrence, whatever case it was written in", () => {
    assert.deepEqual(core.spans("Quire and quire", "QUIRE"), [
      { from: 0, to: 5 },
      { from: 10, to: 15 },
    ]);
  });

  it("cuts the spans at the needle's own length, not the haystack's case", () => {
    const [span] = core.spans("A Folio here", "folio");
    assert.deepEqual(span, { from: 2, to: 7 });
  });

  it("does not overlap two matches that share a character", () => {
    // "aa" in "aaa" is one match from 0 and not a second from 1: the second
    // would mark a character the first already marked.
    assert.deepEqual(core.spans("aaa", "aa"), [{ from: 0, to: 2 }]);
  });

  it("finds nothing for an empty query rather than one hit per character", () => {
    assert.deepEqual(core.spans("a folio", ""), []);
  });
});
