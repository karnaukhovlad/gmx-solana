# Iteration 008 — deposit and withdrawal account counts for 1–15 GLV markets

Date: 2026-06-18

## Purpose

These tables project resolved unique transaction accounts for representative GLV deposit and withdrawal situations as the GLV market count `N` grows from 1 to 15.

The Solana limit is:

```text
unique accounts <= 64
```

Legend:

- `✓` — fits the 64-account limit;
- `LIMIT` — exactly 64 accounts and has no remaining headroom;
- `FAIL` — exceeds 64 and is rejected with `TooManyAccountLocks`;
- bold values — measured directly in the 12-market reproduction;
- other values — calculated from the measured transaction shape while holding feeds, paths, virtual inventories, oracle wrapping, and close-account overlap constant.

## Fixed scenario definitions

### Baseline deposit

- direct GM/market-token deposit;
- no initial long or short token leg;
- no swap path;
- two unique feed accounts;
- no virtual inventories;
- Pyth pull-oracle wrapper;
- one Compute Budget program account.

### Heavy deposit

- same direct GM/market-token deposit;
- eight route-market references;
- all eight route markets already belong to the GLV portfolio, so they add references but no additional unique market keys;
- sixteen distinct virtual-inventory accounts;
- two unique feed accounts;
- Pyth pull-oracle wrapper;
- one Compute Budget program account.

### Baseline withdrawal

- direct withdrawal to the selected market's pool tokens;
- no output swap path;
- two unique feed accounts;
- no virtual inventories;
- Pyth pull-oracle wrapper;
- one Compute Budget program account.

### Heavy withdrawal

- four route-market references;
- one route market is outside the GLV portfolio and therefore adds one unique account;
- eight distinct virtual-inventory accounts;
- three unique feed accounts;
- Pyth pull-oracle wrapper;
- one Compute Budget program account.

## Calibrated formulas

### Deposit

```text
baseline execution only = 2N + 20
baseline execute + close = 2N + 25
heavy execution only    = 2N + 36
heavy execute + close   = 2N + 41
```

The merged close adds five unique accounts to the reproduced deposit shapes.

### Withdrawal

```text
baseline execution only = 2N + 27
baseline execute + close = 2N + 34
heavy execution only    = 2N + 37
heavy execute + close   = 2N + 44
```

The merged close adds seven unique accounts to the reproduced withdrawal shapes.

## Deposit table

| GLV markets `N` | Baseline execution only `2N+20` | Baseline + close `2N+25` | Heavy execution only `2N+36` | Heavy + close `2N+41` |
|---:|---:|---:|---:|---:|
| 1 | 22 ✓ | 27 ✓ | 38 ✓ | 43 ✓ |
| 2 | 24 ✓ | 29 ✓ | 40 ✓ | 45 ✓ |
| 3 | 26 ✓ | 31 ✓ | 42 ✓ | 47 ✓ |
| 4 | 28 ✓ | 33 ✓ | 44 ✓ | 49 ✓ |
| 5 | 30 ✓ | 35 ✓ | 46 ✓ | 51 ✓ |
| 6 | 32 ✓ | 37 ✓ | 48 ✓ | 53 ✓ |
| 7 | 34 ✓ | 39 ✓ | 50 ✓ | 55 ✓ |
| 8 | 36 ✓ | 41 ✓ | 52 ✓ | 57 ✓ |
| 9 | 38 ✓ | 43 ✓ | 54 ✓ | 59 ✓ |
| 10 | 40 ✓ | 45 ✓ | 56 ✓ | 61 ✓ |
| 11 | 42 ✓ | 47 ✓ | 58 ✓ | 63 ✓ |
| 12 | 44 ✓ | **49 ✓** | **60 ✓** | **65 FAIL** |
| 13 | 46 ✓ | 51 ✓ | 62 ✓ | 67 FAIL |
| 14 | 48 ✓ | 53 ✓ | 64 LIMIT | 69 FAIL |
| 15 | 50 ✓ | 55 ✓ | 66 FAIL | 71 FAIL |

### Deposit boundary summary

| Situation | Maximum `N` that fits | Account count at maximum | First failing `N` |
|---|---:|---:|---:|
| Baseline execution only | 22 | 64 | 23 |
| Baseline + close | 19 | 63 | 20 |
| Heavy execution only | 14 | 64 | 15 |
| Heavy + close | 11 | 63 | 12 |

For completeness, the formula-level baseline boundaries beyond this table are:

```text
baseline execution only:
    2N + 20 <= 64
    N <= 22

baseline + close:
    2N + 25 <= 64
    N <= 19
```

