// Loads the folio's functional core the way a folio does: as a plain script,
// evaluated on its own, with no DOM and no imports. It declares one `const`, so
// the script's completion value is that object and nothing has to be exported
// for the tests' sake, which would be a second interface to keep in step with
// the one the browser uses.
import { readFileSync } from "node:fs";
import { runInThisContext } from "node:vm";

const source = readFileSync(
  new URL("../../src/illumination.core.js", import.meta.url),
  "utf8",
);

// This realm rather than a fresh one, so what comes back is built from the same
// Array and Object the assertions are: a value from another realm fails a
// strict deep comparison however equal it looks. The braces keep the core's own
// declaration a block-scoped one, so loading this twice is not a redeclaration.
export const core = runInThisContext(`{\n${source}\ncore;\n}`);
