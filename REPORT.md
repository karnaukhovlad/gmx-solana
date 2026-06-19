# GLV Market-Limit Investigation

## Table of contents

- [Submission note](#submission-note)
- [Report ①: Independent Analysis](#report-①-independent-analysis)
  - [1. Root cause](#1-root-cause)
    - [1.1 Observed failure](#11-observed-failure)
    - [1.2 Solana mechanism](#12-solana-mechanism)
    - [1.3 How I confirmed the diagnosis](#13-how-i-confirmed-the-diagnosis)
  - [2. Solution design](#2-solution-design)
    - [2.1 Recommended solution](#21-recommended-solution)
    - [2.2 Alternatives considered](#22-alternatives-considered)
- [Report ②: AI Reflection](#report-②-ai-reflection)
  - [Part A: How AI improved my analysis](#part-a-how-ai-improved-my-analysis)
  - [Part B: Where I think AI misled me](#part-b-where-i-think-ai-misled-me)
  - [Part C: Judgment AI cannot provide](#part-c-judgment-ai-cannot-provide)

## Submission note

- Actual time spent: 11 (6 hours independent + 4 hours with AI + 1 hours report writing)
- AI tools used: Codex, Claude Code

---

# Report ①: Independent Analysis

## 1. Root cause

### 1.1 Observed failure

The root cause in the current architecture is the 64-account limit per
transaction. This is an artificial runtime constraint designed to mitigate
**account lock contention**. Without it, locking too many accounts would degrade
overall network throughput.

### 1.2 Solana mechanism

Solana rejects a transaction during the validation phase before instruction
execution when the resolved transaction message contains more unique account
keys than the bank's active account-lock limit.

Specifically, within `solana-sdk`,
`SanitizedTransaction::validate_account_locks` checks if
`message.account_keys().len() > tx_account_lock_limit`. If this condition is
breached, the runtime aborts the transaction immediately and throws a
`TransactionError::TooManyAccountLocks` error.

This is an artificial constraint designed to prevent extensive account locking
during transaction execution, which would otherwise degrade overall network
throughput. Since Solana natively supports parallel smart contract execution via
Sealevel, accounts requested with read-only permissions within the same slot can
be processed concurrently, whereas write-locked accounts are queued
sequentially. Feature `9LZdXeKGeBV6hRLdxS1rHbHoEUsKqesCC2ZAPTPKJAbK` is
already implemented to raise this limit to 128, but it has not yet been
activated on mainnet-beta or devnet.

It is critical to distinguish this resource exhaustion from other system constraints:

- **Serialized transaction size:** Transactions remained strictly underneath the maximum 1,232-byte `PACKET_DATA_SIZE` protocol packet ceiling.
- **Address Lookup Table (ALT) capacity:** An ALT compresses the serialized layout size by substituting a 32-byte public key with a 1-byte index, but it does *not* reduce the runtime account load burden. Once the addresses are fully resolved by the validator, they still lock underlying state and count against the 64-account limit.
- **GLV account storage capacity:** On-chain GLV account structures possess a `Glv::MAX_ALLOWED_NUMBER_OF_MARKETS` configuration set to 96 markets. The issue is an execution-layer bottleneck, not data layout or storage array constraints.

### 1.3 How I confirmed the diagnosis

I write test that in `tests/anchor_test/glv.rs` to test transaction shapes, when
we use instruction with account more than 64 it staty failing. You can find
summare and full log output for test runs with different validator setting with
default 64 limittaion and with 128 account limitattion which can be used only on
localnet `report/report-test-run-64` and `report/report-test-run-128`.

You can run test by next command:

#### Localnet with 64 limit

```bash
bash scripts/reproduce_glv_account_limit.sh --local
```

#### Localnet with 128 limit

```bash
bash scripts/reproduce_glv_account_limit.sh --local --active-feature
```

## 2. Solution design

### 2.1 Recommended solution

After a detailed review of the `remaining_accounts` and the commit history
regarding the account layout for the `execute_glv_deposit` instruction, I
concluded that the most efficient and cost-effective approach—requiring minimal
development overhead and auditing—is to eliminate the explicit `price_feed`
accounts. Refactor oracle interaction so that `execute_glv_deposit` and
`execute_glv_withdrawal` no longer receive explicit `price_feed` accounts.
Instead, an SDK or keeper workflow should update or prepare the required token
prices immediately before submitting the core GLV instruction.

This optimization could scale the market capacity from ~12 to ~18.

This optimization can be achieved by refactoring the oracle interaction.
Currently, instructions demand `price_feed` accounts to fetch up-to-date token
prices from oracle providers, which update asynchronously every $N$ seconds.
Pyth, for instance, guarantees updates every 30 seconds, 60 seconds, or 4
minutes—sometimes faster, sometimes slower. Inside the program, there is a
staleness check for the oracle data enforced by a `max_age = 3600` seconds
threshold.

The program must continue to validate that every required token has a sufficiently
fresh, correctly identified price. The prepared oracle state should be bound to the
intended GLV action and protected against omission, substitution, expiry, replay,
and unsafe reuse.

The exact supported envelope must be measured from the final compiled transaction
because feed reuse, route accounts, virtual inventories, optional accounts, and
key deduplication change the result.

### 2.2 Alternatives considered

It may also be possible to eliminate the virtual inventory accounts. However, I
have not yet had enough time to fully examine this protection mechanism or
understand its purpose. It may not need to be executed as part of the deposit
transaction. Instead, pool liquidity could potentially be checked before a
deposit or withdrawal, triggering this mechanism only when necessary and then
proceeding with the operation.

| Option | Benefit | Cost/risk | Decision |
| --- | --- | --- | --- |
| Split execution and close instructions into separate transactions when the merged transaction exceeds the account limit | Immediately removes accounts unique to the close instruction; in the reproduced heavy cases, deposit falls from 65 to 60 accounts and withdrawal from 68 to 61 | Adds a second transaction and fee, and execution may succeed while close fails, requiring idempotent retries, recovery, monitoring, and an explicit `executed, close pending` status | Implement as immediate SDK hardening, but not as the complete scaling solution because execution still grows with the number of markets |
| Remove price-feed accounts from core GLV execution and preload prices | Saves an account for each unique feed and requires relatively contained program and SDK changes | Introduces a multi-transaction lifecycle, oracle-state binding, expiry, retry, and possible slippage concerns | Recommended, subject to lifecycle and security validation |
| Remove or defer virtual-inventory accounts | Could recover additional account headroom | The defensive mechanism and its economic/security role have not been fully evaluated | Investigate separately; do not implement without a complete invariant review |
| Wait for activation of the 128-account runtime feature | Potentially doubles the protocol account-lock ceiling without changing GLV accounting | Feature `9LZdXeKGeBV6hRLdxS1rHbHoEUsKqesCC2ZAPTPKJAbK` is not active on mainnet-beta or devnet, and protocol rollout is outside application control | Do not rely on it as the primary solution |

---

# Report ②: AI Reflection

## Part A: How AI improved my analysis

### 1. It improved the test methodology and clarified how market count affects account usage

AI helped me test different transaction shapes and determine their precise
account-count boundaries. This showed that there is no universal 12-market limit:
the threshold depends on the operation, routing complexity, and whether close is
merged with execution.

#### Withdrawal boundary summary

| Situation | Maximum `N` that fits | Account count at maximum | First failing `N` |
| --- | ---: | ---: | ---: |
| Baseline execution only | 18 | 63 | 19 |
| Baseline + close | 15 | 64 | 16 |
| Heavy execution only | 13 | 63 | 14 |
| Heavy + close | 10 | 64 | 11 |

#### Deposit boundary summary

| Situation | Maximum `N` that fits | Account count at maximum | First failing `N` |
| --- | ---: | ---: | ---: |
| Baseline execution only | 22 | 64 | 23 |
| Baseline + close | 19 | 63 | 20 |
| Heavy execution only | 14 | 64 | 15 |
| Heavy + close | 11 | 63 | 12 |

### 2. It identified the likely root cause during the first iteration

AI identified Solana's account-lock limit immediately, while I needed several
hours to verify it through code analysis and reproducible tests.

## Part B: Where I think AI misled me

### 1. It proposed loading only markets with nonzero GLV balances, plus the current target

This solution does not fit the intended product model because a properly managed GLV
should not retain zero-balance markets. Such markets should be replaced or treated
as configuration errors, so filtering them out does not materially increase
production capacity.

### 2. It proposed deploying multiple GLVs and routing between them at the SDK/UI layer

This approach changes the product rather than solving the limitation of one GLV. It
fragments liquidity and share tokens, complicates routing and integrations, and
prevents atomic rebalancing across the complete portfolio. It is therefore unsuitable
when one unified GLV is required.

## Part C: Judgment AI cannot provide

AI can generate or research standard boilerplate code in seconds, but it cannot
understand how to structure account data or which architectural solution is better
for avoiding future bottlenecks unless this is clearly explained in the context.
In this task, this resulted in endless attempts to optimize non-zero GLV balances,
even though such balances should not exist at all. The AI also kept trying to modify
test cases to match what it believed was the expected result, which was almost never
correct.

Writing test cases is a good example of an area where AI is still weak and often
produces incorrect results. In most cases, it cannot determine whether the problem
is in the test case, the input data, the output data, or the execution logic. Even
when it identifies the issue correctly, its fixes often break other parts of the
system.
