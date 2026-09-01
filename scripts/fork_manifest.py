#!/usr/bin/env python3
"""Fork-only helpers for the update manifest published by fork-build.yml.

`scripts/preview.py` builds the manifest itself; these are the two edits the
fork needs around it, kept out of the workflow YAML so they can be run and
tested locally.
"""

from __future__ import annotations

import argparse
import json
import pathlib

WINDOWS_TARGET = "windows-x86_64"


def load(path: pathlib.Path) -> dict:
    if not path.exists():
        return {}
    text = path.read_text(encoding="utf-8").strip()
    return json.loads(text) if text else {}


def cmd_previous_commit(args: argparse.Namespace) -> int:
    """Print the commit the currently published manifest was built from.

    Prints nothing when there is no manifest yet, so the first fork build still
    produces release notes.
    """
    print(load(pathlib.Path(args.manifest)).get("commit", ""))
    return 0


def cmd_keep_windows(args: argparse.Namespace) -> int:
    """Drop every asset target this fork does not build.

    preview.py always emits all five upstream targets; the fork only publishes
    a Windows zip, and the missing URLs would 404 for anyone who pointed a
    Linux or macOS install at this manifest.
    """
    path = pathlib.Path(args.manifest)
    manifest = load(path)

    def only_windows(assets: dict) -> dict:
        return {WINDOWS_TARGET: assets[WINDOWS_TARGET]} if WINDOWS_TARGET in assets else {}

    manifest["assets"] = only_windows(manifest.get("assets", {}))
    for build in manifest.get("builds", {}).values():
        build["assets"] = only_windows(build.get("assets", {}))
    if not manifest["assets"]:
        raise SystemExit(f"{path} has no {WINDOWS_TARGET} asset")
    path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)

    previous = subcommands.add_parser(
        "previous-commit", help="print the commit of the published manifest"
    )
    previous.add_argument("--manifest", required=True)
    previous.set_defaults(func=cmd_previous_commit)

    keep = subcommands.add_parser(
        "keep-windows", help="strip asset targets this fork does not build"
    )
    keep.add_argument("--manifest", required=True)
    keep.set_defaults(func=cmd_keep_windows)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
