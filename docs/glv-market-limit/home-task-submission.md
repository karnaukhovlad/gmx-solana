# GLV Market-Limit Investigation

## Submission note

- Actual time spent: `[INDEPENDENT_HOURS]` hours independently + `[AI_HOURS]` hours with AI
- AI tools used: `[AI_TOOLS_USED]`
- Author uncertainties: `[AUTHOR_UNCERTAINTIES]`

---

# Report ①: Independent Analysis

## 1. Executive summary

`[INDEPENDENT_EXECUTIVE_SUMMARY]`

State:

- the exact failure mechanism;
- why the observed threshold is approximately 12 markets rather than a GLV constant;
- the recommended solution;
- the most important limitation or uncertainty in the recommendation.

## 2. Root cause

### 2.1 Observed failure

`[INDEPENDENT_FAILURE_DESCRIPTION]`

Include the exact RPC/runtime error, whether the Store program starts executing,
and which GLV operations are affected.

### 2.2 Solana mechanism

`[INDEPENDENT_SOLANA_MECHANISM]`

Explain which transaction resource is exhausted and when Solana validates it.
Distinguish it from:

- serialized transaction size;
- compute-unit limits;
- Address Lookup Table capacity;
- Anchor `remaining_accounts`;
- GLV account storage capacity.

### 2.3 Why the threshold is approximately 12

`[INDEPENDENT_ACCOUNT_COUNT_MODEL]`

Provide a formula derived from the independently inspected account layout:

```text
[INDEPENDENT_FORMULA]
```

Explain which terms are fixed, which scale with the GLV portfolio, which depend on
unique oracle tokens or swap routes, and which accounts are deduplicated.

### 2.4 How I confirmed the diagnosis

`[INDEPENDENT_CONFIRMATION_METHOD]`

Record the exact source paths, Solana documentation, runtime source, commands,
cluster configuration, and transaction observations used independently.

| Transaction shape | Markets | Unique accounts | Serialized bytes | Expected result | Actual result |
|---|---:|---:|---:|---|---|
| `[CASE_1]` | `[N]` | `[COUNT]` | `[BYTES]` | `[RESULT]` | `[RESULT]` |
| `[CASE_2]` | `[N]` | `[COUNT]` | `[BYTES]` | `[RESULT]` | `[RESULT]` |
| `[CASE_3]` | `[N]` | `[COUNT]` | `[BYTES]` | `[RESULT]` | `[RESULT]` |
| `[CASE_4]` | `[N]` | `[COUNT]` | `[BYTES]` | `[RESULT]` | `[RESULT]` |

## 3. Solution design

### 3.1 Recommended solution

`[INDEPENDENT_RECOMMENDATION]`

Describe:

- the program and SDK behavior that changes;
- the invariant preserved by the change;
- account savings and the supported operating envelope;
- compatibility and rollout behavior.

### 3.2 Alternatives considered

| Option | Benefit | Cost/risk | Decision |
|---|---|---|---|
| `[OPTION_1]` | `[BENEFIT]` | `[COST_OR_RISK]` | `[DECISION]` |
| `[OPTION_2]` | `[BENEFIT]` | `[COST_OR_RISK]` | `[DECISION]` |
| `[OPTION_3]` | `[BENEFIT]` | `[COST_OR_RISK]` | `[DECISION]` |

### 3.3 Minimal code or measurement

The committed reproduction originally lived in:

- `tests/anchor_test/glv.rs`
- `scripts/reproduce_glv_account_limit.sh`

The boundary-test version was introduced in commit `5fef9157`. The investigation
documents were committed in `00ea8f80`. The current working-tree test has since
been repurposed, so use the committed version when independently reproducing the
original measurement.

Original reproduction command:

```sh
GMSOL_TEST=glv_unique_account_limit_is_transaction_shape_dependent \
GMSOL_RNG=64012 \
EXTRA_CARGO_ARGS=--nocapture \
RUST_LOG=info \
anchor test --skip-build -- --features mock --features devnet,test-only,migration
```

