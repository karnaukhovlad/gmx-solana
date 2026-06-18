# Iteration 007 — account-count formulas for every GLV instruction and action

Date: 2026-06-18

## Purpose

This document gives account-count formulas for:

- each GLV Store instruction;
- each deposit, withdrawal, and shift lifecycle;
- direct GLV pricing;
- liquidity-provider `stake_glv`, which prices GLV through CPI;
- SDK-composed transactions containing ATA preparation, pull-oracle, compute-budget, and close instructions.

Two counts must be kept separate:

1. **Compiled account references**: every instruction account index plus its program ID. The same address can be referenced repeatedly.
2. **Resolved unique accounts**: the deduplicated transaction account-key set after ALT resolution. This is the count checked against Solana's 64-account lock limit.

Only the second count determines `TooManyAccountLocks`.

## Notation

### Portfolio and pricing

| Symbol | Meaning |
|---|---|
| `N` | Number of markets currently required by GLV portfolio valuation. In current code this is every configured GLV market. |
| `F` | Number of feed accounts passed to the consumer instruction. |
| `L` | Number of route-market account references excluding the current market, as constructed by the execute builder. |
| `L_out` | Number of distinct route markets not already present in the fixed or `N`-market portfolio set. |
| `V` | Number of distinct virtual-inventory accounts. |
| `P` | Total create-action path length: `long_path_len + short_path_len`. |
| `P_out` | Distinct create-path market addresses not already present in the fixed create account set. |

`L` is used in raw-reference formulas. `L_out` is used in unique-account formulas because a route market that is already in the `N` portfolio array adds another reference but no new lock.

### Optional action legs

| Symbol | Meaning |
|---|---|
| `G` | `1` when a deposit supplies GM/market tokens directly; otherwise `0`. |
| `D_L` | `1` when a GLV deposit has an initial long-token leg; otherwise `0`. |
| `D_S` | `1` when a GLV deposit has an initial short-token leg; otherwise `0`. |
| `C` | `1` when `chainlink_program` is a distinct account; `0` when the optional account is `None` and aliases the Store program ID. |

An active deposit token leg normally introduces three distinct static accounts:

```text
token mint + source/escrow account + vault/other escrow account
```

The exact three-account grouping differs between create, execute, and close, but each active long or short leg has a `+3` standard contribution.

### Transaction composition

| Symbol | Meaning |
|---|---|
| `B_ref` | Compiled references contributed by compute-budget instructions. In the reproduction this was `2`, because two zero-account compute-budget instructions each reference the Compute Budget program. |
| `B_out` | New unique account keys contributed by compute-budget instructions. Normally `1`: the Compute Budget program ID. |
| `O_ref` | Compiled references contributed by pull-oracle post/close instructions merged into the consumer transaction. |
| `O_out` | Unique oracle-wrapper accounts not already in the consumer's fixed/feed set. |
| `A_out` | Unique accounts introduced by ATA-preparation instructions that are not already used by the core create instruction. |
| `Q_ATA` | Number of idempotent ATA-create instructions merged into a create transaction. Each current SPL ATA-create instruction has six account metas plus one Associated Token program reference, so it contributes seven compiled references. |
| `X_close` | Unique accounts introduced by a merged close instruction that are not already in execute. Formally `|CloseKeys \ ExecuteKeys|`. |
| `D` | Any additional duplicates caused by address aliases, such as owner = receiver, equal long/short tokens, or shared accounts across categories. |

### Generic exact formulas

For instructions `i = 1..k` in one transaction:

```text
raw_references =
    Σ (1 + instruction_i.account_meta_count)
```

The `1` is the instruction's program ID reference.

The exact account-lock count is:

```text
unique_accounts =
    cardinality(
        payer
        ∪ all instruction program IDs
        ∪ all static account metas
        ∪ all ALT-resolved account metas
    )
```

All expanded formulas below are convenient forms of this set-cardinality rule.

## Summary table: individual Store instructions

`Meta refs` counts Anchor/IDL account metas and remaining accounts. `Compiled refs` additionally counts the executing program ID once for the instruction.

