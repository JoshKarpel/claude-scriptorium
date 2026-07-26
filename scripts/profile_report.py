#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///

import argparse
import bisect
import json
import re
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

DESCRIPTION = """\
Fold a samply profile into a plain-text report.

Reads the JSON samply writes under `--save-only`, whether it recorded the run
itself or imported a `perf.data`, and prints where the CPU went: self time by
crate and by symbol, time on the stack, and the hottest stacks.
Addresses in the profiled binary are resolved with `addr2line`, which recovers
inlined frames rather than charging them to whatever they were inlined into; in
an optimized Rust binary most of a render inlines into a handful of outer
symbols, so a symbol-table reading would report little beyond `main`. A shared
library is read from its dynamic symbol table with `nm` instead: a stripped
system library has no debug info for addr2line, which then answers with the
nearest preceding exported symbol rather than admitting it doesn't know.

Samples from every thread and process that ran the binary are aggregated, so a
recording of several renders reads as one run, and whatever looped them does
not read at all.
"""

UNRESOLVED = "??"
KERNEL_SPACE = 0xFFFF800000000000
FUNCTIONS = "TtWwi"
NAME_WIDTH = 100
STACK_WIDTH = 3
STACK_DEPTH = 14

HASH_SUFFIX = re.compile(r"::h[0-9a-f]{16}$")
LLVM_SUFFIX = re.compile(r" \(\.llvm\.\d+\)$")
ESCAPE = re.compile(r"\$(u[0-9a-f]{2}|[A-Z]{1,2})\$")
ESCAPES = {"SP": "@", "BP": "*", "RF": "&", "LT": "<", "GT": ">", "LP": "(", "RP": ")", "C": ","}
GENERICS = re.compile(r"<[^<>]*>")


@dataclass(frozen=True, slots=True)
class Frame:
    """One symbol on the stack, with the source location it was sampled at."""

    name: str
    location: str | None


@dataclass(frozen=True, slots=True)
class Symbols:
    """A library's exported functions, for naming an address that falls in one."""

    starts: list[int]
    entries: list[tuple[int, int, str]]

    def name(self, address: int) -> str | None:
        index = bisect.bisect_right(self.starts, address) - 1
        if index < 0:
            return None
        start, size, symbol = self.entries[index]
        return symbol if address < start + size else None


@dataclass(frozen=True, slots=True)
class Tally:
    """What the samples add up to, keyed by symbol, crate, and call path."""

    own: Counter[str]
    inclusive: Counter[str]
    by_crate: Counter[str]
    stacks: Counter[tuple[str, ...]]
    roots: list[Counter[str]]
    locations: dict[str, Counter[str]]


def tool(*candidates: str) -> str:
    """The first of these binutils on PATH.

    GNU binutils demangle Rust symbols where LLVM's leave parts of them
    escaped, so each pair asks for GNU's first and falls back to LLVM's (which
    is what a machine without binutils, notably a Mac, is likely to have).
    """
    for candidate in candidates:
        found = shutil.which(candidate)
        if found:
            return found
    sys.exit(f"profile_report.py needs one of {', '.join(candidates)} on PATH")


def demangled(name: str) -> str:
    """A symbol with its hash suffix dropped and any legacy escapes decoded."""
    if "$" in name:
        name = ESCAPE.sub(decoded, name).replace("..", "::")
    return HASH_SUFFIX.sub("", LLVM_SUFFIX.sub("", name))


def decoded(escape: re.Match[str]) -> str:
    code = escape.group(1)
    if code.startswith("u"):
        return chr(int(code[1:], 16))
    return ESCAPES.get(code, escape.group(0))


def shortened(name: str) -> str:
    """A symbol cut to its last couple of path segments, for a stack line."""
    library, _, symbol = name.rpartition("!")
    bare = symbol
    while (stripped := GENERICS.sub("", bare)) != bare:
        bare = stripped
    segments = [segment for segment in bare.split("::") if segment]
    return (library + "!" if library else "") + "::".join(segments[-STACK_WIDTH:] or [symbol])