Independent observations:

`[INDEPENDENT_REPRODUCTION_RESULTS]`

## 4. Production and audit risks

### Hardest part of the fix

`[INDEPENDENT_HARDEST_PART]`

### Failure modes most likely to be overlooked

`[INDEPENDENT_FAILURE_MODES]`

At minimum, consider incomplete account sets, account ordering, zero/nonzero balance
transitions, oracle consistency, retries after ambiguous RPC results, CPI callers,
and compatibility between old and new clients.

## 5. Validation plan

`[INDEPENDENT_VALIDATION_PLAN]`

Define acceptance criteria for:

- the exact transaction-account boundary;
- deposit and withdrawal economic equivalence;
- adversarial account ordering and omission;
- oracle freshness and token-set validation;
- execution/close recovery;
- downstream CPI callers;
- supported maximum market and route shapes.

## 6. Independent conclusion and uncertainties

`[INDEPENDENT_CONCLUSION]`

`[AUTHOR_UNCERTAINTIES]`

Independent time spent: `[INDEPENDENT_HOURS]` hours.

---

# Report ②: AI Reflection

## Part A: What AI helped me improve

### 1. It made the root cause precise

The initial shorthand was that GLV used too many accounts once it reached roughly
12 markets. AI pushed the analysis down to the actual runtime mechanism: Solana
rejects a transaction when its resolved unique account set exceeds the currently
enforced account-lock limit of 64. Static keys and ALT-loaded keys are resolved
before this check, and rejection occurs before the Store program executes.

**My judgment:** I agree. This turns an observed product limit into a falsifiable
runtime diagnosis and explains the absence of GMX program logs.

### 2. It separated independent Solana limits

AI distinguished the 64-account runtime limit from the 1,232-byte serialized
transaction limit. Address Lookup Tables reduce wire size, but they do not reduce
the number of accounts loaded and locked by the runtime.

The committed reproduction supports this distinction: rejected messages contained
65 and 68 unique accounts while remaining only 784 and 1,029 bytes respectively.

**My judgment:** I agree. Treating ALTs as an account-count solution would lead to
the wrong remediation.

### 3. It replaced a fixed market threshold with a transaction-shape model

The improved model is:

```text
resolved unique accounts
  = fixed/action accounts
  + 2 × portfolio markets
  + unique feed accounts
  + route and virtual-inventory accounts
  + optional close accounts
  - duplicated keys
```

The exact implementation can also be expressed using `2 × (N - 1)` because the
current target market and mint already appear in the fixed execute accounts.

This explains why the same 12-market GLV produced account counts from 49 to 68.
There is no on-chain check that rejects market 13 specifically.

**My judgment:** I agree, with one qualification: this is a budgeting framework,
not one universal closed-form formula. Feed reuse, route membership, optional
accounts, and message deduplication must be measured on the final compiled message.

### 4. It identified bounded fixes that preserve current valuation semantics

AI suggested two relatively contained changes:

1. Compile the final versioned message and split the close instruction when merging
   it would exceed the account budget.
2. Value only markets with a nonzero recorded GLV balance, while always including
   the action's target market.

The first suggestion is supported by the committed measurements:

| Transaction shape | With close | Execute only |
|---|---:|---:|
| Heavy deposit | 65, rejected | 60, passed |
| Heavy withdrawal | 68, rejected | 61, passed |

The second suggestion uses the balance already recorded in each
`GlvMarketConfig`. It can remove empty configured markets without omitting any
current contribution to NAV.

**My judgment:** I agree with both as staged improvements. Split close is proven to
recover specific transaction shapes. Active-market filtering remains a proposed
program change and must be validated on-chain; the SDK must not be trusted to
choose which value-bearing markets are included.

### 5. It exposed why a simple `2N → 1N` optimization is unsafe

Each portfolio market currently contributes two independent live inputs:

