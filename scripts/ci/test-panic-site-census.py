#!/usr/bin/env python3
"""Fixtures for `panic-site-census.py`: every rule that moves a number is broken
on purpose in a generated crate, and the census has to put the site on the right
side of the production line.

A count nobody can falsify is a number someone once typed. These run against a
scratch crate tree in a temporary directory, so they hold whatever the repository
happens to contain tomorrow.

    python3 scripts/ci/test-panic-site-census.py
"""

import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "census", HERE / "panic-site-census.py"
)
census_module = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(census_module)

FAILED: list[str] = []


def check(what: str, got: object, want: object) -> None:
    if got != want:
        FAILED.append(f"{what}: got {got!r}, want {want!r}")


def build(root: Path) -> None:
    """A crate whose files cover each classification rule, one `.unwrap()` each."""
    src = root / "crates" / "demo" / "src"
    src.mkdir(parents=True)
    (root / "crates" / "demo" / "Cargo.toml").write_text(
        '[package]\nname = "demo"\n', encoding="utf-8"
    )
    (src / "lib.rs").write_text(
        """
pub fn ships() -> u32 {
    maybe().unwrap(); // this one is in the binary
    let text = "inner.unwrap() is not a call";
    let raw = r#"also not a call: thing.unwrap()"#;
    /* neither is this: other.unwrap() */
    // or this: third.unwrap()
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_panicking_test_is_fine() {
        assert_eq!(value().unwrap(), 1);
    }
}

#[cfg_attr(not(test), deny(unused))]
pub fn still_ships() -> u32 {
    other().unwrap();
    2
}

#[cfg(test)]
mod helpers;

#[cfg(all(test, unix))]
mod on_unix {
    #[test]
    fn another() {
        unix_thing().unwrap();
    }
}

pub mod shared;

// The rest of the fixture's files, declared the way a real crate declares them:
// reachability from a crate root is what decides whether a file is compiled at all.
pub mod unsafe_parts;
pub mod escaped_quotes;
pub mod cfg_shapes;
pub mod path_declared;
pub mod nested;
pub mod mixed;
mod whole_file;
mod after_attr;

#[cfg(test)]
#[path = "shared.rs"]
mod shared_as_test;
""",
        encoding="utf-8",
    )
    (src / "helpers.rs").write_text(
        "#[test]\nfn reached_through_a_test_mod() {\n    helper().unwrap();\n}\n",
        encoding="utf-8",
    )
    (src / "whole_file.rs").write_text(
        "#![cfg(test)]\nfn only_in_tests() {\n    gone().unwrap();\n}\n",
        encoding="utf-8",
    )
    # An inner `cfg(test)` is still the whole file even when lint attributes were
    # written above it, which is the common shape in this workspace.
    (src / "after_attr.rs").write_text(
        '#![allow(dead_code)]\n#![cfg(test)]\nfn also_only_tests() {\n    gone().unwrap();\n}\n',
        encoding="utf-8",
    )
    # A literal whose last character is an escaped quote. A scanner that ends the
    # literal at the first `"` it sees leaves the real terminator to open a second
    # literal, which then runs to the next quote in the file and takes the
    # `cfg(test)` attribute below it with it. That attribute is how the module's
    # `.unwrap()` is recognised as test code, so losing it reports a test panic as
    # one that ships, and moves whatever brace the swallowed text was inside.
    (src / "escaped_quotes.rs").write_text(
        r'''pub fn quotes() -> u32 {
    let mut out = String::new();
    out.push_str("\\\"");
    quoted().unwrap();
    14
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_after_a_literal_ending_in_an_escaped_quote() {
        out.push('"');
        in_a_test().unwrap();
    }
}
''',
        encoding="utf-8",
    )
    # A `cfg` gate that only mentions `test` in one branch still builds in a
    # normal build, so its panics ship. Reading the word `test` anywhere in the
    # condition as "test code" is how a file like
    # `#[cfg(any(target_os = "linux", all(unix, test)))]` left the census while
    # sitting in the released Linux binary.
    (src / "cfg_shapes.rs").write_text(
        """// One branch of the `any` holds without `test`, so this is in the release build.
#[cfg(any(target_os = "linux", all(unix, test)))]
pub fn built_on_linux_without_tests() -> u32 {
    linux_thing().unwrap();
    21
}

// A feature alone is enough to build this one.
#[cfg(any(test, feature = "gated"))]
pub fn built_for_a_feature_too() -> u32 {
    feature_thing().unwrap();
    22
}

// `cfg_attr` only swaps attributes in and out; the item is compiled either way.
#[cfg_attr(test, allow(dead_code))]
pub fn attribute_swaps_but_item_ships() -> u32 {
    attr_swapped().unwrap();
    23
}

// Every branch needs `test`, so nothing here reaches a release build.
#[cfg(any(all(test, unix), all(test, windows)))]
mod only_under_test {
    pub fn either_os() -> u32 {
        either_os_thing().unwrap();
        24
    }
}
""",
        encoding="utf-8",
    )
    (src / "unsafe_parts.rs").write_text(
        """
pub fn wraps() {
    unsafe { read_raw() };
}

pub unsafe fn contract() {}

unsafe impl Send for Widget {}

pub unsafe extern "C" fn from_c() {}

#[cfg(test)]
mod t {
    #[test]
    fn in_a_test() {
        unsafe { poke() };
    }
}
""",
        encoding="utf-8",
    )
    tests = root / "crates" / "demo" / "tests"
    tests.mkdir()
    (tests / "integration.rs").write_text(
        "mod common;\n\n#[test]\nfn it_works() {\n    shipped().unwrap();\n}\n",
        encoding="utf-8",
    )
    # `tests/common/mod.rs` is the shared-helper shape Cargo tells people to use. It
    # is compiled, so it is not one of the files nobody declares, and it is compiled
    # only by a test target, so it is not production either.
    common = tests / "common"
    common.mkdir()
    (common / "mod.rs").write_text(
        "pub fn shared_setup() -> u32 {\n    setup().unwrap();\n    11\n}\n",
        encoding="utf-8",
    )
    # A binary target's submodule sits *beside* the root, not inside a directory
    # named after it: `src/bin/tool.rs` + `mod tool_helper;` is `src/bin/tool_helper.rs`.
    bin_dir = src / "bin"
    bin_dir.mkdir()
    (bin_dir / "tool.rs").write_text(
        "mod tool_helper;\n\nfn main() {\n    tool_helper::helps();\n}\n",
        encoding="utf-8",
    )
    (bin_dir / "tool_helper.rs").write_text(
        "pub fn helps() -> u32 {\n    bin_thing().unwrap();\n    12\n}\n",
        encoding="utf-8",
    )
    # The shape this workspace actually writes: the test module lives in another
    # file named by `#[path]`, so the mod name says nothing about it.
    (src / "path_declared.rs").write_text(
        "#[cfg(test)]\n#[path = \"hidden_tests.rs\"]\nmod tests;\n"
        "pub fn ships_too() -> u32 {\n    side().unwrap();\n    4\n}\n",
        encoding="utf-8",
    )
    (src / "hidden_tests.rs").write_text(
        "#[test]\nfn declared_by_path() {\n    hidden().unwrap();\n}\n",
        encoding="utf-8",
    )
    # And the same one level down: a module declared inside a test module.
    (src / "nested.rs").write_text(
        "pub fn ships_here() -> u32 {\n    nested_side().unwrap();\n    5\n}\n\n"
        "#[cfg(test)]\nmod tests {\n    #[path = \"nested_inner.rs\"]\n    mod inner;\n}\n",
        encoding="utf-8",
    )
    (src / "nested_inner.rs").write_text(
        "#[test]\nfn declared_from_inside_a_test_mod() {\n    deep().unwrap();\n}\n",
        encoding="utf-8",
    )
    # A file that declares both kinds of submodule. Marking every declaration in a
    # file that happens to hold a test module would swallow `mixed/helper.rs`,
    # which ships, so the rule is about the declaration, not the file.
    (src / "mixed.rs").write_text(
        "pub mod helper;\n\n#[cfg(test)]\nmod tests;\n\n"
        "pub fn ships_mixed() -> u32 {\n    mixed().unwrap();\n    6\n}\n",
        encoding="utf-8",
    )
    (src / "mixed").mkdir()
    (src / "mixed" / "helper.rs").write_text(
        "pub fn helps() -> u32 {\n    helper_thing().unwrap();\n    7\n}\n",
        encoding="utf-8",
    )
    (src / "mixed" / "tests.rs").write_text(
        "#[test]\nfn beside_the_host_file() {\n    in_tests().unwrap();\n}\n",
        encoding="utf-8",
    )
    # The compile-it-twice shape: one file named by an ordinary declaration and by
    # a test-gated one. It is in the binary, so a test declaration cannot move it
    # out of production. Both declarations are in `lib.rs` so that the plain
    # `mod shared;` convention and the `#[path]` one resolve to the same file.
    (src / "shared.rs").write_text(
        "pub fn serves_both() -> u32 {\n    shared_thing().unwrap();\n    9\n}\n",
        encoding="utf-8",
    )
    # Left behind by a refactor: no `mod` anywhere names it, so `cargo` never opens
    # it. Its panics cannot fire, and counting them would send a governance batch
    # to fix a file that is not in any binary.
    (src / "nobody_declares.rs").write_text(
        "pub fn never_compiled() -> u32 {\n    dead().unwrap();\n    10\n}\n",
        encoding="utf-8",
    )


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        build(root)
        rows = census_module.census(root)
        check("one crate found", sorted(rows), ["demo"])
        row = rows["demo"]
        # lib.rs: two production unwraps (the shipped one and the not(test) one);
        # four more sit in comments, a string and a raw string and count nowhere;
        # the rest are inside cfg(test) spans or test-only files.
        check("production unwraps", row["unwrap_prod"], 12)
        check("all unwraps counted at all", row["unwrap"], 24)
        check(
            "the file no crate root declares is compiled by nothing, so its "
            ".unwrap() is counted in neither column",
            (row["uncompiled_files"], row["unwrap"] - row["unwrap_prod"]),
            (1, 12),
        )
        check(
            "the whole-file cfg(test) files, the declared test mod and tests/ "
            "are not production",
            row["unwrap"] - row["unwrap_prod"],
            12,
        )
        check("unsafe sites total", row["unsafe"], 5)
        check("unsafe sites in production", row["unsafe_prod"], 4)
        check(
            "unsafe split",
            (
                row["unsafe_block"],
                row["unsafe_fn"],
                row["unsafe_impl"],
                row["unsafe_extern"],
            ),
            (2, 1, 1, 1),
        )
        check("files counted as test code", row["test_files"], 8)
        check("files seen", row["files"], 20)

        # The baseline is the ratchet, so it has to fail when a crate gains a way
        # to panic and pass when one is removed.
        baseline = root / "baseline.tsv"
        wrote = subprocess.run(
            [
                sys.executable,
                str(HERE / "panic-site-census.py"),
                "--root",
                str(root),
                "--write-baseline",
                str(baseline),
            ],
            capture_output=True,
            text=True,
        )
        check("writing a baseline succeeds", wrote.returncode, 0)
        check(
            "baseline records the production row",
            baseline.read_text(encoding="utf-8").splitlines()[-1],
            "demo\t12\t0\t0\t4",
        )
        checker = [
            sys.executable,
            str(HERE / "panic-site-census.py"),
            "--root",
            str(root),
            "--check-baseline",
            str(baseline),
        ]
        held = subprocess.run(checker, capture_output=True, text=True)
        check("an unchanged tree holds its baseline", held.returncode, 0)
        lib = root / "crates" / "demo" / "src" / "lib.rs"
        text = lib.read_text(encoding="utf-8")
        lib.write_text(
            text.replace(
                "#[cfg(test)]\nmod helpers;",
                "pub fn added_one() -> u32 {\n    new().unwrap();\n    3\n}\n\n#[cfg(test)]\nmod helpers;",
            ),
            encoding="utf-8",
        )
        grew = subprocess.run(checker, capture_output=True, text=True)
        check("a gained production unwrap fails the baseline", grew.returncode, 1)
        check("and says which crate", "demo: production sites grew" in grew.stdout, True)
        lib.write_text(
            text.replace(
                "    maybe().unwrap(); // this one is in the binary\n",
                "    let _ = maybe.ok_or(())?; // the panic is gone\n",
            ),
            encoding="utf-8",
        )
        improved = subprocess.run(checker, capture_output=True, text=True)
        check("removing one still holds", improved.returncode, 0)
        check("and is reported as progress", "1 fewer production site" in improved.stdout, True)

        # The uncompiled list is a ratchet too: a file that stops being compiled has
        # to be noticed, because the usual reason is a lost `mod` declaration.
        uncompiled_record = root / "uncompiled.txt"
        recorder = [
            sys.executable,
            str(HERE / "panic-site-census.py"),
            "--root",
            str(root),
        ]
        wrote_record = subprocess.run(
            recorder + ["--write-uncompiled", str(uncompiled_record)],
            capture_output=True,
            text=True,
        )
        check("writing the uncompiled list succeeds", wrote_record.returncode, 0)
        check(
            "it names the file no root declares",
            uncompiled_record.read_text(encoding="utf-8").splitlines()[-1],
            "crates/demo/src/nobody_declares.rs",
        )
        held_record = subprocess.run(
            recorder + ["--check-uncompiled", str(uncompiled_record)],
            capture_output=True,
            text=True,
        )
        check("the recorded uncompiled set holds", held_record.returncode, 0)
        src_dir = root / "crates" / "demo" / "src"
        (src_dir / "lost_its_declaration.rs").write_text(
            "pub fn orphaned() -> u32 {\n    orphan().unwrap();\n    13\n}\n",
            encoding="utf-8",
        )
        grew_record = subprocess.run(
            recorder + ["--check-uncompiled", str(uncompiled_record)],
            capture_output=True,
            text=True,
        )
        check("a second file compiled by nothing fails", grew_record.returncode, 1)
        check(
            "and names the file",
            "crates/demo/src/lost_its_declaration.rs" in grew_record.stdout,
            True,
        )
        (src_dir / "nobody_declares.rs").unlink()
        stale_record = subprocess.run(
            recorder + ["--check-uncompiled", str(uncompiled_record)],
            capture_output=True,
            text=True,
        )
        check(
            "a file that started compiling again is a stale row, not a pass",
            (stale_record.returncode, "but it is not any more" in stale_record.stdout),
            (1, True),
        )

        # A crate that has gone away is a stale row, not a silent pass.
        gone = root / "crates" / "demo"
        stale = root / "stale.tsv"
        stale.write_text("demo\t2\t0\t0\t4\nabsent\t1\t0\t0\t0\n", encoding="utf-8")
        missing = subprocess.run(
            [
                sys.executable,
                str(HERE / "panic-site-census.py"),
                "--root",
                str(root),
                "--check-baseline",
                str(stale),
            ],
            capture_output=True,
            text=True,
        )
        check("a baseline row for a crate that is gone fails", missing.returncode, 1)
        check("named as such", "no longer a crate" in missing.stdout, True)
        gone.mkdir(exist_ok=True)

    if FAILED:
        for line in FAILED:
            print("FAIL " + line)
        return 1
    print("all census fixture checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
