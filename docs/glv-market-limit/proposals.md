# GLV market-limit proposals

## Recommendation

Use a staged fix:

1. immediately make transaction construction account-budget aware and split close;
2. change portfolio valuation to load only markets with nonzero GLV balances, plus the current target;
3. enforce a conservative active-market/route envelope and scale across multiple GLVs;
4. do not introduce cached/batched NAV into the audited protocol without a separate design and audit.

This sequence preserves current pricing semantics and removes avoidable accounts before considering a new trust model.

For a true one-account-per-market design, see
[Iteration 009](iterations/009-one-account-per-market-feasibility.md). The remaining
mint account cannot be removed safely with a GLV-only cache because holders can burn
standard SPL market tokens directly. The smallest viable `1N` design makes
Store-tracked issued supply canonical across every GM pricing path, which is a
protocol-wide accounting migration.

## Phase 1 — SDK account-budgeting and adaptive close

Compile the actual versioned message with its ALTs, resolve and count unique loaded accounts, and check both:

- resolved account count `<= 64`;
- serialized transaction size `<= 1,232`.

Builder behavior:

1. try execute plus close;
2. if it exceeds the account limit, build execute only;
3. if execute fits, send it and then submit close separately;
4. if execute alone does not fit, return a typed local error with a category breakdown and required headroom.

Why this is first:

- the existing reproduction proves it changes heavy deposit from 65 to 60 accounts and heavy withdrawal from 68 to 61;
- it requires no on-chain pricing change;
- it turns an opaque validator error into deterministic SDK behavior.

Engineering trade-offs:

- one extra transaction and fee in fallback cases;
- completed actions and output tokens can remain in escrow until close succeeds;
- orchestration must distinguish “execution succeeded, close pending” from total failure;
- recovery and monitoring become mandatory.

The client should conservatively use 64 even though the SDK constant is 128. A future cluster feature activation can be treated as additional headroom, not correctness.

## Phase 2 — active-balance portfolio valuation

Today every configured market is loaded even when the GLV's recorded balance for it is zero. A zero balance contributes zero to GLV value.

Change the required portfolio set to:

- every configured market with `GlvMarketConfig.balance > 0`;
- the action's current target market, even when its pre-action balance is zero.

On-chain validation must derive the exact expected ordered token set from the GLV account. It must reject missing, extra, duplicate, or reordered entries. The SDK is only a constructor; it is not trusted to decide which value-bearing markets exist.

Expected benefit:

- a GLV can manage many eligible markets without paying account cost for every empty market;
- removes two accounts per empty market;
- removes feeds for index tokens used only by empty markets;
- preserves one-transaction atomic valuation of every nonzero contribution.

Trade-offs:

- the limit becomes an active funded-market limit, not a configured-market limit;
- it does not solve a single GLV with many nonzero balances;
- CPI consumers such as liquidity-provider `stake_glv` must adopt the new layout;
- zero/nonzero transitions and rounding dust require careful tests.

This is the best bounded on-chain change because it removes accounts without changing how any nonzero position is valued.

## Phase 3 — explicit supported envelope and horizontal scaling

Do not advertise `Glv::MAX_ALLOWED_NUMBER_OF_MARKETS == 96` as an executable active-market capacity.

Define and monitor:

- maximum active markets for each supported operation;
- maximum unique index feeds;
- maximum swap-path markets and virtual inventories;
- whether close is merged;
- minimum reserved account headroom for future account additions.

For more total market coverage, deploy multiple GLVs and route at the SDK/UI layer. This preserves the audited atomic pricing model.

Trade-offs:

- separate GLV token supplies and fragmented liquidity;
- no single fungible share token over all markets;
- cross-GLV rebalancing is not one atomic portfolio operation.

If one-token semantics are not a hard requirement, this is materially safer than cached NAV.

## Optional headroom — preloaded oracle prices

The Oracle account already stores validated prices. A versioned begin/append/consume lifecycle could move feed-account validation into preparatory transactions and let GLV execution consume a buffer bound to:

- action;
- GLV;
- authority;
- canonical token-set hash;
- oracle timestamp/slot range;
- expiry and one-time-use nonce.

This removes feed accounts from the execution transaction and can support batching when the feed set itself exceeds 64.

It still leaves the market-plus-mint term and adds a state machine. Treat it as an optimization after Phases 1 and 2, not as the primary fix.

## Rejected as a tactical fix — cached or batched NAV

A cached NAV can make final execution constant-size, but it changes the core invariant. Markets and oracle prices can change while batches are calculated. A final check of every market version recreates the original account problem.

Known alternatives all carry major costs:

- freeze markets during valuation;
- make a global invalidation account writable in every market mutation, creating contention;
- trust an off-chain signed NAV report;
- accept stale or mixed-slot pricing.

Any of these is a new protocol and trust model. It requires a formal economic specification, adversarial analysis, migration design, and independent audit.

## Not a solution — wait for 128 accounts

The feature is currently inactive and outside program control. At 128 accounts the same linear growth remains, and worst-case 96-market operations still do not fit.

## Implementation order

1. Preserve the reproduction as a boundary regression test.
2. Add a reusable compiled-message account report to transaction builders.
3. Implement adaptive close splitting and recovery status.
4. Add active-set helpers to `Glv`, then version deposit/withdrawal/pricing account parsing.
5. Update SDK hints, feed collection, IDL, and liquidity-provider CPI forwarding.
6. Add equivalence tests comparing old full-set valuation with new active-set valuation.
7. Set operational limits from worst-case measured shapes with reserved headroom.