| Instruction | Meta-reference formula | Compiled-reference formula | Standard unique-account formula |
|---|---:|---:|---:|
| `initialize_glv` | `8 + 3N` | `9 + 3N` | `9 + 3N` |
| `update_glv_config` | `3` | `4` | `4` |
| `update_glv_market_config` | `4` | `5` | `5` |
| `toggle_glv_market_flag` | `4` | `5` | `5` |
| `insert_glv_market` | `9` | `10` | `10` |
| `remove_glv_market` | `10` | `11` | `11` |
| `create_glv_deposit` | `21 + P` | `22 + P` | see deposit section |
| `execute_glv_deposit` | `24 + 2N + F + L + V` | `25 + 2N + F + L + V` | see deposit section |
| `close_glv_deposit` | `24` | `25` | `18 + 3D_L + 3D_S - D` |
| `create_glv_withdrawal` | `19 + P` | `20 + P` | see withdrawal section |
| `execute_glv_withdrawal` | `25 + 2N + F + L + V` | `26 + 2N + F + L + V` | see withdrawal section |
| `close_glv_withdrawal` | `24` | `25` | `24 - D` |
| `create_glv_shift` | `13` | `14` | `14` |
| `execute_glv_shift` | `17 + F + V` | `18 + F + V` | `16 + C + F + V - D` |
| `close_glv_shift` | `13` | `14` | `13 - D` |
| `get_glv_token_value` | `8 + 2N + F` | `9 + 2N + F` | `8 + 2N + F - D` |

Notes:

- Event-CPI instructions explicitly include the Store program as an account meta named `program`. Their compiled-reference count still adds the instruction program reference, but the unique count does not increase because both references resolve to the same program ID.
- Optional `None` accounts are still account-meta references. The SDK maps them to the Store program ID, so several absent optionals increase raw references but collapse to one unique key.
- The standard unique formulas assume otherwise distinct addresses. Subtract `D` for documented aliases.

## Management instructions

### `initialize_glv`

Remaining accounts:

```text
N market states
N market-token mints
N GLV market-token vaults
```

Formula:

```text
meta refs      = 8 + 3N
compiled refs  = 9 + 3N
unique         = 9 + 3N
```

The extra unique key beyond the eight declared metas is the Store program ID.

This initialization shape reaches 64 when:

```text
9 + 3N <= 64
N <= floor(55 / 3)
N <= 18
```

This does not limit the GLV to 18 markets permanently because additional markets are inserted one at a time.

### Constant-size management instructions

```text
update_glv_config:
    unique = 4

update_glv_market_config:
    unique = 5

toggle_glv_market_flag:
    unique = 5

insert_glv_market:
    unique = 10

remove_glv_market:
    unique = 11
```

These formulas include the Store program ID and assume the payer/authority is already one of the declared accounts.

## GLV deposit lifecycle

### Deposit action types

The deposit supports these input combinations:

| Deposit type | `G` | `D_L` | `D_S` |
|---|---:|---:|---:|
| GM/market-token only | `1` | `0` | `0` |
| Initial long only | `0` | `1` | `0` |
| Initial short only | `0` | `0` | `1` |
| Initial long + short | `0` or `1` | `1` | `1` |
| GM plus one token leg | `1` | `1` or `0` | `0` or `1` |
| GM plus both token legs | `1` | `1` | `1` |

The program rejects an entirely empty deposit.

### `create_glv_deposit`: core instruction

Raw formula:

```text
meta refs      = 21 + P
compiled refs  = 22 + P
```

The standard unique formula before ATA-preparation instructions is:

```text
U_create_deposit_core =
    15
  + G
  + 3D_L
  + 3D_S
  + P_out
  - D
```

The base `15` consists of:

```text
owner
receiver
store
current market
GLV
deposit PDA
GLV mint
market-token mint
GLV-token escrow
market-token escrow
System program
SPL Token program
Token-2022 program
Associated Token program
Store program
```

Common duplicate:

```text
receiver == owner  => D += 1
```

Each inactive optional account still appears in the 21 meta references but aliases the Store program, so it does not add a unique key.

Expanded by deposit input type, before path and duplicate adjustments:

| Deposit input type | Core unique base |
|---|---:|
| GM/market-token only | `16` |
| Initial long only | `18` |
| Initial short only | `18` |
| Initial long + short | `21` |
| GM + initial long | `19` |
| GM + initial short | `19` |
| GM + initial long + short | `22` |

For each row:

```text
U_create_deposit_core =
    table_base
  + P_out
  - D
```

### SDK create-deposit transaction

The SDK merges idempotent ATA-preparation instructions with `create_glv_deposit`.

For the high-level client builder:

```text
Q_ATA = 3 + D_L + D_S
```

The always-prepared accounts are:

1. receiver GLV-token ATA;
2. deposit GLV-token escrow;
3. deposit market-token escrow.

An active long or short leg adds preparation of its escrow.

Raw formula:

```text
R_create_deposit_sdk =
    (22 + P)
  + 7Q_ATA
```

Exact formula:

```text
U_create_deposit_sdk =
    U_create_deposit_core
  + A_out
```

