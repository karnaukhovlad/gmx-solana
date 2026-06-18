# Iteration 009 — one account per GLV market

Date: 2026-06-18

## Decision

A true `1N` portfolio layout is not a local GLV account-parser optimization.

The current `2N` inputs contain two independent live values for every market:

1. the `Market` account contains pool, PnL, fee, open-interest, and configuration state;
2. the SPL market-token mint contains the current total market-token supply.

GLV pricing needs both:

```text
market-token value = GLV balance × market pool value / market-token supply
```

Solana programs cannot read an account that is not present in the transaction. An
address lookup table changes transaction serialization, but it does not combine the
`Market` and mint accounts or remove either account lock.

Therefore, preserving the current live-SPL-supply pricing semantics requires `2N`
accounts. Reaching `1N` requires changing where canonical supply is stored or
changing the market-token model.

## What commit `2728a776` did

Before commit `2728a776`, execution supplied three accounts per configured GLV
market:

```text
Market + market-token mint + GLV market-token vault
```

The commit added `balance: u64` to `GlvMarketConfig` and updated that value during:

- GLV deposits;
- GLV withdrawals;
- GLV shifts.

It retained a post-operation check that the real vault balance is greater than or
equal to the recorded GLV balance. This allowed portfolio valuation to read the GLV
balance from the already-loaded `Glv` account and removed all `N` vault accounts:

```text
3N -> 2N
```

The same approach cannot be repeated mechanically for supply. A GLV vault balance
changes only through Store-controlled GLV instructions. SPL mint supply can also
decrease when a holder directly invokes the SPL Token `Burn` instruction.

## Why caching mint supply only in `GlvMarketConfig` is invalid

Market-token mints use the standard SPL Token program. The Store PDA is the mint
authority, which prevents unauthorized minting, but a token-account owner or
delegate can burn tokens from its own account. SPL Token updates the mint supply
without invoking the Store program.

Consequently:

```text
cached supply >= live SPL mint supply
```

after an external burn.

Using the cached value only in GLV would make GM pricing inconsistent:

- ordinary market deposit/withdrawal paths would use live SPL supply;
- GLV portfolio pricing would use cached supply.

For deposits, an overstated denominator understates existing GLV NAV and mints more
GLV shares than the current pricing rule permits. For withdrawals and direct pricing,
it understates value. Even if direct burning can be interpreted as abandoning value,
the two pricing systems must not use different supply definitions.

This is an economic/accounting change, not a harmless cache.

## Viable `1N` design: Store-tracked issued supply

The least invasive technically viable design is to make the `Market` account the
canonical source of an issued-supply value.

### State

Use eight bytes from `Market.reserved`:

```rust
market_token_issued_supply: u64
```

The value means:

```text
all market tokens minted by Store - all market tokens burned by Store
```

Direct holder burns intentionally do not reduce issued supply. They become abandoned
shares and do not increase the value of remaining shares.

### Required global invariant

The following invariant must hold after every successful Store operation:

```text
market.market_token_issued_supply >= spl_mint.supply
```

Equality holds unless tokens have been burned outside Store.

All protocol pricing—not only GLV pricing—must use issued supply. Otherwise ordinary
GM operations and GLV operations disagree about the price of the same token.

### Required code changes

1. Add and version the issued-supply field in `Market`.
2. Initialize new markets with issued supply `0`.
3. Add a migration instruction for existing markets:
   - load `Market` and its mint;
   - verify the mint address and Store mint authority;
   - copy the current SPL supply;
   - mark the market supply version initialized.
4. Update `RevertibleLiquidityMarket`:
   - `total_supply()` reads Store-issued supply plus staged mint minus staged burn;
   - commit updates issued supply atomically with Store mint/burn CPIs.
5. Update first-deposit checks to use issued supply.
6. Update every on-chain market-token pricing path to use issued supply.
7. Update SDK simulation/model conversion to use the same definition.
8. Reject GLV execution/pricing for an unmigrated market.
9. Change GLV remaining accounts from:

   ```text
   N Market accounts + N mint accounts
   ```

   to:

   ```text
   N Market accounts
   ```

10. Validate each Market account against the canonical ordered token keys stored in
    `Glv`:
    - derive the expected Market PDA from Store and market-token key;
    - require the supplied account key to match;
    - load and validate `MarketMeta.market_token_mint`;
    - reject missing, extra, duplicate, or reordered portfolio accounts.
11. Update deposit, withdrawal, pricing, SDK builders, IDL, and the
    liquidity-provider pricing CPI caller together.

### Compatibility consequence

This design changes the meaning of market-token supply. A user who directly burns GM
tokens abandons the claim permanently. The burn no longer increases the protocol
price of remaining GM tokens.

This behavior must be explicitly accepted before implementation. It affects:

- indexers and analytics reading SPL mint supply;
- SDK simulations;
- integrations that calculate GM price independently;
- migration and rollback;
- audit assumptions.

## Alternative exact-supply designs

### Controlled/frozen market tokens

Migrate to market tokens whose holder accounts remain frozen and can only move or
burn through Store-controlled thaw/operation/refreeze instructions. Store can then
maintain exact supply in `Market`.

This preserves exact tracked supply but removes normal permissionless SPL-token
transferability and requires new mints plus a holder migration.

### Custom composite token program

Use a custom token program whose mint account also stores the market valuation state,
or whose supply cannot change without updating Store state.

This can provide one physical account with exact state, but it is a new token
architecture and breaks standard SPL assumptions. It is substantially larger than a
GLV optimization.

### Snapshot/cache account

Precompute one valuation snapshot per market and pass only snapshots to GLV.

This is `1N` at execution time, but exact atomic pricing is lost unless every market
mutation and every supply mutation invalidates or updates the snapshot. Direct SPL
burns remain unobservable. Freezing, trusted reports, or a multi-transaction locking
protocol would be required.

## Account-count impact

For deposit and withdrawal execution, the current market and current mint already
appear in the fixed account list. Portfolio arrays repeat them and Solana deduplicates
the addresses.

Changing the portfolio arrays from `Market + mint` to `Market` therefore saves:

```text
N - 1 unique accounts
```

for an execute instruction targeting one of the GLV markets.

For standalone `get_glv_token_value`, no current market/mint pair is fixed, so the
saving is:

```text
N unique accounts
```

Applying the `N - 1` saving to the measured 12-market fixture:

| Transaction shape | Current | Projected `1N` |
|---|---:|---:|
| Baseline deposit + close | 49 | 38 |
| Heavy deposit + close | 65 | 54 |
| Heavy deposit, execution only | 60 | 49 |
| Baseline withdrawal + close | 58 | 47 |
| Heavy withdrawal + close | 68 | 57 |
| Heavy withdrawal, execution only | 61 | 50 |

These are projections from account-set subtraction, not end-to-end measurements.

Oracle feed accounts remain separate. If every market has a distinct index token,
portfolio execution can still add approximately one feed account per market. The
transaction-level scaling can therefore remain close to `2N` even after the GLV
portfolio array itself becomes `1N`.

## Recommendation

Do not add a GLV-only cached supply.

Choose explicitly between:

1. preserve current GM/SPL semantics and retain `2N`, while implementing active
   nonzero-balance filtering and adaptive close splitting;
2. approve Store-tracked issued supply as a protocol-wide accounting migration, then
   implement the `1N` Market-only GLV layout;
3. redesign market tokens so Store can maintain exact supply without observing a
   separate SPL mint account.

Option 2 is the smallest implementation that genuinely reaches `1N`, but it requires
an economic specification and migration plan before code changes.
