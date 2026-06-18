# Iteration 003 — solution-space analysis

Date: 2026-06-18

## Invariant that drives the design

GLV minting and burning use the live total value of all GM balances:

- deposit: mint GLV tokens from `received_value / total_glv_value`;
- withdrawal: derive the claim value from `glv_token_amount / total_glv_value`;
- deposit uses a maximized total value and withdrawal uses a minimized total value.

Today the entire portfolio is valued atomically in one Solana transaction. Any replacement must state whether it preserves that property or intentionally changes the economic/security model.

## Option A — transaction-shape guardrails and split close

Change the SDK to compile the final v0 message, resolve ALT-loaded addresses, count unique loaded accounts, and:

1. send execute plus close when the count fits;
2. otherwise send execute without close, then close in a second transaction;
3. reject locally with a typed error if execution alone exceeds the configured cluster limit.

Advantages:

- small, understandable change;
- no on-chain pricing change;
- existing builders already support `.close(false)`;
- the reproduction demonstrates that this recovers some failing transaction shapes.

Limitations:

- only removes accounts unique to close;
- does not remove the `2N` portfolio term;
- creates a completed-but-not-closed intermediate state;
- needs idempotent retry/recovery logic and monitoring for stranded escrows/action accounts.

Conclusion: required immediate hardening, but not a complete solution for a genuinely large single GLV.

## Option B — value only active balances

Change the portfolio account list from every configured market to:

- every market whose recorded GLV balance is nonzero; plus
- the current target market if it is not already active.

The GLV account already stores each market's balance. The program can iterate its fixed map and prove that the supplied ordered set contains every nonzero-balance market, preventing a caller from omitting liabilities/value.

Advantages:

- preserves atomic valuation;
- no cached NAV or new trusted party;
- allows many configured markets when only a smaller subset is funded;
- reduces both market/mint accounts and index-token feed accounts for zero-balance markets.

Limitations:

- does not help when more than roughly the account budget's worth of markets have nonzero balances;
- changes current behavior for zero-balance markets: they are no longer loaded and their pool values are not calculated. Their contribution is mathematically zero, but tests must confirm that no intended health check depended on touching them.
- active-set transitions around a zero-to-nonzero deposit and nonzero-to-zero withdrawal are easy to get wrong.

Conclusion: strong protocol-level optimization with relatively contained semantics. It should be implemented even if a larger redesign is planned.

## Option C — partition liquidity across multiple GLVs

Keep each GLV below a conservative worst-case active-market budget and create multiple vaults. Aggregate discovery/routing in the SDK/UI rather than pretending they are one on-chain fungible share token.

Advantages:

- preserves the currently audited atomic pricing path;
- no new oracle trust or stale NAV;
- operationally straightforward;
- scales total supported markets horizontally.

Trade-offs:

- liquidity and supply fragment across GLV tokens;
- users/routers choose a vault;
- allocations and shifts across vaults are not one atomic portfolio operation;
- this does not satisfy a requirement for one fungible token spanning every market.

Conclusion: safest production scaling strategy if one-token semantics are not mandatory.

## Option D — preload/batch oracle prices

The Oracle account can already persist validated prices through `set_prices_from_price_feed`, but GLV execution currently calls `with_prices`, which expects a cleared buffer, loads every feed account, sets prices, executes, and clears in one instruction.

A new begin/append/consume lifecycle could validate feed accounts in one or more preparatory transactions and let GLV execution consume the populated Oracle account without feed accounts.

Advantages:

- removes the per-token feed-account term from the execution transaction;
- uses existing price-map storage and validation concepts;
- can be combined with active-only valuation.

Risks/trade-offs:

- lifecycle must bind the buffer to authority, action, GLV, token-set hash, price round, and expiry;
- retries, stale buffers, partial batches, and clear-on-failure behavior become state-machine concerns;
- still leaves one market plus one mint per active GLV market;
- does not by itself scale a 96-active-market GLV.

Conclusion: useful headroom optimization, not a standalone complete fix.

## Option E — cached/batched NAV

Compute per-market or per-shard NAV contributions across multiple transactions and execute deposits/withdrawals against an aggregate snapshot.

Advantages:

- can reduce final execution to a constant or small number of accounts;
- can support a single token over many active markets.

Primary problem:

The snapshot is not automatically coherent. Markets can change through trades, deposits, withdrawals, fees, ADL, and oracle movement while batches are being computed. Checking every market version at finalization recreates the original account problem. A global invalidation counter would require making a shared account writable on every market mutation, introducing severe contention and touching many audited paths.

Other possible forms all change the trust or liveness model:

- freeze all relevant markets during valuation;
- trust a signed off-chain NAV report;
- accept bounded stale/mixed-slot values;
- maintain a globally contended aggregate/cache on every mutation.

Conclusion: do not ship this as a tactical fix. It needs a separate protocol specification, explicit economic assumptions, adversarial analysis, and audit.

## Option F — rely on the 128-account feature

The feature is currently inactive on public clusters and is not controlled by this program. Even if activated, it only moves the threshold and does not make the 96-market on-chain maximum executable in all transaction shapes.

Conclusion: possible future headroom, never the protocol's correctness strategy.

## Recommended sequence

1. Add exact compiled-message account-budget instrumentation and adaptive close splitting.
2. Change valuation to require only nonzero-balance markets plus the current target.
3. Define a conservative supported active-market/route envelope and enforce it operationally.
4. Use multiple GLVs for horizontal scale.
5. Only pursue a single-token cached-NAV architecture as a separately specified protocol redesign.