- a `Market` account containing pool, PnL, open-interest, fee, and configuration
  state;
- the SPL market-token mint containing current supply.

AI checked whether mint supply could simply be cached in GLV or Market state. That
is unsafe under current semantics because a holder can burn standard SPL market
tokens directly without invoking Store. A GLV-only cached value can therefore
diverge from live mint supply and make GLV pricing disagree with ordinary GM
pricing.

A viable one-account-per-market design would require a protocol-wide definition of
Store-issued supply, migration of existing markets, and consistent use by every
pricing path. Direct holder burns would become abandoned shares rather than
reducing canonical issued supply.

**My judgment:** I agree with the diagnosis. I would not treat this as an account
optimization; it is an economic and compatibility change requiring its own design
review and audit.

### 6. It made oracle preloading concrete

AI observed that feed accounts can be removed from the execute transaction only if
prices are loaded in a separate transaction. Loading prices in an earlier
instruction of the same transaction does not help because Solana locks the union
of all transaction accounts.

It also identified the minimum safety binding for a prepared price buffer:

- action and action kind;
- GLV;
- authority;
- canonical required-token-set hash;
- timestamp and slot range;
- expiry;
- generation or nonce;
- one-time consumption and cleanup behavior.

**My judgment:** I agree that this is technically viable account headroom. I do not
consider it a primary fix until the multi-transaction lifecycle, contention, and
failure recovery have been evaluated operationally.

## Part B: Where I think AI misled me

### 1. The early `~26 + 3N` formula was too confident

An early answer modeled deposit and withdrawal as approximately a fixed base plus
three accounts per market and implied that 12 markets fit while 13 markets fail.
That model treated an oracle feed as if it were inherently one-per-market.

This is not generally correct. The invariant portfolio layout is one Market account
and one market-token mint per configured market: `2N` raw portfolio references.
Oracle cost is based on unique required tokens and configured providers. Several
markets can share an index token or feed. Route and virtual-inventory accounts can
also add discontinuous costs.

The committed fixture deliberately made all 12 synthetic index tokens share one
feed. Its 12-market transactions ranged from 49 to 68 accounts, proving that neither
`3N` nor a universal 12/13 boundary describes every operation.

### 2. The proposed 20-active-market guarantee was not demonstrated

A later AI plan combined active-market filtering, oracle preloading, and split close
and described support for 20 active markets. That may be a plausible target, but it
was inferred from account subtraction rather than confirmed by end-to-end
transactions covering the promised route and oracle shapes.

The actual count depends on fixed instruction accounts, current-market
deduplication, distinct feeds, swap paths, virtual inventories, optional token
accounts, CPI forwarding, and whether close is merged. A production guarantee
cannot be based only on a projected formula. It needs final compiled-message
measurements and tests for every supported envelope.

### 3. The modified test can be mistaken for proof of active filtering

The current uncommitted test is named
`glv_omits_inactive_markets_from_execution_accounts` and expects route-heavy
transactions with inactive configured markets to execute. However, the current
on-chain parser still calculates:

```rust
let len = self.num_markets();
```

and requires `len` Market accounts followed by `len` mint accounts. The SDK also
collects `glv.market_tokens()` when constructing the execution hint.

Therefore the working-tree test expresses a desired future behavior, not an
implemented production invariant. The valid root-cause evidence is the committed
boundary-test version in `5fef9157` and its recorded results. Treating the modified
test as current proof would reverse the status of proposal and implementation.

## Part C: Judgment AI cannot provide

### 1. Whether split-close recovery is acceptable operationally

The account saving is measurable, but accepting the design requires experience with
keeper retries and Solana RPC ambiguity. Execute may succeed while the client times
out, and close may then fail independently. The SDK must distinguish:

- execution not submitted;
- execution submitted with unknown result;
- action completed but close pending;
- action cancelled;
- close completed.

