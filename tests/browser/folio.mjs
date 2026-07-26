// What the browser tests need: a built binary, a rendered folio, and a served
// session they can grow under the reader.
//
// The folios are rendered through the real CLI rather than a library call, so
// what is driven in the browser is the artifact a reader opens. Release-built
// for the same reason `just render` is: an unoptimized render takes longer than
// an optimized build and render together.
import { execFileSync, spawn } from "node:child_process";
import { appendFileSync, copyFileSync, mkdtempSync } from "node:fs";
import { createConnection, createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { chromium } from "playwright";

const ROOT = new URL("../../", import.meta.url).pathname;
const BINARY = join(ROOT, "target", "release", "claude-scriptorium");

let built = false;

const build = () => {
  if (built) return;
  execFileSync("cargo", ["build", "--release"], { cwd: ROOT, stdio: "inherit" });
  built = true;
};

const scratch = () => mkdtempSync(join(tmpdir(), "folio-"));

/** Renders a fixture session and answers with the URL of the written folio. */
export const render = (fixture) => {
  build();
  const written = join(scratch(), fixture.replace(".jsonl", ".html"));
  execFileSync(BINARY, ["render", `tests/fixtures/${fixture}`, "-o", written], {
    cwd: ROOT,
  });
  return pathToFileURL(written).href;
};

/**
 * Serves a copy of a session, which the test may grow. Only a served folio
 * carries the follow control, so following can be exercised nowhere else.
 */
export const serve = async () => {
  build();
  const session = join(scratch(), "live.jsonl");
  copyFileSync(join(ROOT, "tests/fixtures/session.jsonl"), session);
  const port = await freePort();
  const server = spawn(BINARY, ["serve", session, "--port", String(port)], {
    cwd: ROOT,
    stdio: "ignore",
  });
  await reachable(port);
  return {
    url: `http://127.0.0.1:${port}/`,
    // Append an assistant turn, as a live session gains one. `serve` polls the
    // session's mtime and reloads the page on its own.
    grow(note) {
      appendFileSync(
        session,
        JSON.stringify({
          type: "assistant",
          timestamp: "2026-03-11T16:00:00.000Z",
          isSidechain: false,
          message: {
            role: "assistant",
            model: "claude-opus-5",
            content: [{ type: "text", text: note }],
          },
        }) + "\n",
      );
    },
    stop() {
      server.kill();
    },
  };
};

// Bound rather than guessed: a listener on port 0 is handed a free one, and
// handing it straight back leaves the port free for the server under test.
const freePort = () =>
  new Promise((resolve, reject) => {
    const probe = createServer();
    probe.on("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
  });

const reachable = async (port, attempts = 400) => {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const up = await new Promise((resolve) => {
      const probe = createConnection({ port, host: "127.0.0.1" });
      probe.on("connect", () => {
        probe.destroy();
        resolve(true);
      });
      probe.on("error", () => resolve(false));
    });
    if (up) return;
    await new Promise((wake) => setTimeout(wake, 25));
  }
  throw new Error(`nothing came up on port ${port}`);
};

/** One browser for the whole file, one page per test. */
export const browsing = () => {
  const state = {};
  return {
    async open() {
      state.browser = await chromium.launch();
    },
    async close() {
      await state.browser?.close();
    },
    async page() {
      await state.page?.close();
      state.page = await state.browser.newPage({
        viewport: { width: 1400, height: 900 },
      });
      return state.page;
    },
  };
};

// The minimap is drawn once the folio's own layout settles, so a test waits on a
// band rather than on `load`: by the time one exists, the script has run.
export const BAND = ".minimap__band";

export const openFolio = async (page, url) => {
  await page.goto(url);
  await page.waitForSelector(BAND);
};

/** Where the folio says the reader landed, however they got there. */
export const landed = (page) =>
  page.evaluate(() => {
    const panel = document.querySelector(".turn[data-landed], .turn:target");
    return panel ? panel.id : null;
  });
