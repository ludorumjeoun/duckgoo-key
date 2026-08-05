#!/usr/bin/env python3
"""Compare two SemVer values without introducing a packaging dependency."""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass


SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)"
    r"(?:-((?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


@dataclass(frozen=True)
class Version:
    core: tuple[int, int, int]
    prerelease: tuple[str, ...] | None

    @classmethod
    def parse(cls, raw: str) -> "Version":
        match = SEMVER.fullmatch(raw)
        if match is None:
            raise ValueError(f"invalid SemVer: {raw}")
        prerelease = tuple(match.group(4).split(".")) if match.group(4) else None
        return cls(
            core=(int(match.group(1)), int(match.group(2)), int(match.group(3))),
            prerelease=prerelease,
        )


def compare_identifiers(left: str, right: str) -> int:
    if left == right:
        return 0
    left_numeric = left.isdigit()
    right_numeric = right.isdigit()
    if left_numeric and right_numeric:
        return -1 if int(left) < int(right) else 1
    if left_numeric != right_numeric:
        return -1 if left_numeric else 1
    return -1 if left < right else 1


def compare(left: Version, right: Version) -> int:
    if left.core != right.core:
        return -1 if left.core < right.core else 1
    if left.prerelease is None or right.prerelease is None:
        if left.prerelease == right.prerelease:
            return 0
        return 1 if left.prerelease is None else -1

    for left_part, right_part in zip(left.prerelease, right.prerelease):
        result = compare_identifiers(left_part, right_part)
        if result != 0:
            return result
    if len(left.prerelease) == len(right.prerelease):
        return 0
    return -1 if len(left.prerelease) < len(right.prerelease) else 1


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: compare-semver.py LEFT RIGHT", file=sys.stderr)
        return 2
    try:
        result = compare(Version.parse(sys.argv[1]), Version.parse(sys.argv[2]))
    except ValueError as error:
        print(f"compare-semver: {error}", file=sys.stderr)
        return 2
    print(result)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
