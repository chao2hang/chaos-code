# TODO open-item classification (2026-11-14)

This is a classification, not a claim that the remaining roadmap items are done.
The source of truth remains `TODO.md`.

The Web/GUI integration line (M-1～M5) was removed from the roadmap on 2026-11-14,
when the ZCode frontend, the `chaos-web` host and the `/ws/zcode` RPC bridge were
deleted from the repository. Its groups remain in the table below at `0 / 0` so the
classifier's group set does not change; they own no rows any more. What remains is
the maintenance line (`MT-1`～`MT-7` and the §8 checklist).

## Current status counts

| group | unchecked | partial |
|---|---|---|
| M-1 | 0 | 0 |
| M0 | 0 | 0 |
| M1 | 0 | 0 |
| M2 | 0 | 0 |
| M3 | 0 | 0 |
| M4 | 0 | 0 |
| M5 | 0 | 0 |
| Maintenance items / §8 | 5 | 19 |

The row-level inventory is `docs/verification/todo-open-items.tsv`; the CI gate
re-renders it with `python3 scripts/ci/classify-open-todos.py --check-export`.
