#!/usr/bin/env python3
"""Strip service method declarations from a Candid .did file.

Removes the declarations of service methods whose names are listed in a
config file (one per line; blank lines and `#` comments ignored) and
writes the stripped interface to stdout.

Used by the `candid-backward-compat` CI job to exclude endpoints that
ship without a backwards-compatibility guarantee. Stripping a method
declaration makes its exclusive transitive types (those reachable only
through that method) unreferenced, and `didc check` ignores unreferenced
types — so changes to those types no longer gate the PR.

Only declarations inside the `service : ... { ... }` block are removed;
top-level `type` definitions and record fields are left untouched. A
method declaration may span multiple lines; stripping runs from the line
that opens the declaration until its brackets close and the terminating
`;` is consumed.

Usage: strip_candid_methods.py <did_file> <endpoints_conf>
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


def load_names(conf: Path) -> set[str]:
    names: set[str] = set()
    for raw in conf.read_text().splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            names.add(line)
    return names


def bracket_delta(line: str) -> int:
    delta = 0
    for ch in line:
        if ch in "({[":
            delta += 1
        elif ch in ")}]":
            delta -= 1
    return delta


def strip(did_text: str, names: set[str]) -> str:
    out: list[str] = []
    in_service = False
    service_depth = 0
    skip_depth = 0
    name_re = re.compile(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*:\s*")

    for line in did_text.splitlines(keepends=True):
        if not in_service:
            out.append(line)
            if re.match(r"^\s*service\b", line):
                in_service = True
                service_depth = bracket_delta(line)
            continue

        if skip_depth > 0:
            skip_depth += bracket_delta(line)
            if skip_depth <= 0:
                skip_depth = 0
            continue

        m = name_re.match(line)
        if m and m.group(1) in names:
            skip_depth = bracket_delta(line)
            # A declaration whose brackets balance on the opening line still
            # ends with `;` on that same line — drop it entirely.
            if skip_depth == 0:
                continue
            continue

        out.append(line)
        service_depth += bracket_delta(line)
        if service_depth <= 0:
            in_service = False
            service_depth = 0

    return "".join(out)


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(f"usage: {argv[0]} <did_file> <endpoints_conf>", file=sys.stderr)
        return 2
    did_path = Path(argv[1])
    conf_path = Path(argv[2])
    names = load_names(conf_path)
    stripped = strip(did_path.read_text(), names)
    sys.stdout.write(stripped)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