def crate_of(name: str) -> str:
    """The crate (or shared library) a symbol belongs to.

    A heuristic over the demangled name, enough to roll the report up by
    dependency rather than a parse of Rust's type grammar.
    """
    if "!" in name:
        return name.split("!", 1)[0]
    head = name.lstrip("<&*").split(" as ")[0]
    for separator in ("::", "<", "("):
        head = head.split(separator)[0]
    return head.rstrip(">") or name


def resolve(tool: str, binary: str, addresses: list[int]) -> dict[int, tuple[Frame, ...]]:
    """Resolve each address in one library to its inline chain, outermost first."""
    if not Path(binary).exists():
        return {}
    completed = subprocess.run(
        [tool, "--exe", binary, "--addresses", "--functions", "--inlines", "--demangle"],
        input="\n".join(hex(address) for address in addresses),
        capture_output=True,
        text=True,
        check=True,
    )
    # Per address, addr2line emits the address, then a (symbol, location) pair
    # per inline frame, innermost first.
    chains: dict[int, tuple[Frame, ...]] = {}
    address: int | None = None
    chain: list[Frame] = []
    lines = iter(completed.stdout.splitlines())
    for line in lines:
        if is_address(line):
            if address is not None:
                chains[address] = tuple(reversed(chain))
            address, chain = int(line, 16), []
            continue
        location = next(lines)
        chain.append(Frame(demangled(line), None if location.startswith(UNRESOLVED) else location))
    if address is not None:
        chains[address] = tuple(reversed(chain))
    return chains


def is_address(line: str) -> bool:
    """Whether a line is one of addr2line's `0x…` address markers.

    A demangled symbol can hold anything, so match the marker's whole shape
    rather than just its prefix.
    """
    if not line.startswith("0x") or " " in line:
        return False
    try:
        int(line, 16)
    except ValueError:
        return False
    return True


def unmapped(name: str) -> str:
    """Name a frame that belongs to no mapped library.

    Time in the kernel is sampled against the user stack that entered it, and
    the addresses stay unresolved unless `kptr_restrict` is open, so gather
    them under one name rather than one row per address.
    """
    if is_address(name) and int(name, 16) >= KERNEL_SPACE:
        return "[kernel]"
    return name


def library_of(thread: dict, libraries: list[dict], function: int) -> dict | None:
    resource = thread["funcTable"]["resource"][function]
    if resource is None or resource < 0:
        return None
    index = thread["resourceTable"]["lib"][resource]
    return libraries[index] if index is not None else None


def profiled(libraries: list[dict], product: str, binary: Path | None) -> dict:
    """The library the report is about, whose frames carry the interesting code.

    A profile samply recorded names the binary as its product; one imported
    from a `perf.data` names whatever perf was told to run, which is the shell
    the renders were looped in, so the recipe says which binary it meant.
    """
    if binary is not None:
        wanted = str(binary.resolve())
        found = next((library for library in libraries if library["path"] == wanted), None)
        if found is None:
            sys.exit(f"no frames from {binary} in this profile")
        return found
    found = next((library for library in libraries if library["debugName"] == product), None)
    if found is None:
        sys.exit(f"nothing in this profile matches its product ({product}): pass --binary")
    return found


def ran_binary(thread: dict, libraries: list[dict], binary: dict) -> bool:
    """Whether a thread executed the profiled binary at all.

    perf follows children, so a recording that loops the renders under a shell
    carries the shell and its other children too; they are the harness rather
    than the measurement.
    """
    return any(
        (library := library_of(thread, libraries, thread["frameTable"]["func"][index]))
        and library["path"] == binary["path"]
        for index in range(thread["frameTable"]["length"])
    )


def sampled_addresses(threads: list[dict], libraries: list[dict], binary: dict) -> list[int]:
    """Every address sampled in the profiled binary itself."""
    addresses: set[int] = set()
    for thread in threads:
        for index in range(thread["frameTable"]["length"]):
            library = library_of(thread, libraries, thread["frameTable"]["func"][index])
            if library and library["path"] == binary["path"]:
                addresses.add(thread["frameTable"]["address"][index])
    return sorted(addresses)


