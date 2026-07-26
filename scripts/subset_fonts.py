#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools>=4.55", "brotli>=1.1"]
# ///

from __future__ import annotations

import argparse
import io
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

DESCRIPTION = """\
Cut the vendored web fonts down to what a transcript actually sets.

A folio inlines every face as base64, so the faces are almost the whole file: a
short session renders to ~3 MB of which ~98% is font. Upstream ships coverage
and variation axes this project never asks for, and both are dead weight in
every folio ever written.

Two cuts, in order:

1. Pin the axes the stylesheet never varies. Junicode is variable on weight,
   width, and ENLA (enlarged minuscules); `illumination.css` only ever sets
   `font-weight`, so width and ENLA are pinned to their defaults and weight is
   held to the 300-700 the `@font-face` rule declares. No glyph is lost.
2. Subset to KEEP, below. Upstream Junicode carries 3162 codepoints for
   medieval scholarship; a transcript is Latin prose, code, and the punctuation
   and box-drawing a terminal emits.

Both cuts are written beside the whole faces rather than over them, because the
binary embeds both: a folio carries the cut faces unless its own text reaches a
character the cut dropped, and then it carries the whole ones instead. That
choice is what `dropped.txt` is for, so it records the *regression* (what a face
had upstream and lost here) rather than what the cut faces cover. A character no
face ever carried, an emoji or a CJK ideograph, is not a regression: it fell
back to the reader's own fonts before this script existed and still does.

UnifrakturCook is copied through untouched. It is 25 KB, so there is nothing to
win, and it is the one face whose OFL declares a Reserved Font Name, which a
modified copy could not keep.
"""

# The blocks real transcripts reach for, measured over the corpus under
# ~/.claude/projects rather than guessed: Latin and its punctuation, the
# arrows and math a model writes in prose, and the box drawing, block elements,
# geometric shapes, and braille that CLI tools draw tables and spinners with.
# Greek earns its place (delta in diffs and maths); Cyrillic has no evidence
# behind it and costs ~50 KB, so it is left out until a render says otherwise.
#
# Widening this is cheap and safe: a codepoint a face never had is simply not
# kept, and the whole-face fallback covers anything cut too far.
KEEP: tuple[tuple[int, int], ...] = (
    (0x0000, 0x00FF),  # Basic Latin, Latin-1 Supplement
    (0x0100, 0x017F),  # Latin Extended-A
    (0x02B0, 0x02FF),  # Spacing Modifier Letters
    (0x0300, 0x036F),  # Combining Diacritical Marks
    (0x0370, 0x03FF),  # Greek and Coptic
    (0x2000, 0x206F),  # General Punctuation
    (0x2070, 0x209F),  # Superscripts and Subscripts
    (0x20A0, 0x20BF),  # Currency Symbols
    (0x2100, 0x214F),  # Letterlike Symbols
    (0x2150, 0x218F),  # Number Forms
    (0x2190, 0x21FF),  # Arrows
    (0x2200, 0x22FF),  # Mathematical Operators
    (0x2300, 0x23FF),  # Miscellaneous Technical
    (0x2400, 0x243F),  # Control Pictures
    (0x2500, 0x257F),  # Box Drawing
    (0x2580, 0x259F),  # Block Elements
    (0x25A0, 0x25FF),  # Geometric Shapes
    (0x2600, 0x26FF),  # Miscellaneous Symbols
    (0x2700, 0x27BF),  # Dingbats
    (0x2800, 0x28FF),  # Braille Patterns
    (0xFB00, 0xFB06),  # Latin ligatures
    (0xFEFF, 0xFEFF),  # Byte order mark
    (0xFFFD, 0xFFFD),  # Replacement character
)


@dataclass(frozen=True, slots=True)
class Face:
    """One vendored face and how to cut it down."""

    file: str
    # Axes to pin away (None) or hold to a range. Empty when the face is not
    # variable.
    axes: dict[str, tuple[int, int] | None]
    # UnifrakturCook is small enough that cutting it wins nothing, and its OFL
    # reserves the family name against modified copies.
    cut: bool = True


