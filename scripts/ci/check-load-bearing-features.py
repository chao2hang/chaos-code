#!/usr/bin/env python3
"""Fail when a feature this repository's behaviour depends on stops reaching a crate.

Cargo unifies features per build *and per target*. A dependency feature that one
code path enables therefore arrives through whichever crates happen to be in the
build for the target being built, and a target-gated dependency can silently
withdraw it. The result is a crate whose code is compiled identically on every
platform but linked against a dependency with different capabilities, which no
`cfg` and no compiler error can show you.

That is not hypothetical. `xai-grok-tools` builds MCP elicitation forms by
walking a JSON Schema `properties` object, which only keeps the schema's order if
`serde_json` is built with `preserve_order`; without it the map is a `BTreeMap`
and every form renders alphabetically. Nothing in `xai-grok-tools/Cargo.toml`
asked for that feature. It arrived through `xai-grok-sandbox`, whose `nono`
dependency requests it, and `nono` is declared under `[target.'cfg(unix)'.
dependencies]`. So the schema-order test was green on Linux and red on the
Windows CI leg, where it reported `["alpha", "zeta"]` instead of
`["zeta", "alpha"]`, and the diff between the two runs was a feature that only
existed on one of them.

`cargo tree -e features --target <triple>` shows the whole tree, and diffing
those trees across targets found 58 differences on the current one. Every one of
them is deliberate: `nix`'s per-OS feature lists, `tokio`'s `windows-sys`, the
`sdk`-shaped feature sets of platform-only dependencies. A gate over all of them
would need 58 exemptions and would then be ignored, so this script does not look
for every difference. It checks the ones this repository has written down as
load-bearing: one row per behaviour that depends on a dependency being built a
certain way, in `scripts/ci/load-bearing-features.tsv`.

What it enforces per row (`crate`, `dependency`, `feature`, `targets`, `why`):

- the row's target list names at least two targets, because a comparison over one
  target cannot fail;
- every listed target is installed, checked against `rustup target list
  --installed`. A missing target is a failure, not a skip: a guard that degrades
  to "nothing checked" on a machine without the tier-2 targets installed is the
  same silence that hid the `preserve_order` bug;
- for each target, `cargo tree -p <crate> -e features -i <dependency>` reports
  `<dependency> feature "<feature>"` as a child of the inverted root, i.e. the
  feature really is enabled for that crate on that target;
- the dependency is present at all on each target, and the feature is enabled on
  at least one target -- a row asking about a feature nothing enables is stale,
  and a table of stale rows reads like coverage.

A row is added when someone discovers that behaviour depends on a feature. A row
is removed when the crate declares the feature itself and the dependency can no
longer lose it -- which is what happened here, and the row stays because a
declaration can be dropped by the next person who tidies `Cargo.toml`.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

CONFIG = "scripts/ci/load-bearing-features.tsv"

# `cargo tree -e features` prints one node per line with tree glyphs. A feature
# node is self-describing (`serde_json feature "preserve_order"`), so the label
# alone identifies it; the leading glyphs only have to be stripped.
FEATURE_NODE = re.compile(r'^(?P<pkg>[A-Za-z0-9_.+-]+) feature "(?P<feature>[^"]+)"$')
PACKAGE_NODE = re.compile(r"^(?P<pkg>[A-Za-z0-9_.+-]+) v[^ ]+")

Row = tuple[str, str, str, list[str], str]


class ConfigError(Exception):
    """The table itself is malformed, which is a different bug from a failure."""


def parse_config(text: str) -> list[Row]:
    rows: list[Row] = []
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        fields = raw.split("\t")
        if len(fields) != 5:
            raise ConfigError(
                f"line {lineno}: expected 5 tab-separated fields "
                f"(crate, dependency, feature, targets, why), got {len(fields)}"
            )
        crate, dep, feature, targets, why = (f.strip() for f in fields)
        if not all((crate, dep, feature, targets, why)):
            raise ConfigError(f"line {lineno}: an empty field")
        target_list = [t.strip() for t in targets.split(",") if t.strip()]
        if len(target_list) < 2:
            raise ConfigError(
                f"line {lineno}: `{crate}`/`{dep}` lists {len(target_list)} target(s); "
                "a feature comparison over one target cannot fail, so it proves nothing"
            )
        if len(set(target_list)) != len(target_list):
            raise ConfigError(f"line {lineno}: `{crate}`/`{dep}` repeats a target")
        if "\n" in why:
            raise ConfigError(f"line {lineno}: the reason has to fit on one line")
        rows.append((crate, dep, feature, target_list, why))
    if not rows:
        raise ConfigError("no rows: the table of load-bearing features is empty")
    return rows


def parse_tree(text: str, dependency: str) -> tuple[set[str], bool]:
    """Feature names of `dependency` in one `cargo tree -e features` dump.

    Returns the set together with whether the dependency appeared at all, so a
    dependency that vanished is reported rather than reading as "no features".
    """
    features: set[str] = set()
    present = False
    for line in text.splitlines():
        label = line.rstrip("\n").strip().lstrip("│├└─ ").strip()
        # `cargo tree` prints a subtree it has already shown as `name rest (*)`.
        # A feature can appear only in that form, so the marker has to come off
        # before the label is matched or the feature reads as absent.
        if label.endswith(" (*)"):
            label = label[: -len(" (*)")]
        if not label:
            continue
        match = PACKAGE_NODE.match(label)
        if match and match.group("pkg") == dependency:
            present = True
            continue
        match = FEATURE_NODE.match(label)
        if match and match.group("pkg") == dependency:
            features.add(match.group("feature"))
    return features, present


def enabled_features(
    crate: str, dependency: str, target: str, repo: Path
) -> tuple[set[str], bool]:
    """Feature names of `dependency` visible to `crate` on `target`."""
    proc = subprocess.run(
        [
            "cargo", "tree", "-p", crate, "-e", "features", "--locked",
            "--target", target, "-i", dependency,
        ],
        cwd=repo,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        detail = (proc.stderr or proc.stdout).strip().splitlines()
        raise ConfigError(
            f"cargo tree failed for `{crate}` / `{dependency}` / {target} "
            f"(exit {proc.returncode}): {detail[-1] if detail else 'no output'}"
        )
    return parse_tree(proc.stdout, dependency)


def installed_targets() -> set[str]:
    proc = subprocess.run(
        ["rustup", "target", "list", "--installed"], capture_output=True, text=True
    )
    if proc.returncode != 0:
        raise ConfigError(
            "rustup target list --installed failed: "
            + (proc.stderr.strip() or "no output")
        )
    return {line.strip() for line in proc.stdout.splitlines() if line.strip()}


def check(
    repo: Path,
    rows: list[Row],
    *,
    features_for=enabled_features,
    available_targets=installed_targets,
) -> tuple[list[str], int]:
    problems: list[str] = []
    have = available_targets()
    for crate, dep, feature, targets, why in rows:
        missing = sorted(set(targets) - have)
        if missing:
            problems.append(
                f"`{crate}`/`{dep}`/`{feature}`: target(s) {', '.join(missing)} are not "
                f"installed; run `rustup target add {' '.join(missing)}` -- this guard "
                "fails rather than skip, because skipping is how the difference was "
                "originally missed"
            )
            continue
        enabled_on: list[str] = []
        absent: list[str] = []
        for target in targets:
            features, present = features_for(crate, dep, target, repo)
            if not present:
                absent.append(target)
                problems.append(
                    f"`{crate}` does not depend on `{dep}` when built for {target}, so "
                    f"the `/{feature}` question has no answer (row reason: {why})"
                )
                continue
            if feature in features:
                enabled_on.append(target)
        if not enabled_on:
            problems.append(
                f"`{dep}`/`{feature}` is enabled for `{crate}` on none of "
                f"{', '.join(targets)}; the row is stale or the feature was lost "
                f"(row reason: {why})"
            )
        else:
            # A target where the dependency is not built at all has already been
            # reported; calling it "built without the feature" would be false.
            built_on = [t for t in targets if t not in absent]
            if len(enabled_on) != len(built_on):
                missing_feature = sorted(set(built_on) - set(enabled_on))
                problems.append(
                    f"`{crate}` builds `{dep}` without `{feature}` on "
                    f"{', '.join(missing_feature)} (enabled on: "
                    f"{', '.join(enabled_on)}). The code cannot see the "
                    f"difference; row reason: {why}"
                )
    return problems, len(rows)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--repo",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root (default: the parent of scripts/ci)",
    )
    args = parser.parse_args(argv)

    repo: Path = args.repo.resolve()
    config = repo / CONFIG
    if not config.is_file():
        print(f"check-load-bearing-features: {CONFIG} is missing", file=sys.stderr)
        return 2
    try:
        rows = parse_config(config.read_text())
    except (ConfigError, OSError) as exc:
        print(f"check-load-bearing-features: bad {CONFIG}: {exc}", file=sys.stderr)
        return 2

    try:
        problems, row_count = check(repo, rows)
    except ConfigError as exc:
        print(f"check-load-bearing-features: {exc}", file=sys.stderr)
        return 2

    for problem in problems:
        print(f"check-load-bearing-features: {problem}", file=sys.stderr)
    if problems:
        print(
            f"check-load-bearing-features: {len(problems)} problem(s) across "
            f"{row_count} row(s)",
            file=sys.stderr,
        )
        return 1
    print(
        f"check-load-bearing-features: {row_count} load-bearing feature row(s) "
        "hold on every target they name"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
