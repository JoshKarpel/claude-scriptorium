#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///

import argparse
import json
import random
import sys
from pathlib import Path

DESCRIPTION = """\
Write a large synthetic session, for benchmarking a render at scale.

The turns are drawn from the committed fixtures rather than invented, so the
shapes are the harness's own: every built-in tool's call and result, prose,
thinking, and code in the languages the corpus actually carries. They are dealt
out in a seeded order and stamped with fresh ids and timestamps until the file
reaches the size asked for, so the same arguments always write the same session.

A session this long measures a different thing from the small fixtures. Syntax
highlighting compiles a language's regexes once, so a session that meets its
languages early and then runs for megabytes is dominated by the per-byte work
(matching, markup, allocation) where a short one is dominated by that one-time
compile. Benchmark both, and expect a change to move them differently.
"""

MEGABYTE = 1024 * 1024
SOURCES = ("playground.jsonl", "session.jsonl", "answers.jsonl")


def turns(fixtures: Path) -> list[list[dict]]:
    """The fixtures' turns, grouped so a tool's call keeps its result.

    A tool result is a `user` turn that answers the `tool_use` before it, so the
    two are dealt as one unit; anything else stands alone.
    """
    grouped: list[list[dict]] = []
    for source in SOURCES:
        for line in (fixtures / source).read_text().splitlines():
            entry = json.loads(line)
            if grouped and answers_a_call(entry):
                grouped[-1].append(entry)
            else:
                grouped.append([entry])
    return grouped


def answers_a_call(entry: dict) -> bool:
    content = entry.get("message", {}).get("content")
    if entry.get("type") != "user" or not isinstance(content, list):
        return False
    return any(block.get("type") == "tool_result" for block in content)


def restamp(unit: list[dict], ordinal: int, second: int) -> list[dict]:
    """One unit of turns, with ids and timestamps of its own.

    A tool result names the call it answers, so both sides of the pair are
    renumbered together; a result whose call is elsewhere would be stamped with
    an id nothing answers, and the renderer would have no tool to name it by.
    """
    calls: dict[str, str] = {}
    stamped = []
    for offset, entry in enumerate(unit):
        turn = json.loads(json.dumps(entry))
        turn["timestamp"] = timestamp(second + offset)
        message = turn.get("message")
        if isinstance(message, dict):
            if "id" in message:
                message["id"] = f"msg_{ordinal}_{offset}"
            renumber(message.get("content"), ordinal, calls)
            if isinstance(message.get("content"), str):
                message["content"] = f"[turn {ordinal}] {message['content']}"
        stamped.append(turn)
    return stamped


def renumber(content: object, ordinal: int, calls: dict[str, str]) -> None:
    if not isinstance(content, list):
        return
    for index, block in enumerate(content):
        if not isinstance(block, dict):
            continue
        if block.get("type") == "tool_use":
            calls[block["id"]] = f"call_{ordinal}_{index}"
            block["id"] = calls[block["id"]]
        if block.get("type") == "tool_result":
            answered = block.get("tool_use_id")
            block["tool_use_id"] = calls.get(answered, f"call_{ordinal}_{index}")
        if block.get("type") == "text" and isinstance(block.get("text"), str):
            block["text"] = f"[turn {ordinal}] {block['text']}"


def timestamp(second: int) -> str:
    """A stamp that climbs through the session, an hour of turns per day."""
    day, rest = divmod(second, 86400)
    hour, rest = divmod(rest, 3600)
    minute, second = divmod(rest, 60)
    return f"2026-{1 + day // 28:02d}-{1 + day % 28:02d}T{hour:02d}:{minute:02d}:{second:02d}Z"


def deal(units: list[list[dict]], budget: int, seed: int) -> list[str]:
    """Deal units until the session reaches its budget, reshuffling each pass."""
    shuffler = random.Random(seed)
    lines: list[str] = []
    written = ordinal = second = 0
    deck: list[list[dict]] = []
    while written < budget:
        if not deck:
            deck = list(units)
            shuffler.shuffle(deck)
        unit = deck.pop()
        for turn in restamp(unit, ordinal, second):
            line = json.dumps(turn)
            lines.append(line)
            written += len(line) + 1
        ordinal += 1
        second += len(unit) + shuffler.randrange(1, 30)
    return lines


def main() -> None:
    parser = argparse.ArgumentParser(
        description=DESCRIPTION, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--megabytes", type=float, required=True, help="how large to make it")
    parser.add_argument("--output", type=Path, required=True, help="where to write it")
    parser.add_argument("--fixtures", type=Path, default=Path("tests/fixtures"))
    parser.add_argument("--seed", type=int, default=1387, help="which session to write")
    arguments = parser.parse_args()

    units = turns(arguments.fixtures)
    lines = deal(units, int(arguments.megabytes * MEGABYTE), arguments.seed)
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text("\n".join(lines) + "\n")
    written = arguments.output.stat().st_size
    print(f"{arguments.output}: {len(lines):,} turns, {written / MEGABYTE:.1f} MB", file=sys.stderr)


if __name__ == "__main__":
    main()
