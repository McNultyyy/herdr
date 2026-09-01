#!/usr/bin/env python3
"""Print the wire protocol version this checkout speaks.

Used by .github/workflows/fork-build.yml to stamp the fork's update manifest,
which `herdr --remote` compares against the client's own protocol.
"""

from __future__ import annotations

import pathlib
import re
import sys

WIRE = pathlib.Path("src/protocol/wire.rs")


def main() -> int:
    match = re.search(
        r"pub const PROTOCOL_VERSION: u32 = (\d+);",
        WIRE.read_text(encoding="utf-8"),
    )
    if match is None:
        print(f"no PROTOCOL_VERSION in {WIRE}", file=sys.stderr)
        return 1
    print(match.group(1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