def exported(tool: str, library: str) -> Symbols:
    """The functions a shared library exports, by address.

    The dynamic symbol table only: a stripped system library keeps nothing
    else, and reading it with sizes means an address inside an unexported
    function (a memmove variant, malloc's innards) stays an address instead of
    borrowing the name of whatever happens to precede it.
    """
    completed = subprocess.run(
        [tool, "--dynamic", "--defined-only", "--print-size", "--demangle", library],
        capture_output=True,
        text=True,
        check=False,
    )
    entries = []
    for line in completed.stdout.splitlines():
        columns = line.split(maxsplit=3)
        if len(columns) == 4 and columns[2] in FUNCTIONS:
            entries.append((int(columns[0], 16), int(columns[1], 16), columns[3].split("@")[0]))
    entries.sort()
    return Symbols([entry[0] for entry in entries], entries)


def frames_of(
    thread: dict,
    libraries: list[dict],
    chains: dict[int, tuple[Frame, ...]],
    symbols: dict[str, Symbols],
    binary: dict,
) -> list[tuple[Frame, ...]]:
    """Resolve every frame in a thread's frame table to its inline chain.

    A frame from a shared library is named `library!symbol`, so the report can
    tell the profiled binary's own code (which holds every crate it was linked
    from) apart from what it calls out to.
    """
    strings = thread["stringArray"]
    resolved: list[tuple[Frame, ...]] = []
    for index in range(thread["frameTable"]["length"]):
        function = thread["frameTable"]["func"][index]
        address = thread["frameTable"]["address"][index]
        library = library_of(thread, libraries, function)
        if library is None:
            resolved.append((Frame(unmapped(strings[thread["funcTable"]["name"][function]]), None),))
            continue
        if library["path"] == binary["path"]:
            fallback = (Frame(f"{library['debugName']}!{hex(address)}", None),)
            resolved.append(chains.get(address) or fallback)
            continue
        known = symbols[library["path"]].name(address) if library["path"] in symbols else None
        resolved.append((Frame(f"{library['debugName']}!{known or hex(address)}", None),))
    return resolved


def tally(threads: list[dict], frames: dict[int, list[tuple[Frame, ...]]]) -> Tally:
    """Walk every sample's stack, charging its weight to each way of reading it."""
    tallied = Tally(Counter(), Counter(), Counter(), Counter(), [], defaultdict(Counter))
    for order, thread in enumerate(threads):
        resolved = frames[order]
        roots: Counter[str] = Counter()
        tallied.roots.append(roots)
        stacks = thread["stackTable"]
        paths: list[tuple[Frame, ...]] = []
        for index in range(stacks["length"]):
            prefix = stacks["prefix"][index]
            parent = paths[prefix] if prefix is not None else ()
            paths.append(parent + resolved[stacks["frame"][index]])
        samples = thread["samples"]
        weights = samples.get("weight") or [1] * samples["length"]
        for stack, weight in zip(samples["stack"], weights, strict=True):
            if stack is None or not paths[stack]:
                continue
            path = paths[stack]
            leaf = path[-1]
            roots[path[0].name] += weight
            tallied.own[leaf.name] += weight
            tallied.by_crate[crate_of(leaf.name)] += weight
            if leaf.location:
                tallied.locations[leaf.name][leaf.location] += weight
            for name in {frame.name for frame in path}:
                tallied.inclusive[name] += weight
            tallied.stacks[tuple(frame.name for frame in path)] += weight
    return tallied


def unwound(roots: list[Counter[str]], total: int) -> str:
    """How many stacks reached the bottom, which is how far to trust the tables.

    A sampler that can't unwind a stack all the way reports it rooted wherever
    it gave up, and every frame above that loses the sample. A complete unwind
    reaches the frame a thread began at, so the test is per thread and against
    whichever root that thread's stacks agree on, rather than against a named
    entry point: a worker thread starts where its pool spawned it and never
    sees `_start` at all. The share is how much of the tables can be read as
    inclusive time; below it, they are a floor.
    """
    reached = sum(root.most_common(1)[0][1] for root in roots if root)
    share = 100 * reached / total
    caveat = "" if share > 95 else ", so inclusive figures below are a floor"
    return f"{share:.1f}% of stacks unwound to the frame their thread began at{caveat}"


