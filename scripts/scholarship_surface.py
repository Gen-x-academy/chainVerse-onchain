#!/usr/bin/env python3
"""Derive and gate the scholarship contracts' public interface.

Soroban contracts have no source-level semver: a renamed function, a
reordered parameter, or a changed event topic is invisible in a diff review
and breaks every deployed client and indexer at once. This script derives a
normalised surface (public functions with their exact parameter lists, and
event topics with payload arity) straight from the sources, so the gate works
without a working Rust toolchain, and compares it against a committed
baseline.

Breaking: a function or event disappears, a signature changes, a topic is
renamed, or payload arity changes. Everything else is additive and allowed.

Approving a breaking change is deliberate, not incidental: write an override
naming each accepted change and the migration note that justifies it. The
override is reviewable in the same PR as the change, so "why is this allowed"
is answerable from the diff alone.

Usage:
    scholarship_surface.py --write
    scholarship_surface.py --check            # exit 1 on unapproved breaking change
    scholarship_surface.py --report           # human-readable inventory
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONTRACTS_DIR = os.path.join(REPO_ROOT, "contracts")
CONTRACT_GLOB = "scholarship-*"

DEFAULT_BASELINE = os.path.join(CONTRACTS_DIR, "scholarship-api-baseline.json")
DEFAULT_OVERRIDE = os.path.join(REPO_ROOT, ".github", "scholarship-compat-override.json")

# A function that exists only to be called from tests.
TEST_ONLY = re.compile(r"^(test_|__)")


def read(path: str) -> str:
    with open(path, "r", encoding="utf-8") as handle:
        return handle.read()


def split_top_level(text: str) -> list:
    """Split on commas that are not nested inside brackets or braces."""
    parts, depth, current = [], 0, ""
    for char in text:
        if char in "([{<":
            depth += 1
        elif char in ")]}>":
            depth -= 1
        if char == "," and depth == 0:
            parts.append(current.strip())
            current = ""
        else:
            current += char
    if current.strip():
        parts.append(current.strip())
    return parts


def matching_paren(text: str, start: int) -> int:
    """Index of the ')' matching the '(' at `start`, or -1."""
    depth = 0
    for index in range(start, len(text)):
        if text[index] == "(":
            depth += 1
        elif text[index] == ")":
            depth -= 1
            if depth == 0:
                return index
    return -1


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def parse_functions(source: str, label: str = "<source>") -> dict:
    """Public functions reachable through #[contractimpl], keyed by name.

    The `env: Env` first parameter is not part of the on-chain interface and
    is dropped, so a cosmetic change to how a contract names it is not
    reported as a breaking change.
    """
    source = strip_comments(source)
    functions = {}

    for match in re.finditer(r"#\[contractimpl[^\]]*\]", source):
        region = source[match.end():]
        # The macro applies to a single impl block.
        block_end = region.find("\n#[")
        if block_end != -1:
            region = region[:block_end]

        for fn in re.finditer(r"\bpub\s+fn\s+([A-Za-z0-9_]+)\s*\(", region):
            name = fn.group(1)
            if TEST_ONLY.match(name):
                continue
            open_paren = region.index("(", fn.start())
            close_paren = matching_paren(region, open_paren)
            if close_paren == -1:
                # Silently skipping would make this function look removed,
                # which is a false alarm at best and a missed real change at
                # worst. Stop instead.
                raise ValueError(
                    f"{label}: cannot parse the parameter list of `{name}`. "
                    "The contract source is likely malformed; the interface "
                    "cannot be compared until it is fixed."
                )
            params_src = region[open_paren + 1:close_paren]
            params = []
            for param in split_top_level(params_src):
                param = re.sub(r"\s+", " ", param).strip()
                if not param:
                    continue
                if re.match(r"^_?env\s*:\s*Env$", param):
                    continue  # not part of the public interface
                params.append(param)

            tail = region[close_paren + 1:close_paren + 400]
            body = tail.find("{")
            semi = tail.find(";")
            if semi != -1 and (body == -1 or semi < body):
                signature_end = semi
            elif body != -1:
                signature_end = body
            else:
                signature_end = len(tail)
            ret = tail[:signature_end]
            ret = ret.split("->", 1)[1] if "->" in ret else ""
            ret = re.sub(r"\s+", " ", ret).strip()

            functions[name] = {"params": params, "returns": ret}

    return functions


def parse_events(source: str) -> dict:
    """Event topics published by this contract, with payload arity.

    Handles `env.events().publish((symbol_short!("X"),), (a, b))` written
    across several lines, which a line-oriented scan silently misses.
    """
    source = strip_comments(source)
    events = {}

    for match in re.finditer(r"\.publish\s*\(", source):
        open_paren = source.index("(", match.start())
        close_paren = matching_paren(source, open_paren)
        if close_paren == -1:
            continue
        call = source[open_paren + 1:close_paren]

        # First group is topics, remainder is the data payload.
        first_open = call.find("(")
        if first_open == -1:
            continue
        first_close = matching_paren(call, first_open)
        if first_close == -1:
            continue
        topics = re.findall(r'symbol_short!\("([A-Z]{3,12})"\)', call[first_open:first_close])
        if not topics:
            continue

        data = call[first_close + 1:].lstrip(" ,\n\t")
        arity = 0
        if data.startswith("("):
            data_close = matching_paren(data, 0)
            if data_close != -1:
                arity = len(split_top_level(data[1:data_close]))

        for topic in topics:
            events[topic] = arity

    return events


def contract_name(path: str) -> str:
    return os.path.basename(path)


def find_contract_root(manifest_dir: str) -> str:
    with open(os.path.join(manifest_dir, "Cargo.toml"), "r", encoding="utf-8") as handle:
        for line in handle:
            stripped = line.strip()
            if stripped.startswith("name"):
                return stripped.split("=", 1)[1].strip().strip('"')
    return os.path.basename(manifest_dir)


def collect() -> dict:
    """Build the normalised surface for every scholarship contract."""
    surface = {}
    if not os.path.isdir(CONTRACTS_DIR):
        return surface

    for entry in sorted(os.listdir(CONTRACTS_DIR)):
        manifest_dir = os.path.join(CONTRACTS_DIR, entry)
        lib = os.path.join(manifest_dir, "src", "lib.rs")
        if not entry.startswith("scholarship-") or not os.path.isfile(lib):
            continue
        source = read(lib)
        name = find_contract_root(manifest_dir)
        surface[name] = {
            "functions": parse_functions(source, label=name),
            "events": parse_events(source),
        }
    return surface


def find_consumers(surface: dict) -> dict:
    """Map each entry point to the places in the repo that reference it.

    A removed function is only interesting if something used it, so the
    report names the callers rather than just naming the function.
    """
    consumers = {}
    try:
        tracked = subprocess.run(
            ["git", "ls-files"],
            cwd=REPO_ROOT, capture_output=True, text=True, check=True,
        ).stdout.split()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return consumers

    cache = {}
    for name, contract in surface.items():
        for fn in contract["functions"]:
            # Generated clients are used through whatever alias the caller
            # imported, e.g. `ScholarshipCoreContractClient::create_program(`,
            # and cross-contract calls go through the underscored crate path.
            crate = name.replace("-", "_")
            pattern = re.compile(
                rf"\b[A-Za-z0-9_]*Client::\s*{re.escape(fn)}\b"
                rf"|\b{re.escape(crate)}::\s*{re.escape(fn)}\b"
            )
            for path in tracked:
                if not path.endswith((".rs", ".sh", ".md", ".ts", ".js")):
                    continue
                full = os.path.join(REPO_ROOT, path)
                if full not in cache:
                    try:
                        cache[full] = read(full)
                    except (OSError, UnicodeDecodeError):
                        cache[full] = ""
                if pattern.search(cache[full]):
                    consumers.setdefault(f"{name}.{fn}", []).append(path)

    return {key: sorted(set(value)) for key, value in consumers.items()}


def diff_surfaces(old: dict, new: dict) -> list:
    """Classify differences as 'breaking' or 'additive'."""
    changes = []

    for name in sorted(set(old) | set(new)):
        if name not in old:
            changes.append({"kind": "additive", "scope": name,
                            "detail": "new contract", "key": name})
            continue
        if name not in new:
            changes.append({"kind": "breaking", "scope": name,
                            "detail": "contract removed", "key": name})
            continue

        old_c, new_c = old[name], new[name]

        for fn in sorted(set(old_c["functions"]) | set(new_c["functions"])):
            key = f"{name}.{fn}"
            if fn not in old_c["functions"]:
                changes.append({"kind": "additive", "scope": name,
                                "detail": f"new function `{fn}`", "key": key})
            elif fn not in new_c["functions"]:
                changes.append({"kind": "breaking", "scope": name,
                                "detail": f"function `{fn}` removed", "key": key})
            else:
                o, n = old_c["functions"][fn], new_c["functions"][fn]
                if o["params"] != n["params"]:
                    changes.append({"kind": "breaking", "scope": name,
                                    "detail": f"`{fn}` parameters changed: "
                                              f"{o['params']} -> {n['params']}", "key": key})
                if o["returns"] != n["returns"]:
                    changes.append({"kind": "breaking", "scope": name,
                                    "detail": f"`{fn}` return type changed: "
                                              f"{o['returns'] or '()'} -> {n['returns'] or '()'}",
                                    "key": key})

        for topic in sorted(set(old_c["events"]) | set(new_c["events"])):
            key = f"{name}.{topic}"
            if topic not in old_c["events"]:
                changes.append({"kind": "additive", "scope": name,
                                "detail": f"new event `{topic}`", "key": key})
            elif topic not in new_c["events"]:
                changes.append({"kind": "breaking", "scope": name,
                                "detail": f"event `{topic}` removed", "key": key})
            elif old_c["events"][topic] != new_c["events"][topic]:
                changes.append({"kind": "breaking", "scope": name,
                                "detail": f"event `{topic}` payload arity changed: "
                                          f"{old_c['events'][topic]} -> {new_c['events'][topic]}",
                                "key": key})

    return changes


def load_override(path: str) -> tuple:
    """Return (approved keys, approvals needing a migration note)."""
    if not os.path.isfile(path):
        return {}, []
    try:
        data = json.loads(read(path))
    except json.JSONDecodeError as error:
        print(f"error: override file is not valid JSON: {error}", file=sys.stderr)
        return {}, []
    approvals = data.get("approvals", [])
    approved = {a["key"]: a for a in approvals if isinstance(a, dict) and "key" in a}
    incomplete = [a for a in approvals if not str(a.get("migration", "")).strip()]
    return approved, incomplete


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--write", action="store_true",
                        help="write the baseline to --baseline and exit")
    parser.add_argument("--check", action="store_true", help="fail on unapproved breaking changes")
    parser.add_argument("--report", action="store_true", help="print the surface inventory")
    parser.add_argument("--baseline", default=DEFAULT_BASELINE)
    parser.add_argument("--override", default=DEFAULT_OVERRIDE)
    args = parser.parse_args()

    try:
        surface = collect()
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    if not surface:
        print("error: no scholarship contracts found", file=sys.stderr)
        return 2

    if args.write:
        payload = {"schema": 1, "contracts": surface}
        os.makedirs(os.path.dirname(args.baseline), exist_ok=True)
        with open(args.baseline, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True)
            handle.write("\n")
        total_f = sum(len(c["functions"]) for c in surface.values())
        total_e = sum(len(c["events"]) for c in surface.values())
        print(f"wrote {args.baseline}")
        print(f"  {len(surface)} contracts, {total_f} functions, {total_e} events")
        return 0

    if args.report:
        consumers = find_consumers(surface)
        for name in sorted(surface):
            contract = surface[name]
            print(f"\n{name}  ({len(contract['functions'])} functions, "
                  f"{len(contract['events'])} events)")
            for fn in sorted(contract["functions"]):
                spec = contract["functions"][fn]
                ret = f" -> {spec['returns']}" if spec["returns"] else ""
                print(f"    fn {fn}({', '.join(spec['params'])}){ret}")
            for topic in sorted(contract["events"]):
                print(f"    evt {topic}  payload={contract['events'][topic]}")
        if consumers:
            print("\nconsumers")
            for key in sorted(consumers):
                print(f"    {key} <- {', '.join(consumers[key][:3])}")
        return 0

    if not os.path.isfile(args.baseline):
        print(f"error: no baseline at {args.baseline}", file=sys.stderr)
        print("       run: scripts/scholarship_surface.py --write", file=sys.stderr)
        return 2

    baseline = json.loads(read(args.baseline)).get("contracts", {})
    changes = diff_surfaces(baseline, surface)
    breaking = [c for c in changes if c["kind"] == "breaking"]
    additive = [c for c in changes if c["kind"] == "additive"]
    approved, incomplete = load_override(args.override)

    unapproved = [c for c in breaking if c["key"] not in approved]
    suppressed = [c for c in breaking if c["key"] in approved]

    for change in additive:
        print(f"  additive  {change['scope']}: {change['detail']}")
    for change in suppressed:
        note = approved[change["key"]].get("migration", "(no note)")
        print(f"  approved  {change['scope']}: {change['detail']}  [{note}]")
    for change in unapproved:
        print(f"  BREAKING  {change['scope']}: {change['detail']}")

    print(f"\n{len(additive)} additive, {len(breaking)} breaking "
          f"({len(suppressed)} approved, {len(unapproved)} unapproved)")

    if incomplete:
        print("\nerror: override entries missing a `migration` note:", file=sys.stderr)
        for entry in incomplete:
            print(f"    {entry.get('key')}", file=sys.stderr)
        return 1

    failed = False

    if unapproved:
        failed = True
        print("\nBreaking interface changes need an explicit approval.", file=sys.stderr)
        print(f"  Add each `key` to {os.path.relpath(args.override, REPO_ROOT)} "
              f"with a `migration` note, or restore the interface.", file=sys.stderr)

    if additive:
        # A stale baseline hides the next removal: the function was added
        # after the baseline was written, so deleting it later compares equal.
        # Requiring the baseline to be regenerated keeps it describing the
        # interface as it is now, not as it was when it was last updated.
        failed = True
        print("\nThe baseline does not describe the current interface.", file=sys.stderr)
        print("  Run: scripts/scholarship_surface.py --write", file=sys.stderr)
        print("  and commit the result alongside the change.", file=sys.stderr)

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
