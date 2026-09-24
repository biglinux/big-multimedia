"""Whitespace-insensitive source text for the static checkers.

The contracts below match literal fragments of Rust source. The workspace is
formatted with rustfmt (`cargo fmt --all --check` is a release gate), so any
fragment may be re-wrapped across lines at any time without its meaning
changing. Matching on whitespace-stripped text keeps the contracts about what
the code says rather than how it is wrapped.
"""

from __future__ import annotations

import re

_SPACE = re.compile(r"\s+")


def compact(text: str) -> str:
    """Drop every whitespace character so wrapping cannot change a match."""
    return _SPACE.sub("", text)


class Source(str):
    """Source text whose substring operations ignore whitespace.

    The instance *is* the compacted text, so positions returned by `find` are
    comparable with each other — which is all the ordering contracts need.
    """

    def __new__(cls, text: str):
        return super().__new__(cls, compact(text))

    def __contains__(self, needle: object) -> bool:
        return str.__contains__(self, compact(str(needle)))

    def find(self, needle, start=0, end=None) -> int:  # type: ignore[override]
        needle = compact(str(needle))
        return (
            str.find(self, needle, start)
            if end is None
            else str.find(self, needle, start, end)
        )

    def __getitem__(self, key):
        sliced = str.__getitem__(self, key)
        return Source(sliced) if isinstance(key, slice) else sliced

    def index(self, needle, start=0, end=None) -> int:  # type: ignore[override]
        found = self.find(needle, start, end)
        if found < 0:
            raise ValueError("substring not found")
        return found

    def split(self, sep=None, maxsplit=-1):  # type: ignore[override]
        if sep is None:
            return str.split(self, sep, maxsplit)
        return [Source(part) for part in str.split(self, compact(str(sep)), maxsplit)]