def truncated(name: str, width: int) -> str:
    return name if len(name) <= width else name[: width - 3] + "..."


def symbol_table(
    title: str,
    counted: Counter[str],
    total: int,
    top: int,
    locations: dict[str, Counter[str]] | None = None,
) -> str:
    """One ranked section of the report."""
    rows = counted.most_common(top)
    shown = f"top {len(rows)} of {len(counted)}" if len(counted) > len(rows) else f"all {len(counted)}"
    lines = [f"## {title} ({shown})", ""]
    for name, count in rows:
        where = ""
        if locations and locations.get(name):
            source = locations[name].most_common(1)[0][0]
            file, _, line = source.rpartition(":")
            where = f"  {Path(file).name}:{line}"
        lines.append(f"{100 * count / total:6.2f}%  {count:>7,}  {truncated(name, NAME_WIDTH)}{where}")
    return "\n".join(lines)


def stack_table(stacks: Counter[tuple[str, ...]], total: int, top: int) -> str:
    """The hottest call paths, cut to the frames nearest the leaf."""
    rows = stacks.most_common(top)
    lines = [f"## hottest stacks, by self time (top {len(rows)} of {len(stacks)})", ""]
    for path, count in rows:
        deepest = path[-STACK_DEPTH:]
        elided = "...;" if len(path) > len(deepest) else ""
        folded = ";".join(shortened(frame) for frame in deepest)
        lines.append(f"{100 * count / total:6.2f}%  {count:>7,}  {elided}{folded}")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description=DESCRIPTION, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("profile", type=Path, help="the profile.json samply wrote")
    parser.add_argument("--binary", type=Path, help="the profiled binary, if it isn't the profile's product")
    parser.add_argument("--label", default="", help="the command the profile was recorded from")
    parser.add_argument("--top", type=int, default=40, help="rows per symbol table")
    parser.add_argument("--stacks", type=int, default=20, help="rows in the stack table")
    arguments = parser.parse_args()

    profile = json.loads(arguments.profile.read_text())
    libraries = profile["libs"]
    product = profile["meta"]["product"]
    threads = [thread for thread in profile["threads"] if thread["samples"]["length"]]
    if not threads:
        sys.exit(f"{arguments.profile} holds no samples: did the command run long enough to sample?")

    binary = profiled(libraries, product, arguments.binary)
    threads = [thread for thread in threads if ran_binary(thread, libraries, binary)]
    addresses = sampled_addresses(threads, libraries, binary)
    chains = resolve(tool("addr2line", "llvm-addr2line"), binary["path"], addresses)
    reader = tool("nm", "llvm-nm")
    symbols = {
        library["path"]: exported(reader, library["path"])
        for library in libraries
        if library["path"] != binary["path"] and Path(library["path"]).exists()
    }
    frames = {
        order: frames_of(thread, libraries, chains, symbols, binary)
        for order, thread in enumerate(threads)
    }
    tallied = tally(threads, frames)

    total = sum(tallied.own.values())
    interval = profile["meta"]["interval"]
    processes = {thread["pid"] for thread in threads}
    print(arguments.label or product)
    print(
        f"{total:,} samples at {1000 / interval:g} Hz: {total * interval / 1000:.2f} s of CPU"
        f" across {len(threads)} thread(s) in {len(processes)} process(es)"
    )
    print(unwound(tallied.roots, total))
    for section in (
        symbol_table("self time by crate", tallied.by_crate, total, arguments.top),
        symbol_table("self time by symbol", tallied.own, total, arguments.top, tallied.locations),
        symbol_table("time on the stack, inclusive", tallied.inclusive, total, arguments.top),
        stack_table(tallied.stacks, total, arguments.stacks),
    ):
        print(f"\n{section}")


if __name__ == "__main__":
    main()