Whether that extra state and operational burden is acceptable depends on the real
keeper, monitoring, fee, and incident-response environment.

### 2. Whether multiple GLVs are preferable to changing NAV semantics

Multiple GLVs preserve atomic live valuation but fragment liquidity and create
separate share tokens. Cached or batched NAV can preserve one broad portfolio but
introduces stale or mixed-state pricing, invalidation, locking, or trust
assumptions.

Choosing between them is an economic and product decision informed by expected
portfolio composition, liquidity distribution, user routing, rebalancing behavior,
and attack tolerance—not only by account-count arithmetic.

### 3. Whether Store-issued supply is an acceptable definition

Using Store-issued supply could remove mint accounts from GLV valuation, but it
changes the effect of direct SPL burns. A holder burning GM tokens would abandon
the claim permanently instead of increasing the value represented by each remaining
token.

Only engineers responsible for protocol accounting, integrations, migration, and
auditor expectations can decide whether that semantic change is acceptable.

### 4. How much account headroom production needs

Building exactly to 64 accounts is fragile. A later security check, optional
provider account, token-program migration, or CPI integration can break the
transaction envelope.

The correct reserved headroom depends on the upgrade roadmap and supported
transaction shapes. I would require an explicit operational budget below the
runtime maximum, but the exact number must be selected from production measurements
and future interface plans.

### 5. Whether prepared oracle sessions are operationally viable

Oracle preloading introduces writable shared state, possible keeper contention,
front-running considerations, abandoned buffers, expiry cleanup, and retry rules.
The security checks can be specified, but practical viability depends on real
keeper concurrency, oracle update cadence, transaction inclusion latency, and the
frequency of actions that become invalid between preparation and execution.

## Evidence used during the AI-assisted review

The committed 12-market reproduction recorded:

| Transaction shape | Unique accounts | Result | Serialized bytes |
|---|---:|---|---:|
| Baseline deposit + close | 49 | Passed | 744 |
| Baseline withdrawal + close | 58 | Passed | 878 |
| Heavy deposit + close | 65 | `TooManyAccountLocks` | 784 |
| Heavy deposit, execution only | 60 | Passed | 639 |
| Heavy withdrawal + close | 68 | `TooManyAccountLocks` | 1,029 |
| Heavy withdrawal, execution only | 61 | Passed | 820 |

The rejected transactions produced no GMX program logs, which is consistent with
runtime rejection before program execution. Every message was below the independent
1,232-byte packet limit.

The GLV state supports up to 96 configured markets. That storage capacity is not an
execution guarantee because every current portfolio valuation supplies the complete
configured Market and mint arrays.

## Validation required before accepting a fix

1. Preserve regression cases with exactly 64 and 65 resolved unique accounts using
   v0 messages and ALTs.
2. Count resolved keys from the final compiled message, not raw instruction
   references or static keys alone.
3. Compare deposit, withdrawal, and direct-pricing outputs against the current
   implementation for identical bank and oracle state.
4. For active-market filtering, test:
   - target market initially at zero;
   - full withdrawal to zero;
   - one-unit rounding dust;
   - missing, extra, duplicated, and reordered accounts;
   - stale SDK hints after a balance or configuration update.
5. For split close, test ambiguous send results, idempotent close retries, stranded
   escrow recovery, and monitoring of completed-but-unclosed actions.
6. For oracle preloading, reject missing tokens, wrong action/GLV bindings, stale or
   expired prices, reused generations, and buffers prepared before relevant state
   changes.
7. Update and test every downstream CPI consumer, especially GLV pricing used by
   liquidity-provider staking.
8. Measure baseline and worst supported routes with distinct index feeds, each
   oracle provider, virtual inventories, optional token legs, and merged/separate
   close.
9. Keep both independent limits in acceptance tests:

   ```text
   resolved unique accounts <= configured operational budget <= 64
   serialized transaction size <= 1,232 bytes
   ```

AI-assisted review time: `[AI_HOURS]` hours.
