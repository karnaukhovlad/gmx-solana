# Iteration 004 — end-to-end reproduction results

Date: 2026-06-18

## Result

The targeted Anchor test passed and reproduced the failure as `TransactionError::TooManyAccountLocks`.

Failing transactions produced no GMX program logs. The validator rejected them before instruction execution. Every reproduced transaction remained below the independent 1,232-byte packet limit.

Command:

```sh
GMSOL_TEST=glv_unique_account_limit_is_transaction_shape_dependent \
GMSOL_RNG=64012 \
EXTRA_CARGO_ARGS=--nocapture \
RUST_LOG=info \
anchor test --skip-build -- --features mock --features devnet,test-only,migration
```

## Exact measurements

| 12-market transaction shape | Unique accounts | Result | Serialized bytes |
|---|---:|---|---:|
| Baseline deposit + close | 49 | Pass | 744 |
| Baseline withdrawal + close | 58 | Pass | 878 |
| Heavy deposit + close | 65 | `TooManyAccountLocks` | 784 |
| Same heavy deposit, execution only | 60 | Pass | 639 |
| Heavy withdrawal + close | 68 | `TooManyAccountLocks` | 1,029 |
| Same heavy withdrawal, execution only | 61 | Pass | 820 |

The limit is 64. The failing cases were 65 and 68. Splitting close reduced them to 60 and 61 without changing the GLV's 12-market membership.

This directly proves:

1. market count alone does not decide success;
2. resolved unique accounts decide success;
3. ALT compression kept packet size safe but did not avoid account-lock rejection;
4. close merging can be the difference between pass and fail;
5. the failure occurs before GMX code executes.

## Why the fixture matters

All 12 synthetic index tokens intentionally shared one index feed. The baseline therefore used only two unique oracle feed accounts. This isolates the portfolio and transaction-shape account terms.

In a real GLV, a new market can add:

- one market state account;
- one market-token mint account;
- often one new index-token feed account;
- sometimes route/virtual-inventory accounts when that market participates in a swap path.

The guaranteed slope is two unique accounts per additional configured market. The common practical slope is two to three, with route-dependent step increases.

## Why the limit is “about 12”

The 64-account budget is shared by:

- a fixed execute-deposit/withdrawal account set;
- all `2N` portfolio market and mint references, less duplicates for the current market;
- unique feed accounts;
- route markets and virtual inventories;
- any merged close instruction.

At 12 markets the measured transactions ranged from 49 to 68 unique accounts. Therefore “12 markets” is an observed operating envelope, not a protocol constant:

- a light 12-market transaction passed with 15 accounts of headroom;
- a heavy 12-market transaction failed by one account;
- removing close made the exact same action pass.

For a fixed transaction shape where each extra market introduces only its market and mint, reducing market count by one reduces the resolved count by two. If the new market also introduces a unique feed, it reduces the count by three.

## Test artifact

The reproduction lives in `tests/anchor_test/glv.rs` and is launched by `scripts/reproduce_glv_account_limit.sh`. A deterministic `GLV_ACCOUNT_REPORT` stderr line was added so exact metrics remain visible even if tracing filters change.