In the normal builder, most prepared escrow ATAs already appear in the core instruction. The receiver's GLV-token ATA does not, so the typical value is:

```text
A_out ≈ 1
```

Use set cardinality rather than assuming `1` if custom account addresses or preparation behavior changes.

### `execute_glv_deposit`: core instruction

Raw formula:

```text
meta refs      = 24 + 2N + F + L + V
compiled refs  = 25 + 2N + F + L + V
```

Standard unique formula:

```text
U_execute_deposit_core =
    17
  + 3D_L
  + 3D_S
  + C
  + 2(N - 1)
  + F
  + L_out
  + V
  - D
```

Why `2(N - 1)`, not `2N`:

- the fixed accounts already contain the current market;
- the fixed accounts already contain the current market-token mint;
- those same two addresses reappear in the portfolio arrays;
- they add references but not unique locks.

If a route market belongs to the GLV portfolio, it contributes to `L` but not `L_out`.

Expanded by deposit input type:

| Execute-deposit type | Core unique formula before route/feed/inventory terms |
|---|---:|
| GM/market-token only | `17 + C + 2(N - 1)` |
| Initial long only | `20 + C + 2(N - 1)` |
| Initial short only | `20 + C + 2(N - 1)` |
| Initial long + short | `23 + C + 2(N - 1)` |
| GM plus token legs | Same row as the active token legs; the GM source is already represented by the mandatory execute escrow accounts |

Complete each row with:

```text
+ F + L_out + V - D
```

and add SDK composition terms `B_out`, `O_out`, and optionally `X_close`.

### SDK execute-deposit transaction without close

The production builder adds compute-budget instructions and can be wrapped by pull-oracle posting/closing instructions:

```text
R_execute_deposit_sdk =
    (25 + 2N + F + L + V)
  + B_ref
  + O_ref
```

```text
U_execute_deposit_sdk =
    U_execute_deposit_core
  + B_out
  + O_out
```

Typical `B_out` is one Compute Budget program key.

### SDK execute-deposit transaction with merged close

```text
R_execute_deposit_plus_close =
    (25 + 2N + F + L + V)
  + 25
  + B_ref
  + O_ref
```

```text
U_execute_deposit_plus_close =
    U_execute_deposit_sdk
  + X_close
```

Do not add the standalone close unique count. Most close accounts overlap execute. `X_close` is the set difference.

In the reproduction:

```text
X_close = 5
```

for the tested deposit shape.

### `close_glv_deposit` standalone

```text
meta refs      = 24
compiled refs  = 25
unique         = 18 + 3D_L + 3D_S - D
```

Common aliases affecting `D`:

- executor = owner;
- owner = receiver;
- long and short token mints are equal;
- corresponding ATA addresses collapse when token and authority are equal.

## GLV withdrawal lifecycle

### Withdrawal action types

Withdrawal always burns GLV tokens and selects one current GM market. Its output shape is:

| Withdrawal type | Long path | Short path |
|---|---|---|
| Direct pool tokens | empty | empty |
| Swap long output | nonempty | empty |
| Swap short output | empty | nonempty |
| Swap both outputs | nonempty | nonempty |
| Custom final tokens | path as required | path as required |

### `create_glv_withdrawal`: core instruction

```text
meta refs      = 19 + P
compiled refs  = 20 + P
```

Standard unique formula:

```text
U_create_withdrawal_core =
    20
  + P_out
  - D
```

Common duplicates:

```text
receiver == owner           => D += 1
final_long == final_short   => mint and derived-account aliases may increase D
```

Route type changes only `P`/`P_out`; it does not change the fixed 20-account unique base:

| Create-withdrawal type | Core unique formula |
|---|---:|
| Direct long and short outputs | `20 - D` |
| Long output swap only | `20 + long_path_out - D` |
| Short output swap only | `20 + short_path_out - D` |
| Both output swaps | `20 + P_out - D` |

### SDK create-withdrawal transaction

The SDK prepares receiver token ATAs and action escrows:

For the high-level client builder:

```text
Q_ATA = 6
```

The six idempotent ATA instructions prepare:

1. receiver final-long ATA;
2. receiver final-short ATA;
3. GLV-token escrow;
4. market-token escrow;
5. final-long escrow;
6. final-short escrow.

Raw formula:

```text
R_create_withdrawal_sdk =
    (20 + P)
  + 7Q_ATA
```

The lower-level configurable builder can skip receiver-ATA preparation, so use its actual `Q_ATA` rather than assuming six.

```text
U_create_withdrawal_sdk =
    U_create_withdrawal_core
  + A_out
```