These are not general supported-market limits. They apply only to the fixed baseline shape defined above.

## Withdrawal table

| GLV markets `N` | Baseline execution only `2N+27` | Baseline + close `2N+34` | Heavy execution only `2N+37` | Heavy + close `2N+44` |
|---:|---:|---:|---:|---:|
| 1 | 29 ✓ | 36 ✓ | 39 ✓ | 46 ✓ |
| 2 | 31 ✓ | 38 ✓ | 41 ✓ | 48 ✓ |
| 3 | 33 ✓ | 40 ✓ | 43 ✓ | 50 ✓ |
| 4 | 35 ✓ | 42 ✓ | 45 ✓ | 52 ✓ |
| 5 | 37 ✓ | 44 ✓ | 47 ✓ | 54 ✓ |
| 6 | 39 ✓ | 46 ✓ | 49 ✓ | 56 ✓ |
| 7 | 41 ✓ | 48 ✓ | 51 ✓ | 58 ✓ |
| 8 | 43 ✓ | 50 ✓ | 53 ✓ | 60 ✓ |
| 9 | 45 ✓ | 52 ✓ | 55 ✓ | 62 ✓ |
| 10 | 47 ✓ | 54 ✓ | 57 ✓ | 64 LIMIT |
| 11 | 49 ✓ | 56 ✓ | 59 ✓ | 66 FAIL |
| 12 | 51 ✓ | **58 ✓** | **61 ✓** | **68 FAIL** |
| 13 | 53 ✓ | 60 ✓ | 63 ✓ | 70 FAIL |
| 14 | 55 ✓ | 62 ✓ | 65 FAIL | 72 FAIL |
| 15 | 57 ✓ | 64 LIMIT | 67 FAIL | 74 FAIL |

### Withdrawal boundary summary

| Situation | Maximum `N` that fits | Account count at maximum | First failing `N` |
|---|---:|---:|---:|
| Baseline execution only | 18 | 63 | 19 |
| Baseline + close | 15 | 64 | 16 |
| Heavy execution only | 13 | 63 | 14 |
| Heavy + close | 10 | 64 | 11 |

For completeness:

```text
baseline withdrawal execution only:
    2N + 27 <= 64
    N <= 18
```

## Direct comparison at 12 markets

| Situation | Calculated | Measured | Result |
|---|---:|---:|---|
| Baseline deposit + close | `2×12 + 25 = 49` | 49 | Pass |
| Heavy deposit execution only | `2×12 + 36 = 60` | 60 | Pass |
| Heavy deposit + close | `2×12 + 41 = 65` | 65 | `TooManyAccountLocks` |
| Baseline withdrawal + close | `2×12 + 34 = 58` | 58 | Pass |
| Heavy withdrawal execution only | `2×12 + 37 = 61` | 61 | Pass |
| Heavy withdrawal + close | `2×12 + 44 = 68` | 68 | `TooManyAccountLocks` |

The exact match confirms that each additional GLV market adds two unique accounts for these fixed transaction shapes:

```text
one Market state + one market-token mint
```

## How different action details move the tables

The tables should be adjusted using these deltas:

| Transaction change | Unique-account delta |
|---|---:|
| Add one GLV portfolio market | `+2` |
| Add one distinct feed account | `+1` |
| Add one route market already in the GLV portfolio | `+0` unique, but `+1` raw reference |
| Add one route market outside the GLV portfolio | `+1` |
| Add one distinct virtual inventory | `+1` |
| Add an initial long deposit leg | normally `+3` |
| Add an initial short deposit leg | normally `+3` |
| Merge close into reproduced deposit shape | `+5` |
| Merge close into reproduced withdrawal shape | `+7` |
| Split close into another transaction | subtract the corresponding merged-close delta from execution |
| Add a distinct optional Chainlink program | `+1` |

Example: heavy deposit execution only at 12 markets was 60. Adding an initial long-token leg normally gives:

```text
60 + 3 = 63
```

Adding both initial token legs gives:

```text
60 + 3 + 3 = 66
```

which would fail even without merged close.

## Important limitation

These tables are calibrated scenario tables, not universal constants.

Counts change if:

- the oracle provider changes;
- the number of unique feeds changes;
- route markets are inside or outside the GLV portfolio;
- virtual inventories differ;
- owner and receiver aliases differ;
- long and short token mints are equal;
- optional accounts resolve differently;
- pull-oracle instructions are packed into different transactions;
- SDK or program account layouts change.

Production code must compile and count the exact final v0 message. These tables are for capacity planning and explaining which action shape crosses the limit first.