FACES: tuple[Face, ...] = (
    Face("JunicodeVF-Roman.woff2", {"wdth": None, "ENLA": None, "wght": (300, 700)}),
    Face("JunicodeVF-Italic.woff2", {"wdth": None, "ENLA": None, "wght": (300, 700)}),
    Face("FiraCode-VF.woff2", {"wght": (300, 700)}),
    Face("UnifrakturCook.woff2", {}, cut=False),
)


def keep_codepoints() -> list[int]:
    return [cp for low, high in KEEP for cp in range(low, high + 1)]


def instantiate(font: TTFont, axes: dict[str, tuple[int, int] | None]) -> TTFont:
    """Pin or narrow `axes`, returning a font whose tables are safe to subset.

    Subsetting `gvar` in memory straight after instancing trips a fontTools
    lazy-load bug (`KeyError` on a glyph the instancer dropped), so the font is
    written out and read back before it goes anywhere near the subsetter.
    """
    instancer.instantiateVariableFont(font, axes, inplace=True)
    buffer = io.BytesIO()
    font.flavor = None
    font.save(buffer)
    buffer.seek(0)
    return TTFont(buffer)


def cut_face(source: Path, destination: Path, face: Face) -> set[int]:
    """Cut one face down, returning the codepoints it lost in the process."""
    before = set(TTFont(source).getBestCmap())

    font = TTFont(source)
    if face.axes:
        font = instantiate(font, face.axes)
    subsetter = subset.Subsetter(options=subset.Options())
    subsetter.populate(unicodes=keep_codepoints())
    subsetter.subset(font)
    font.flavor = "woff2"
    font.save(destination)

    after = set(TTFont(destination).getBestCmap())
    return before - after


def as_ranges(codepoints: set[int]) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    for codepoint in sorted(codepoints):
        if ranges and codepoint == ranges[-1][1] + 1:
            ranges[-1] = (ranges[-1][0], codepoint)
        else:
            ranges.append((codepoint, codepoint))
    return ranges


def write_dropped(path: Path, dropped: set[int]) -> list[tuple[int, int]]:
    ranges = as_ranges(dropped)
    lines = [
        "# Codepoints an embedded face carried upstream and the cut one lost.",
        "#",
        "# Recovered from the fonts' own cmaps by `just fonts`, never edited by",
        "# hand. `build.rs` compiles these into the binary; a folio whose text",
        "# reaches any of them carries the whole faces instead of the cut ones,",
        "# so cutting a face can never render a character worse than upstream.",
        "#",
        "# A character no face ever had (an emoji, a CJK ideograph) is absent",
        "# here: it falls back to the reader's own fonts, as it always did.",
        "",
    ]
    lines += [f"{low:04X}-{high:04X}" for low, high in ranges]
    path.write_text("\n".join(lines) + "\n", encoding="utf8")
    return ranges


def main() -> int:
    parser = argparse.ArgumentParser(
        description=DESCRIPTION, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("source", type=Path, help="directory holding the upstream faces")
    parser.add_argument("destination", type=Path, help="fonts directory to write into")
    args = parser.parse_args()

    cut_dir = args.destination / "cut"
    cut_dir.mkdir(parents=True, exist_ok=True)
    dropped: set[int] = set()

    for face in FACES:
        source = args.source / face.file
        shutil.copyfile(source, args.destination / face.file)
        whole = source.stat().st_size

        if not face.cut:
            shutil.copyfile(source, cut_dir / face.file)
            print(f"  {face.file:24} {whole:>9,} bytes, kept whole", file=sys.stderr)
            continue

        dropped |= cut_face(source, cut_dir / face.file, face)
        cut = (cut_dir / face.file).stat().st_size
        print(
            f"  {face.file:24} {whole:>9,} -> {cut:>9,} bytes ({100 - cut * 100 // whole}% off)",
            file=sys.stderr,
        )

    ranges = write_dropped(cut_dir / "dropped.txt", dropped)
    print(
        f"  {'dropped.txt':24} {len(dropped):,} codepoints in {len(ranges)} ranges",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
