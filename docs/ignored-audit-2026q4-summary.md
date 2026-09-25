# 2026 Q4 ignored test audit

The inventory was regenerated on 2026-09-23 with the repaired Python scanner at `scripts/ci/ignored-tests.py`; the full machine-readable result is `docs/ignored-audit-2026q4.csv`.

The scanner emits one CSV row per Rust `#[ignore]` attribute and the CSV was read back with Python's standard CSV parser. Five fixtures cover trailing comments, multiline reasons with escaped quotes, CSV commas/quotes/newlines, an empty source tree, and textual `#[ignore]` examples inside documentation comments.

Current inventory: **434 attributes**, of which **218 have no Rust reason string** and **37 reasons include a `YYYY-MM` review date**. These are scanner counts, not an approval or classification of ignored tests. The 2026 Q3 file records a prior point-in-time scan using a different method, so its counts are not directly comparable. The source-by-source review is still pending. A separate checked-in baseline now lists the 218 existing bare attributes by package/path/function and CI rejects additions not reviewed into that baseline; this does not bless their missing reasons or dates.
