# Iteration 010 — GLV oracle feed preloading

Date: 2026-06-19

## Question

Can GLV deposit and withdrawal execution stop passing every price-feed account in
the execution instruction, and what freshness and state-machine constraints would
be required?

## Transaction evidence

The reference transaction is:

- Solscan:
  [`3RYm3dq6fVCfYVMtgvRxFieJrkPFUYRD8LSwx8TB4Lhr6McnLre2Zsr2zjK1eCxL9rDHTVhvBuj9GqPipWaNECtr`](https://solscan.io/tx/3RYm3dq6fVCfYVMtgvRxFieJrkPFUYRD8LSwx8TB4Lhr6McnLre2Zsr2zjK1eCxL9rDHTVhvBuj9GqPipWaNECtr)
- slot: `427536650`;
- block time: `2026-06-19 15:51:43 UTC`;
- result: successful `ExecuteGlvDeposit`, followed by `CloseGlvDeposit`;
- compute units consumed: `432,644`;
- resolved transaction accounts: `50`.

The execute instruction has 24 fixed account references followed by:

```text
8 Market accounts
+ 8 market-token mint accounts
+ 9 feed accounts
= 25 remaining accounts
```

The execute and close instructions together resolve to 50 unique transaction
accounts after Solana message deduplication and address lookup table resolution.
If all nine feed accounts were removed from this transaction and no replacement
account were added, the projected count would be:

```text
50 - 9 = 41 unique accounts
```

This is an account-set projection, not a replay of a modified transaction. A
production design may need one additional buffer or metadata account unless that
metadata fits safely in the existing Oracle account.

Public Solana RPC was used to decode the transaction and the referenced Store
account. The Store account used by the transaction is
`CTDLvGGXnoxvqLyTpGzdGLg9pD6JexKxKXSV8tqqo8bN`.

## Current GLV price flow

The account layout is derived and validated in two places:

- SDK deposit and withdrawal builders collect every index/swap token, resolve its
  configured feed, and append feed accounts after the GLV Market and mint arrays;
- on-chain `Glv::validate_and_split_remaining_accounts` validates the Market and
  mint arrays and derives the canonical ordered token list from those validated
  markets and the action.

Both GLV execution instructions then call:

```rust
Oracle::with_prices(store, token_map, tokens, remaining_accounts, execute)
```

`with_prices` performs the following lifecycle in one instruction:

1. requires the Oracle to be cleared and its price map to be empty;
2. takes the first `tokens.len()` remaining accounts as feed accounts;
3. parses and validates one price for every required token;
4. stores the validated prices and aggregate timestamp/slot range in the Oracle;
5. runs deposit or withdrawal execution using those prices;
6. clears every price before returning, including when feed validation fails.

Relevant implementation paths:

- `programs/store/src/states/oracle/mod.rs`;
- `programs/store/src/states/oracle/validator.rs`;
- `programs/store/src/instructions/glv/deposit.rs`;
- `programs/store/src/instructions/glv/withdrawal.rs`;
- `programs/store/src/ops/glv.rs`;
- `programs/store/src/states/glv.rs`;
- `crates/sdk/src/client/ops/exchange/glv_deposit.rs`;
- `crates/sdk/src/client/ops/exchange/glv_withdrawal.rs`.

The SDK feed collection is not trusted as the definition of required prices. The
program derives the required token set from validated on-chain accounts. Any
preloading design must preserve that property.

## Freshness and time constraints

There are two different freshness layers.

### Feed acceptance

`PriceValidator` validates each feed when its price is loaded:

```text
adjusted timestamp = oracle timestamp - token/provider timestamp adjustment
adjusted timestamp + oracle_max_age >= current chain time
oracle timestamp <= current chain time + oracle_max_future_timestamp_excess
max adjusted timestamp - min adjusted timestamp <= oracle_max_timestamp_range
```

The repository defaults are:

| Configuration | Default |
|---|---:|
| `oracle_max_age` | 3,600 seconds |
| `oracle_max_timestamp_range` | 300 seconds |
| `oracle_max_future_timestamp_excess` | 0 seconds |
| `request_expiration` | 3,600 seconds |

The deployed Store used by the reference transaction was decoded as:

| Configuration | Deployed value |
|---|---:|
| `oracle_max_age` | 3,600 seconds |
| `oracle_max_timestamp_range` | 300 seconds |
| `oracle_max_future_timestamp_excess` | 0 seconds |
| `request_expiration` | 1,800 seconds |

Therefore a feed accepted for that Store may be up to 3,600 seconds old at load
time, while all prices loaded into one Oracle batch must be within a 300-second
timestamp range. Token-specific timestamp adjustments are applied before age and
range aggregation.

### Action execution

Feed acceptance is not sufficient for GLV execution. Deposit and withdrawal also
call `Oracle::validate_time` with the action as validator. This requires:

- the minimum oracle timestamp to be at or after the action's `updated_at`;
- the minimum oracle slot to be at or after the action's `updated_at_slot`;
- the maximum oracle timestamp to be at or before
  `updated_at + request_expiration`.

For the reference Store, the last condition gives a 1,800-second execution window.
The action timestamp and slot checks are important: a feed can satisfy the
3,600-second Store-wide age limit but still be invalid for an action created more
recently.

### Standalone GLV valuation

`get_glv_token_value` is different from execution. It accepts a caller-supplied
`max_age` and applies `MaxAgeValidator` after loading feeds. The SDK builder defaults
this argument to 120 seconds.

This 120-second value is not the deposit/withdrawal execution feed limit. Feed
parsing still applies Store-level validation first, so effective standalone
valuation freshness is bounded by both the Store configuration and the supplied
`max_age`.

## Existing persistent-price capability

The Store program already exposes `set_prices_from_price_feed`. It:

- requires the caller to be the Oracle authority and have the
  `ORACLE_CONTROLLER` role;
- requires the Oracle to be cleared;
- validates feeds using the same `PriceValidator`;
- leaves validated prices in the Oracle instead of clearing them.

`clear_all_prices` separately clears that state.

This proves that the Oracle account can physically persist validated prices across
transactions. It does not make current GLV execution compatible with preloading:

- `ExecuteGlvDeposit` and `ExecuteGlvWithdrawal` always call `with_prices`;
- `with_prices` rejects an Oracle that already contains prices with
  `PricesAreAlreadySet`;
- both instructions still interpret the first required remaining accounts as live
  feeds.

The existing persistent instruction is consequently a useful primitive, not a
drop-in way to omit GLV execution feeds.

## Feasibility

### Separate preload and execution transactions

Removing feed accounts from GLV execution is technically feasible if prices are
loaded in an earlier transaction and execution consumes the populated Oracle
without parsing feeds again.

The minimum flow is:

```text
prepare/begin buffer
        |
        v
append validated feed batches
        |
        v
execute GLV action using the bound buffer
        |
        v
consume and clear
```

Feed loading can be split over several transactions if a future GLV requires more
feed accounts than one transaction can carry. Validation cannot be considered
complete until the exact required token set is present.

### Preload and execution in one transaction

Putting `set_prices_from_price_feed` and GLV execution into separate instructions
inside the same transaction does not solve the account-lock problem. Solana resolves
and locks the union of accounts used by every instruction in the transaction. The
feed accounts remain in the transaction account set even when only the first
instruction reads them.

The feed-loading transaction must therefore be separate from the execution
transaction to remove feed accounts from execution's resolved account count.

### Recommended interface strategy

Preserve the existing deposit and withdrawal instructions and add versioned
execution variants that consume preloaded prices. Existing clients retain the
current atomic load/execute/clear behavior, while account-constrained clients can
opt into the multi-transaction lifecycle.

Conceptually:

```text
execute_glv_deposit_v2(..., oracle_buffer_generation)
execute_glv_withdrawal_v2(..., oracle_buffer_generation)
```

The exact wire shape should be chosen with the state design. A generation/nonce can
be passed as an argument if all binding metadata fits in the existing Oracle
reserved space. Otherwise a dedicated preparation account may be clearer, at the
cost of one execution account.

The new execute path must:

1. derive its required token set from validated Market/action accounts;
2. verify the prepared set matches it exactly;
3. validate action, GLV, authority, generation, timestamp, slot, and expiry binding;
4. run the same economic operation used by the existing instruction;
5. make the preparation unusable after successful execution or terminal
   cancellation.

The current instructions should not silently switch semantics during an upgrade.
A versioned interface makes mixed client/program deployments fail predictably.

## Required buffer binding

A reusable Store-wide price cache is unsafe. A prepared buffer must be bound to at
least:

- Store;
- Oracle authority/controller;
- action kind: GLV deposit or GLV withdrawal;
- action account address;
- GLV address;
- canonical required token-set hash and token count;
- aggregate minimum oracle timestamp;
- aggregate maximum oracle timestamp;
- aggregate minimum oracle slot;
- generation or nonce;
- preparation expiry;
- lifecycle status.

The canonical token-set hash must be computed over the exact ordered token list
derived by the program. It must not be based only on SDK input. This prevents a
buffer prepared for a smaller, stale, or differently ordered portfolio from being
used after GLV/action state changes.

Binding directly to the action account also prevents another keeper or user from
front-running a prepared buffer into a different deposit or withdrawal. It is not
enough to bind only to a GLV because concurrent actions may have different swap
paths and required token sets.

## Lifecycle and failure behavior

The lifecycle needs explicit behavior for every terminal and nonterminal result.

### Successful execution

After a successful deposit or withdrawal, mark the generation consumed and clear
the price map. The same prepared prices must not execute another action or replay
the same action.

### Cancel-on-execution-error

Current GLV execution may convert selected operation failures into action
cancellation when `throw_on_execution_error` is false. That is a terminal result:
the preparation should be consumed and cleared after cancellation completes.

### Hard instruction failure

Solana rolls back all writes made by a failed transaction. Clearing the buffer
inside an execution instruction is therefore not persistent if the instruction
returns an error. The preparation remains populated after the failed transaction.

The protocol needs one or both of:

- an idempotent retry path that can safely reuse the same generation only for the
  same pending action;
- an explicit permissioned or permissionless expiry cleanup instruction.

Cleanup must verify that the preparation expired or that the bound action can no
longer execute. It must not let an unrelated caller erase an active keeper's
preparation immediately.

### Partial preload

If feeds are appended in batches, a partial preparation must never be consumable.
Use explicit status such as:

```text
Empty -> Preparing -> Ready -> Consumed/Expired
```

Finalization must compare the complete canonical token set and only then mark the
buffer `Ready`.

## Concurrency and operational risks

### Writable Oracle contention

The Oracle is writable during preparation and execution. One shared Oracle permits
only one in-flight preparation and serializes otherwise independent actions.
Multiple keeper-owned Oracle accounts can reduce contention, but each account must
still have an authenticated authority and isolated generation state.

### Front-running and griefing

Only authorized controllers should be able to prepare or replace a buffer. Binding
to the action and generation prevents a third party from consuming it for another
action. Replacement rules must prevent one authorized worker from unexpectedly
invalidating another worker's ready preparation without an explicit generation
transition.

### Ambiguous transaction results and retries

A client must query the action state and buffer generation after an RPC timeout. It
must not assume execution failed and overwrite the buffer. Retry logic should
distinguish:

- preparation missing;
- preparation partial;
- preparation ready;
- action executed but cleanup observation delayed;
- action cancelled;
- preparation expired.

### Provider-specific feed handling

Pyth, Switchboard, and custom/Chainlink feed accounts have different account
parsers and off-chain preparation flows. Preloading should reuse
`Oracle::set_prices_from_remaining_accounts` and `PriceValidator`, rather than add a
second provider-validation implementation.

The SDK must retain the current provider-to-account resolution when building preload
transactions. Only the instruction that receives those feed accounts changes.

## Security properties that must remain unchanged

The optimization must not weaken these current properties:

- all required tokens are derived and checked on-chain;
- every price is validated against the active Token Map configuration;
- disabled token configs are rejected;
- provider-specific timestamp adjustment is applied;
- every feed satisfies Store max age and future timestamp limits;
- the complete batch satisfies the maximum timestamp range;
- execution prices are not older than the action timestamp or slot;
- execution prices do not exceed the action expiration time;
- prices cannot be reused for an unrelated or later action;
- deposit and withdrawal use the same maximize/minimize and PnL rules as today.

## Account-count impact

The exact saving is the number of distinct feed accounts not already referenced
elsewhere in the transaction, minus any new preparation metadata account added to
execution.

For the reference transaction:

| Shape | Resolved unique accounts |
|---|---:|
| Current deposit plus close | 50 |
| Projected without 9 feed accounts | 41 |
| Projected without feeds, with one new buffer metadata account | 42 |

This creates useful headroom and can move feed validation into multiple transactions.
It does not remove GLV portfolio scaling:

```text
N Market accounts + N market-token mint accounts
```

still remain for a GLV with `N` contributing markets. Feed preloading should be
combined with active-balance valuation and adaptive close splitting; it is not a
standalone path to a 96-active-market GLV.

## Required tests

### Freshness and range

1. Accept a feed exactly at the configured 3,600-second age boundary.
2. Reject a feed older than 3,600 seconds with `MaxPriceAgeExceeded`.
3. Accept a complete batch with an adjusted timestamp range of exactly 300 seconds.
4. Reject a batch whose adjusted range exceeds 300 seconds.
5. Reject a future timestamp beyond `oracle_max_future_timestamp_excess`.
6. Cover nonzero token/provider timestamp adjustments.

### Action binding

1. Reject prices whose timestamp is before action `updated_at`.
2. Reject prices whose slot is before action `updated_at_slot`.
3. Accept and reject the exact 1,800-second deployed request-expiration boundary.
4. Reject a deposit buffer for a withdrawal, another action, or another GLV.
5. Reject a stale token-set hash after a GLV balance/config or swap-path change.
6. Reject missing, extra, duplicated, and reordered tokens.
7. Reject an old or already consumed generation.

### Lifecycle

1. Preload all feeds, execute successfully, and prove the buffer cannot be reused.
2. Preload partially and prove execution cannot consume it.
3. Preload successfully, trigger a hard execution failure, then safely retry the
   same action or clean up after expiry.
4. Exercise cancel-on-execution-error and prove the preparation is terminally
   consumed.
5. Test concurrent preparations against one Oracle and against separate
   keeper-owned Oracles.
6. Test ambiguous RPC results followed by state-based retry.

### Economic equivalence

For identical bank and oracle inputs, compare current and preloaded variants for:

- direct GM-token GLV deposit;
- token deposit with long and short swap paths;
- direct GLV withdrawal;
- withdrawal with long and short swap paths;
- zero and nonzero balances in the current market;
- maximum and minimum price-side selection;
- PnL caps, price impact, fees, and emitted values.

Minted GLV amount, burned GLV amount, token outputs, market/GLV balances, fees, and
events must be identical.

### Account measurements

Compile v0 messages with production-style lookup tables and report resolved unique
accounts for:

- current load-and-execute;
- preloaded execute;
- each shape with merged and split close;
- baseline and heavy deposit/withdrawal routes;
- the reference transaction shape, confirming approximately `50 -> 41` when no new
  execution account is required.

Tests must also enforce the independent 1,232-byte transaction packet limit.

## Recommendation

Implement preloaded Oracle consumption only as an optional, versioned GLV execution
path.

The design is feasible and removes the feed-account term from deposit/withdrawal
execution, but the existing `set_prices_from_price_feed` instruction is not enough
by itself. The production design requires an action-bound, exact-token-set,
generation-controlled, expiring, one-time preparation lifecycle with explicit
failure cleanup and retry semantics.

Treat the feature as an account-headroom optimization. It does not solve the
remaining `Market + mint` linear scaling and must not weaken the current atomic
market-state valuation rules.