Normally the escrows are already part of the core instruction. The receiver's final-long and final-short ATAs are not.

Typical values:

```text
different final tokens => A_out ≈ 2
same final token        => A_out ≈ 1
```

### `execute_glv_withdrawal`: core instruction

```text
meta refs      = 25 + 2N + F + L + V
compiled refs  = 26 + 2N + F + L + V
```

Standard unique formula:

```text
U_execute_withdrawal_core =
    24
  + C
  + 2(N - 1)
  + F
  + L_out
  + V
  - D
```

The current market and current market-token mint account for the `-2` implicit in `2(N - 1)`.

Expanded by withdrawal route type:

| Execute-withdrawal type | Core unique formula |
|---|---:|
| Direct pool-token outputs | `24 + C + 2(N - 1) + F - D` |
| Long output swap only | `24 + C + 2(N - 1) + F + L_out + V - D` |
| Short output swap only | `24 + C + 2(N - 1) + F + L_out + V - D` |
| Both output swaps | `24 + C + 2(N - 1) + F + L_out + V - D` |

Long-only, short-only, and both-path formulas have the same algebra; their measured `F`, `L_out`, and `V` values differ.

### SDK execute-withdrawal without close

```text
R_execute_withdrawal_sdk =
    (26 + 2N + F + L + V)
  + B_ref
  + O_ref
```

```text
U_execute_withdrawal_sdk =
    U_execute_withdrawal_core
  + B_out
  + O_out
```

### SDK execute-withdrawal with merged close

```text
R_execute_withdrawal_plus_close =
    (26 + 2N + F + L + V)
  + 25
  + B_ref
  + O_ref
```

```text
U_execute_withdrawal_plus_close =
    U_execute_withdrawal_sdk
  + X_close
```

In the reproduction:

```text
X_close = 7
```

for the tested withdrawal shape.

### `close_glv_withdrawal` standalone

```text
meta refs      = 24
compiled refs  = 25
unique         = 24 - D
```

The largest common source of duplicates is owner/receiver/executor aliasing. Equal final token mints can also collapse mint and ATA keys.

## GLV shift lifecycle

GLV shift does not price all `N` GLV markets. It operates on the selected from/to markets, so it does not contain the `2N` term.

### `create_glv_shift`

```text
meta refs      = 13
compiled refs  = 14
unique         = 14
```

The from/to market-token mints must be different, so their market states, mints, and GLV vaults are normally distinct.

### `execute_glv_shift`: core instruction

```text
meta refs      = 17 + F + V
compiled refs  = 18 + F + V
```

Standard unique formula:

```text
U_execute_shift_core =
    16
  + C
  + F
  + V
  - D
```

With `chainlink_program = None`, its optional meta aliases the explicit Store program account, so `C = 0`.

### SDK execute-shift without close

```text
R_execute_shift_sdk =
    (18 + F + V)
  + B_ref
  + O_ref
```

```text
U_execute_shift_sdk =
    U_execute_shift_core
  + B_out
  + O_out
```

### SDK execute-shift with merged close

```text
R_execute_shift_plus_close =
    (18 + F + V)
  + 14
  + B_ref
  + O_ref
```

```text
U_execute_shift_plus_close =
    U_execute_shift_sdk
  + |CloseShiftKeys \ ExecuteShiftKeys|
```

### `close_glv_shift` standalone

```text
meta refs      = 13
compiled refs  = 14
unique         = 13 - D
```

Unlike create, close is an event-CPI instruction and already has the Store program in its declared account metas.

## Direct GLV pricing

### `get_glv_token_value`

```text
meta refs      = 8 + 2N + F
compiled refs  = 9 + 2N + F
unique         = 8 + 2N + F - D
```

The fixed eight unique accounts are:

```text
authority
store
token map
oracle
GLV
GLV-token mint
event authority
Store program
```

Unlike deposit/withdrawal execution, there is no fixed current market or current market-token mint, so the portfolio contributes the full `2N`.

The count is identical for all pricing modes:

| Pricing mode | Unique formula |
|---|---:|
| `maximize = true` | `8 + 2N + F - D` |
| `maximize = false` | `8 + 2N + F - D` |
| `emit_event = true` | No account-count change; event accounts are already fixed |
| `emit_event = false` | No account-count change |

If pricing is wrapped by pull-oracle instructions:

```text
R_pricing_wrapped =
    (9 + 2N + F)
  + O_ref
  + optional B_ref
```

```text
U_pricing_wrapped =
    (8 + 2N + F - D)
  + O_out
  + optional B_out
```

