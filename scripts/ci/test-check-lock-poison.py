#!/usr/bin/env python3
"""Fixtures for `check-lock-poison.py`.

`.lock().ok()` is the mirror image of `.lock().unwrap()`: one raises a second panic in a thread
that only wanted to read a value, the other pretends the report never arrived and answers from a
`None` it never learned anything about. Both are ratcheted; this file is the set of ways the
second shape hides, plus the ways the check itself could go quietly useless:

  * the three acquisitions are each found (`lock`, `read`, `write`), and each message names the
    crate, the path, the line and the receiver the call hangs off;
  * an acquisition rustfmt broke across lines is still found at the line the `.lock()` sits on,
    and its receiver is still named -- the first version of this guard cut its search window at
    the newline and printed `?` for exactly those wrapped chains, which turned out to be 8 of the
    19 sites the batch had to convert;
  * the shapes that are *not* this decision stay green, so a run that reads 0 sites means the rule
    was applied and not that nothing was read: `try_lock().ok()`, `if let Ok(guard) = ..`,
    `let Ok(..) = .. else { return }`, an async `.read().await.ok()` (async locks have no
    poisoning to swallow) and a `read(buf)` that takes arguments and is not a lock at all;
  * test code is scanned, unlike the test-side pass of `check-unbounded-recv.py`: a test that
    swallows poisoning cannot observe it either;
  * a site under `#[cfg(any())]` does not count, because no build contains it, while a live
    sibling in the same file still has to be reported;
  * the idiom inside a raw string, a char literal and a comment does not count, and the same file
    is then made to count by turning one mention into live code, so a green run here proves the
    blanking worked rather than that nothing was read;
  * `third_party/` and `target/` are not scanned;
  * a root with no Rust sources exits 2 rather than reporting a clean tree;
  * the shipped tree is clean at 0 sites, with no baseline file to lean on.

    python3 scripts/ci/test-check-lock-poison.py
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-lock-poison.py")
REPO = SCRIPT.parents[2]
SRC = "crates/codegen/widget/src/lib.rs"

# Everything the rule must leave alone. Each one is a decision a reader can see on the line, which
# is the whole difference between these and `.ok()`.
CLEAN = '''\
use std::sync::{Mutex, RwLock};

static CACHE: RwLock<Option<u32>> = RwLock::new(None);
static PIN: Mutex<Option<String>> = Mutex::new(None);

/// Skips the update when a holder died. A visible decision, so it stays.
pub fn skips_when_poisoned() -> Option<u32> {
    let Ok(guard) = CACHE.read() else {
        return None;
    };
    *guard
}

pub fn logs_on_a_dead_holder() {
    if let Ok(mut guard) = CACHE.write() {
        *guard = Some(1);
    }
}

pub fn only_when_nobody_holds_it() -> bool {
    PIN.try_lock().ok().is_some()
}

/// An async lock has no poisoning, so its `Err` is a closed or cancelled wait.
pub async fn async_reader(lock: &tokio::sync::RwLock<u32>) -> Option<u32> {
    lock.read().await.ok().map(|guard| *guard)
}

/// `read` here is `std::io::Read::read`, which takes a buffer; the gate needs empty parens.
pub fn not_a_lock(reader: &mut std::io::Stdin) -> usize {
    reader.read(&mut [0u8; 1]).ok().unwrap_or(0)
}
'''

# The shapes the rule exists to catch. The last is written the way rustfmt emits it, because that
# is the shape the guard has to survive to be worth running at all.
DIRTY = '''\
use std::sync::{Arc, Mutex, RwLock};

static CACHE: RwLock<Option<u32>> = RwLock::new(None);
static PIN: Mutex<Option<String>> = Mutex::new(None);

pub fn reads_a_dead_lock() -> Option<String> {
    PIN.lock().ok().and_then(|g| g.clone())
}

pub fn reads_a_dead_tier() -> Option<u32> {
    CACHE.read().ok().and_then(|g| *g)
}

pub fn writes_a_dead_tier() {
    if CACHE.write().ok().is_some() {}
}

pub struct Session {
    current_prompt_id: Arc<Mutex<Option<String>>>,
}

impl Session {
    pub fn falls_back_to_the_live_pin(&self) -> Option<String> {
        let _fallback = self
            .current_prompt_id
            .lock()
            .ok()
            .and_then(|g| g.clone());
        None
    }
}
'''

# One site inside a module no build contains, one outside it.
DEAD_CFG = '''\
use std::sync::Mutex;

static PIN: Mutex<Option<String>> = Mutex::new(None);

#[cfg(any())]
mod never_compiled {
    use super::PIN;

    pub fn swallowed() -> Option<String> {
        PIN.lock().ok().and_then(|g| g.clone())
    }
}

pub fn live() -> Option<String> {
    PIN.lock().ok().and_then(|g| g.clone())
}
'''

# The idiom mentioned without existing. Flipping the comment into code has to make it count, or a
# green run here would only prove the file was never read.
NOISED = '''\
use std::sync::Mutex;

static PIN: Mutex<Option<String>> = Mutex::new(None);

pub fn mentions_only() -> Option<String> {
    let shape = r#"PIN.lock().ok()"#;
    let quote = '.'; // a dot, next to a note about PIN.lock().ok()
    let _ = (shape, quote);
    // PIN.lock().ok() is what this function used to do
    None
}
'''

NOISED_LIVE = NOISED.replace(
    "    // PIN.lock().ok() is what this function used to do\n    None",
    "    PIN.lock().ok().and_then(|g| g.clone())",
)

# Test code is in scope for this rule, so the same shape inside `#[cfg(test)]` has to be reported.
IN_TEST_MOD = '''\
use std::sync::Mutex;

static PIN: Mutex<Option<String>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::PIN;

    #[test]
    fn asserts_away_the_poison() {
        assert!(PIN.lock().ok().is_some());
    }
}
'''

# What `--inventory` is supposed to see: the branched-on acquisitions the rule allows. Two drop the
# guard (the `if let` / `let else` forms bind nothing on the failure path), one `Err` arm takes it
# back with `into_inner()`, and three look-alikes must stay out of the count entirely.
INVENTORY = '''\
use std::sync::{Mutex, RwLock};

static CACHE: RwLock<Option<u32>> = RwLock::new(None);
static PIN: Mutex<Option<String>> = Mutex::new(None);

pub fn drops_a_write(guard_value: u32) {
    if let Ok(mut guard) = CACHE.write() {
        *guard = Some(guard_value);
    }
}

pub fn drops_a_read() -> Option<u32> {
    let Ok(guard) = CACHE.read() else {
        return None;
    };
    *guard
}

pub fn takes_the_guard_back() -> Option<u32> {
    match CACHE.read() {
        Ok(guard) => *guard,
        Err(poisoned) => poisoned.into_inner().take(),
    }
}

pub fn invents_absence() -> Option<u32> {
    match CACHE.read() {
        Ok(guard) => *guard,
        Err(_) => None,
    }
}

pub fn busy_is_not_poisoned() -> bool {
    if let Ok(guard) = PIN.try_lock() {
        return guard.is_some();
    }
    false
}

pub fn not_a_lock_at_all(reader: &mut std::io::Stdin) -> usize {
    let Ok(_n) = reader.read(&mut [0u8; 1]) else {
        return 0;
    };
    0
}

/// `if let Ok(guard) = CACHE.read()` in prose is not a site.
pub fn mentions_only() {}

#[cfg(test)]
mod tests {
    use super::PIN;

    #[test]
    fn drops_in_a_test_too() {
        if let Ok(guard) = PIN.lock() {
            assert!(guard.is_some());
        }
    }
}

#[cfg(any())]
mod never_compiled {
    use super::CACHE;

    pub fn also_drops() -> Option<u32> {
        if let Ok(guard) = CACHE.read() {
            return *guard;
        }
        None
    }
}
'''


def line_of(body: str, needle: str, nth: int = 1) -> int:

    """1-based line holding the nth occurrence of `needle`."""
    at = -1
    for _ in range(nth):
        at = body.index(needle, at + 1)
    return body.count("\n", 0, at) + 1


def make_tree(tmp: Path, files: dict[str, str], crates: tuple[str, ...] = ("widget",)) -> Path:
    """A workspace holding `crates`, with `files` written at their relative paths."""
    for crate in crates:
        manifest = tmp / "crates" / "codegen" / crate / "Cargo.toml"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(f'[package]\nname = "{crate}"\nversion = "0.1.0"\n')
    for rel, body in files.items():
        path = tmp / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    return tmp


def run(repo: Path, *extra: str) -> tuple[int, str]:
    argv = [sys.executable, str(SCRIPT), "--root", str(repo)]
    argv += list(extra)
    proc = subprocess.run(argv, capture_output=True, text=True)
    return proc.returncode, proc.stdout + proc.stderr


class LockPoison(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="lock-poison-")
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def test_the_shapes_that_are_decisions_stay_green(self) -> None:
        repo = make_tree(self.tmp, {SRC: CLEAN})
        code, out = run(repo, "--verbose")
        self.assertEqual(code, 0, out)
        self.assertIn("0 site(s)", out)
        self.assertIn("scanned 1 .rs file(s)", out)

    def test_each_acquisition_is_named_with_its_receiver(self) -> None:
        repo = make_tree(self.tmp, {SRC: DIRTY})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        for needle in ("PIN.lock().ok()", "CACHE.read().ok()", "CACHE.write().ok()"):
            where = f"widget: {SRC}:{line_of(DIRTY, needle)}: {needle}"
            self.assertIn(where, out)

    def test_a_wrapped_chain_reports_its_receiver_not_a_question_mark(self) -> None:
        """The guard's own bug: cutting the window at a newline named nothing for wrapped chains."""
        repo = make_tree(self.tmp, {SRC: DIRTY})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertNotIn("?.lock().ok()", out)
        # the second `.lock()` in the fixture is the wrapped one; the first is a single-liner
        self.assertIn(
            f"widget: {SRC}:{line_of(DIRTY, '.lock()', nth=2)}: self.current_prompt_id.lock().ok()",
            out,
        )

    def test_a_site_no_build_contains_does_not_count(self) -> None:
        repo = make_tree(self.tmp, {SRC: DEAD_CFG})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        # the 2nd occurrence is the live one; the 1st sits inside the `#[cfg(any())]` module
        self.assertIn(f"widget: {SRC}:{line_of(DEAD_CFG, 'PIN.lock()', nth=2)}", out)
        self.assertEqual(out.count("  widget: "), 1, out)

    def test_the_idiom_in_a_string_or_comment_does_not_count(self) -> None:
        repo = make_tree(self.tmp, {SRC: NOISED})
        code, out = run(repo)
        self.assertEqual(code, 0, out)
        # and the blanking is the reason, not a file that was never read
        make_tree(self.tmp, {SRC: NOISED_LIVE})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("PIN.lock().ok()", out)

    def test_test_code_is_scanned_too(self) -> None:
        repo = make_tree(self.tmp, {SRC: IN_TEST_MOD})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn(f"widget: {SRC}:{line_of(IN_TEST_MOD, 'PIN.lock()')}", out)

    def test_vendored_and_built_trees_are_not_scanned(self) -> None:
        # one clean source outside the pruned dirs, so exit 0 means "pruned" and not "read nothing"
        repo = make_tree(
            self.tmp,
            {
                SRC: CLEAN,
                "third_party/vendored/src/lib.rs": DIRTY,
                "target/debug/build/widget/src/lib.rs": DIRTY,
            },
        )
        code, out = run(repo, "--verbose")
        self.assertEqual(code, 0, out)
        self.assertIn("scanned 1 .rs file(s)", out)

    def test_a_root_without_rust_sources_is_not_a_clean_tree(self) -> None:
        repo = make_tree(self.tmp, {}, crates=())
        code, out = run(repo)
        self.assertEqual(code, 2, out)
        self.assertIn("refusing to report a clean tree", out)

    def test_the_inventory_buckets_the_allowed_branches(self) -> None:
        """`--inventory` names the decisions the rule allows, split by whether the guard is dropped.

        The count has to come from the scanner that defines the exemption, and the look-alikes that
        are not this decision (a `try_lock()`, a `read(buf)` that takes arguments, prose) must not
        inflate it -- an inventory nobody can misread is the only kind worth quoting in a TODO row.
        """
        repo = make_tree(self.tmp, {SRC: INVENTORY})
        code, out = run(repo, "--inventory")
        self.assertEqual(code, 0, out)
        self.assertIn("if-let-Ok      prod-drop    1  prod-hold    0  test-drop    1  test-hold    0", out)
        self.assertIn("let-Ok-else    prod-drop    1  prod-hold    0  test-drop    0  test-hold    0", out)
        self.assertIn("match-Err-arm  prod-drop    1  prod-hold    1  test-drop    0  test-hold    0", out)
        self.assertIn("5 site(s) branch on the poison error", out)
        self.assertIn("4 drop the guard, 1 take it back", out)
        # the two `match` arms differ only in their `Err` body, so the line numbers carry the bucket
        self.assertIn(f"{SRC}:{line_of(INVENTORY, 'match CACHE.read()', nth=2)}", out)

    def test_the_inventory_counts_prose_only_once_it_becomes_code(self) -> None:
        """Same non-vacuity pair as the rule itself: blanking, not an unread file, gives the low count."""
        repo = make_tree(self.tmp, {SRC: INVENTORY})
        code, out = run(repo, "--inventory")
        self.assertEqual(code, 0, out)
        self.assertEqual(out.count("  ["), 5, out)  # the doc comment's copy is not a row
        make_tree(
            self.tmp,
            {
                SRC: INVENTORY.replace(
                    "/// `if let Ok(guard) = CACHE.read()` in prose is not a site.",
                    "pub fn prose_became_code() {\n    if let Ok(guard) = CACHE.read() {\n"
                    "        let _ = *guard;\n    }\n}",
                )
            },
        )
        code, out = run(repo, "--inventory")
        self.assertEqual(code, 0, out)
        self.assertEqual(out.count("  ["), 6, out)

    def test_shipped_tree_is_clean(self) -> None:
        code, out = run(REPO)
        self.assertEqual(code, 0, out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
