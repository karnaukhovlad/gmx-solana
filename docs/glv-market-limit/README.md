# GLV market-limit investigation

This directory is the durable investigation record for the GLV failure observed at roughly 12 markets.

## Files

- `iterations/001-initial-code-trace.md` — first-pass code trace, hypotheses, and evidence still required.
- `iterations/002-runtime-limit-confirmation.md` — Solana/Agave account-lock mechanism and cluster feature status.
- `iterations/003-solution-space.md` — candidate designs and trade-offs.
- `iterations/004-reproduction-results.md` — exact end-to-end account counts and failure results.
- `iterations/005-final-review.md` — final decision record and audit focus.
- `iterations/006-why-the-limit-is-64.md` — expanded history and mechanics behind 32-byte keys, the 64-account runtime cap, and the inactive 128-account feature.
- `iterations/007-account-count-formulas.md` — raw-reference and resolved-unique account formulas for every GLV instruction and action lifecycle.
- `iterations/008-market-count-scenario-tables.md` — deposit and withdrawal account-count tables for 1–15 GLV markets across baseline, heavy, execution-only, and merged-close situations.
- `iterations/009-one-account-per-market-feasibility.md` — why the remaining Market and mint accounts are independent, and the protocol migration required for a true `1N` layout.
- `iterations/010-oracle-feed-preloading.md` — deployed oracle freshness limits, the reference transaction's feed-account cost, and the state machine required to preload and consume prices safely.
- `overview.md` — consolidated root-cause statement; populated as evidence is confirmed.
- `proposals.md` — solution options, trade-offs, and recommendation; populated after root cause confirmation.
- `audit-risk-register.md` — production and audit-sensitive risks; populated alongside the proposals.

## Working rule

Each material investigation pass is saved as a new numbered file under `iterations/`. Earlier iterations are not rewritten to hide superseded hypotheses; later iterations explicitly correct them.
