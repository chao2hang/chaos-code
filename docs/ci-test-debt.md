# CI test debt

`cargo test` in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) is
scoped, not `--workspace`. This file is the ledger for what is excluded and why.

## The rule

**The exclusion list is append-never, remove-only.**

Adding a crate to the `--exclude` list is not an accepted way to make CI green.
If a change breaks tests in a crate that is currently covered, fix the change or
fix the test — do not widen the list. Removing an entry is the only edit that
should reach `main` without discussion.

## Why these are excluded

CI ran no `cargo` step at all until 0.2.116. `release.yml` only runs
`cargo build`, which never compiles `#[cfg(test)]` code, so test targets drifted
out of sync with production structs for an extended period and nothing reported
it. 0.2.116 fixed 96 compile errors and brought roughly 7.4k tests back into a
runnable state; the crates below still carry runtime failures that predate that
work — they were never executed, so they were never green.

They are excluded so the job can gate on *new* regressions today, rather than
staying permanently red and being ignored.

## Current exclusions

| Crate | Why it is excluded | Notes |
| --- | --- | --- |
| `xai-grok-pager` | TUI core; pre-existing runtime failures | Largest single block. Highest user-facing surface — the interactive UI. |
| `xai-grok-pager-bin` | Binary wrapper around the above | Likely follows `xai-grok-pager`. |
| `xai-grok-pager-minimal` | Reduced pager build | Likely follows `xai-grok-pager`. |
| `xai-grok-pager-pty-harness` | PTY integration harness | Needs a real PTY; may also be environment-sensitive in CI. |
| `xai-grok-shell-base` | Shared shell primitives | Depended on widely; repairing this may unblock others. |
| `xai-grok-tools` | Tool implementations | User-facing behaviour (file edits, search, exec). |
| `xai-grok-update` | Self-update / installer logic | Smallest surface; best first candidate to reintegrate. |

The aggregate figure recorded when the job was introduced was roughly **209
failing tests** across these seven crates. Per-crate counts are not yet
established — see *Next steps*.

## Risk this leaves open

`cargo check --all-targets` and `cargo clippy --all-targets` still run over the
full workspace, so test code cannot silently stop compiling again. What is
**not** covered is runtime behaviour in exactly the three areas users touch
most: the TUI (`pager`), the tools, and the updater. A logic regression in any
of them ships without CI objecting.

Mitigation until the list is empty: a change that touches only an excluded
crate should be accompanied by a local `cargo test -p <crate>` run, and the
result stated in the PR description.

## Next steps

Ordered by cost-to-value, cheapest first:

1. **Establish per-crate counts.** Run `cargo test -p <crate> --no-fail-fast`
   for each of the seven and record the real numbers in the table above. The
   `209` figure is an aggregate and may have drifted.
2. **Reintegrate `xai-grok-update`.** Smallest surface, and the failure mode it
   guards (a broken self-update) is severe and hard to hotfix once shipped.
3. **Reintegrate `xai-grok-tools`.** High user-facing value; failures here are
   likely to be genuine assertions about fork-changed behaviour rather than
   deep breakage.
4. **`xai-grok-shell-base`.** Shared dependency; may resolve failures in the
   pager crates as a side effect.
5. **The four pager crates last.** Largest block, and `pty-harness` may need CI
   environment work (PTY availability) beyond test repair.

For each crate, the repair split is usually: tests asserting upstream xAI
defaults this fork removed by design → mark `#[ignore]` with a reason (as
0.2.116 did for 21 such tests); everything else → a real fix.

## Related

- Tests marked `#[ignore]` are a separate, smaller debt. Their reasons must
  stay readable and be revisited periodically; a permanent `#[ignore]` is a
  deleted test with extra steps.
