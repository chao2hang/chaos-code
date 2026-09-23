# 2026 Q4 ignored test audit — deferred

The initial audit attempt on 2026-09-23 exposed defects in `scripts/ci/ignored-tests.sh`:

- CSV output does not quote fields correctly, so reasons containing commas are split into columns.
- The parser treats trailing source comments after `#[ignore]` as a non-empty reason. For example, `#[ignore] // requires pre-built binary` is still a bare Rust attribute, but was not classified as `NO_REASON`.
- Therefore the preliminary totals (452 attributes, 228 bare, 49 dated, 403 undated) and `docs/ignored-audit-2026q4.csv` are invalid and must not be used as a governance baseline.

A direct source-line scan found at least 218 `#[ignore]` attributes with no Rust reason string, but this is not a complete count because valid reasons may span lines and attributes can carry different syntax. The Q4 audit remains open until the inventory tool parses Rust attributes accurately, emits valid CSV, and passes fixtures for comments, multiline reasons, escaped quotes, and empty inventories.

No ignore reasons were mass-edited and no CI gate was enabled against this unverified inventory.
