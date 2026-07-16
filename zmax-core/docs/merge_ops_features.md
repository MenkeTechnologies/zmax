# `zmax-core::merge_ops` — round-4 diff / merge / patch batch

Pure-Rust, editor-type-free algorithms in `zmax-core/src/merge_ops.rs`, each
unit-tested in isolation (15 tests). Built on top of the round-1
`zmax-core::region_ops`, round-2 `zmax-core::text_engine`, and round-3
`zmax-core::power_edit` batches, and deliberately disjoint from them.

The existing `zmax-core::diff` module only exposes `compare_ropes`, which
produces a ropey `Transaction` for the undo engine — there is no *renderable*
line/word diff, no three-way merge, no unified-patch application, and no conflict
reconciliation. VCS-aware editors (GNU Emacs `smerge`/`ediff`, `git merge`, VS
Code, Neovim, Sublime, JetBrains, Zed, Helix) all ship these. This module adds
them as plain functions over `&str`.

Every algorithm shares one separately-tested primitive: a dynamic-programming
longest-common-subsequence over an arbitrary token type (`lcs_pairs`), so the
line diff, word diff, three-way merge and the token reconciler all rest on the
same core. Lines split on `\n` and re-join with `\n`, so a merge / patch
round-trips a buffer exactly.

Honesty: every row below is a real in-engine algorithm + test. No
Git-plumbing-over-subprocess, network, GPU, or renderer work is claimed here —
those boundaries stay with the VCS / view layers.

| Function / type | Capability | Prior art |
|---|---|---|
| `lcs_pairs` | Generic DP longest-common-subsequence, returns matched index pairs | Hunt–Szymanski / Myers diff core |
| `line_diff` | Renderable line-granular diff (`Equal`/`Delete`/`Insert`) | git unified diff body, VS Code inline diff |
| `word_diff` | Token-granular diff with same-kind run coalescing | git `--word-diff`, Emacs `ediff` refine |
| `three_way_merge` | diff3 line merge: auto-take one-sided & identical changes, git-style conflict markers otherwise | `git merge`, Emacs `smerge-mode`, `diff3` |
| `parse_unified_diff` / `apply_unified_diff` | Parse `@@` hunks and apply with context verification (mismatch / OOB / overlap errors) | `patch(1)`, git apply |
| ⭐ `reconcile_conflict` | Token-level auto-resolution of a conflict when both sides edit disjoint token spans of the same line | **zmax original** — every listed tool conflicts at line/hunk granularity; none weaves two same-line edits at word granularity |

## `three_way_merge` conflict format

Standard git diff3 markers:

```
<<<<<<< OURS
...ours...
||||||| BASE
...base...
=======
...theirs...
>>>>>>> THEIRS
```

`MergeResult.conflicts` counts the conflict regions (`0` == clean merge).

## Test coverage

`cargo test -p zmax-core --lib merge_ops` — 15 tests, all green. Full crate:
249 tests (234 baseline + 15 new), clippy clean.
