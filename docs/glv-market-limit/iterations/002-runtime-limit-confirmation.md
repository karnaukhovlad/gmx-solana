# Iteration 002 — runtime limit confirmation

Date: 2026-06-18

## Confirmed runtime mechanism

Solana/Agave rejects a transaction before program execution when the resolved message contains more account keys than the bank's transaction account-lock limit.

For Solana SDK 2.1.21:

- `SanitizedTransaction::validate_account_locks` checks `message.account_keys().len() > tx_account_lock_limit`.
- The returned error is `TransactionError::TooManyAccountLocks`.
- The SDK maximum constant is 128, but the bank only uses 128 when feature `increase_tx_account_lock_limit` is active.
- Feature address: `9LZdXeKGeBV6hRLdxS1rHbHoEUsKqesCC2ZAPTPKJAbK`.
- Direct feature-status queries on 2026-06-18 reported this feature as inactive on mainnet-beta, devnet, and testnet.
- Current official Solana transaction documentation states a 64-account enforced limit, with 128 conditional on that currently inactive feature.

The reproduction's `Anchor.toml` explicitly deactivates this feature because a fresh local validator otherwise activates all features and would test against 128 rather than the production 64-account behavior.

## Why Address Lookup Tables do not solve it

An ALT changes message serialization: a 32-byte address can be referenced by a compact lookup index. After lookup resolution, those addresses are still loaded accounts and still count in `message.account_keys().len()` for account-lock validation.

Therefore an ALT can solve the 1,232-byte packet limit while the same transaction still fails with `TooManyAccountLocks`.

## GLV-specific scaling term

`Glv::validate_and_split_remaining_accounts` requires two aligned arrays for every managed market:

1. `N` market state accounts;
2. `N` market-token mint accounts.

The SDK's `split_to_accounts` constructs exactly those `2N` account metas.

The transaction also contains:

- 24 declared accounts for execute-deposit, before deduplication;
- 25 declared accounts for execute-withdrawal, before deduplication;
- one feed account per unique priced token;
- swap-path market accounts, often duplicates of the portfolio market array;
- virtual-inventory accounts, which are generally additional unique accounts;
- accounts introduced by the merged close instruction;
- compute-budget and program accounts.

The resolved unique count, not the raw number of account-meta references, is what matters. Current-market state and mint accounts appear in both the fixed accounts and the `2N` portfolio arrays, so they deduplicate. Other overlaps vary by action.

## Refined formula

For an execution transaction:

`U = F + 2 * (N - 1) + P + V + C - D`

Where:

- `U` is resolved unique accounts;
- `F` is the fixed unique account set including the current market and mint;
- `N` is GLV market count;
- `P` is unique oracle/feed-provider accounts not already in `F`;
- `V` is unique virtual-inventory and other route-specific state;
- `C` is unique accounts added by the close instruction when merged;
- `D` is any additional cross-category duplication.

Failure occurs when `U > 64`.

This establishes why `12` is approximate. There is no `12` check in the program. Each extra market consumes two accounts, while the remaining budget depends on the exact transaction shape.

## Sources

- Local SDK source: `solana-sdk-2.1.21/src/transaction/sanitized.rs`
- Local feature registry: `solana-feature-set-2.1.21/src/lib.rs`
- Official Solana transaction limits: <https://solana.com/docs/core/transactions>
- Official ALT definition: <https://solana.com/docs/references/terminology#address-lookup-table-alt>

## Remaining work

- Capture exact counts from the end-to-end reproduction.
- Record which mitigations only move the threshold and which remove the `O(N)` account dependency.
- Define the consistency invariant for any batched or cached valuation design.