## Liquidity-provider `stake_glv`

`stake_glv` is an outer instruction in the liquidity-provider program that calls `get_glv_token_value` through CPI.

Outer declared accounts:

```text
15 account metas
```

The liquidity-provider program ID is not one of those metas, so:

```text
compiled base refs = 16
```

The remaining accounts forwarded to Store pricing are:

```text
N market states
N market-token mints
F feed accounts
```

Formula:

```text
meta refs      = 15 + 2N + F
compiled refs  = 16 + 2N + F
unique         = 16 + 2N + F - D
```

The Store program is already present as `gt_program`, and the CPI's fixed pricing accounts are already present in the outer account set. CPI does not create a separate transaction account-key namespace.

This means `stake_glv` has the same `2N` scalability problem as direct GLV pricing, with a larger fixed base:

```text
direct pricing base = 8
stake_glv base      = 16
```

Ignoring feed growth and aliases:

```text
16 + 2N + F <= 64
```

so GLV staking can become the first operation to hit the account limit.

## Pull-oracle wrapper formulas

The pull-oracle wrapper builds three logical groups:

1. price-update post instructions;
2. the GLV consumer transaction;
3. price-update close instructions.

The bundle builder may pack post/consumer/close instructions into transactions according to size and atomic-group rules. Therefore `O_ref` and `O_out` are provider- and packing-dependent.

They must be measured from the final compiled transaction, not inferred only from `F`.

For the Pyth reproduction:

```text
F = 2 => O_ref = 14
F = 3 => O_ref = 15
O_out = 2
```

These values are evidence for that test shape, not universal constants for every oracle provider.

## Validation against the 12-market reproduction

### Baseline deposit plus close

Parameters:

```text
N = 12
F = 2
D_L = 0
D_S = 0
C = 0
L_out = 0
V = 0
B_out = 1
O_out = 2
X_close = 5
```

Unique formula:

```text
17 + 2(12 - 1) + 2 + 1 + 2 + 5
= 17 + 22 + 2 + 1 + 2 + 5
= 49
```

Measured: `49`.

Raw-reference formula:

```text
execute:       25 + 24 + 2 = 51
close:         25
compute:       2
oracle wrapper:14
total:         92
```

Measured: `92`.

### Heavy deposit, execution only

Additional parameters:

```text
L = 8
L_out = 0       # all route markets already occur in the GLV portfolio
V = 16
```

Unique:

```text
17 + 22 + 2 + 0 + 16 + 1 + 2
= 60
```

Measured: `60`.

Raw:

```text
(25 + 24 + 2 + 8 + 16) + 2 + 14
= 75 + 2 + 14
= 91
```

Measured: `91`.

### Heavy deposit plus close

```text
60 + X_close(5) = 65
```

Measured: `65`, rejected with `TooManyAccountLocks`.

### Baseline withdrawal plus close

Parameters:

```text
N = 12
F = 2
C = 0
L_out = 0
V = 0
B_out = 1
O_out = 2
X_close = 7
```

Unique:

```text
24 + 22 + 2 + 1 + 2 + 7
= 58
```

Measured: `58`.

Raw:

```text
execute:        26 + 24 + 2 = 52
close:          25
compute:        2
oracle wrapper: 14
total:          93
```

Measured: `93`.

### Heavy withdrawal, execution only

Parameters:

```text
N = 12
F = 3
L = 4
L_out = 1      # one route market was outside the GLV portfolio
V = 8
B_out = 1
O_out = 2
```

Unique:

```text
24 + 22 + 3 + 1 + 8 + 1 + 2
= 61
```

Measured: `61`.

Raw:

```text
(26 + 24 + 3 + 4 + 8) + 2 + 15
= 65 + 2 + 15
= 82
```

Measured: `82`.

### Heavy withdrawal plus close

```text
61 + X_close(7) = 68
```

Measured: `68`, rejected with `TooManyAccountLocks`.

## Formula to use in production code

Symbolic formulas are useful for design reviews, but production admission logic should not estimate aliases manually.

The authoritative algorithm is:

1. Build the exact final transaction, including:
   - ATA preparation;
   - oracle post/close instructions;
   - compute-budget instructions;
   - optional merged close;
   - all ALTs.
2. Compile the v0 message.
3. Resolve the ALT addresses.
4. Count:

```text
static_account_keys
+ writable_lookup_indices
+ readonly_lookup_indices
```

5. Require:

```text
resolved_unique_accounts <= 64
serialized_transaction_size <= 1,232
```

The formulas in this document explain growth and identify the source category. The compiled-message count is the final authority.
