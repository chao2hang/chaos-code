# 2026 Q4 ignored test audit

> Status: inventory evidence only. The source-by-source owner/reviewer decision is due as of 2026-10-01 and remains open; no row is treated as renewed or approved by this scan.

The inventory was regenerated on 2026-10-01 with the repaired Python scanner at `scripts/ci/ignored-tests.py`; the full machine-readable result is `docs/ignored-audit-2026q4.csv`.

The scanner emits one CSV row per Rust `#[ignore]` attribute and the CSV was read back with Python's standard CSV parser. Five fixtures cover trailing comments, multiline reasons with escaped quotes, CSV commas/quotes/newlines, an empty source tree, and textual `#[ignore]` examples inside documentation comments.

Current inventory: **429 attributes**, of which **218 have no Rust reason string** and **37 reasons include a `YYYY-MM` review date**. The previous snapshot's 434 attributes included five updater tests that asserted upstream xAI installer URLs and were ignored even though the shipped Chaos installer is channel-independent; those expectations are now replaced by three running tests against the real `reinstall_hint` entry point. These are scanner counts, not an approval or classification of ignored tests. The 2026 Q3 file records a prior point-in-time scan using a different method, so its counts are not directly comparable. The source-by-source review is still pending. A separate checked-in baseline now lists the 218 existing bare attributes by package/path/function and CI rejects additions not reviewed into that baseline; this does not bless their missing reasons or dates.
