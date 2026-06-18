# GLV market-limit overview

## Executive conclusion

The GLV does not have a 12-market program limit. Its state supports up to 96 markets.

The actual limit is Solana's currently enforced maximum of 64 resolved accounts per transaction. Deposit, withdrawal, GLV pricing, and CPI consumers price the complete GLV portfolio by passing one market state account and one market-token mint account for every managed market. They also need fixed instruction accounts, oracle feeds, swap/virtual-inventory accounts, and often a merged close instruction.

The runtime resolves static and ALT-loaded addresses, deduplicates them, and rejects the transaction with `TooManyAccountLocks` when the final unique count exceeds 64. This occurs before GMX executes. ALTs reduce serialized bytes but do not reduce loaded/locked account count.

## Why “approximately 12”

For a given action:

`unique accounts = fixed/action accounts + 2 × portfolio markets + unique feeds + route state + close state - duplicates`

The current market and mint are duplicated between fixed and portfolio accounts, so an equivalent form is:

`U = F + 2 × (N - 1) + P + V + C - D`

There is no universal market threshold because `P`, `V`, `C`, and `D` vary.

The reproduction measured the following on the same 12-market GLV:

- 49 accounts: baseline deposit plus close — passed;
- 58 accounts: baseline withdrawal plus close — passed;
- 65 accounts: heavy deposit plus close — rejected;
- 60 accounts: the same heavy deposit without close — passed;
- 68 accounts: heavy withdrawal plus close — rejected;
- 61 accounts: the same heavy withdrawal without close — passed.

All messages were below 1,232 bytes. The observed limit follows account count, not packet size, compute, GLV account capacity, or a hard-coded market count.

## Why Solana uses 64 rather than 32

The 64-account limit is a runtime policy introduced with versioned transactions, not a value calculated from transaction serialization.

The often-cited “32 accounts” was only an approximate legacy-transaction constraint. Public keys are 32 bytes, and all legacy account keys had to fit inline within a 1,232-byte transaction together with signatures, a blockhash, instruction indices, and instruction data. Depending on transaction shape, the practical range was roughly 30–35 accounts.

V0 transactions and Address Lookup Tables made many more account indices representable in the same packet. The runtime developers introduced a separate 64-account cap because allowing the full 256-address representation could cause excessive account loading, lock contention, and denial-of-service exposure.

The historical sources call 64 a reasonable, artificial initial cap. They do not define a mathematical formula proving 64 is optimal. It provided materially more capacity than legacy serialization while bounding the new runtime work enabled by ALTs.

See [Iteration 006](iterations/006-why-the-limit-is-64.md) for the expanded explanation.

## Per-instruction formulas

The complete account-budget reference is in [Iteration 007](iterations/007-account-count-formulas.md). It covers every GLV instruction, deposit/withdrawal input and route type, shift, pricing, `stake_glv` CPI, pull-oracle overhead, ATA preparation, compute-budget instructions, and merged close behavior.

Projected 1–15 market counts for representative deposit and withdrawal situations are in [Iteration 008](iterations/008-market-count-scenario-tables.md).

## Scope

The portfolio-wide account pattern is used by:

- execute GLV deposit;
- execute GLV withdrawal;
- get GLV token value;
- liquidity-provider GLV staking through pricing CPI.

GLV shift only prices its from/to markets and does not have the same `2N` portfolio requirement. Initialization has a separate `3N` account shape, while markets can subsequently be inserted one at a time.

## Confirmation

- Local Agave/Solana 2.1.21 source checks resolved account-key length and returns `TooManyAccountLocks`.
- Feature `increase_tx_account_lock_limit` would raise 64 to 128, but was inactive on mainnet-beta, devnet, and testnet on 2026-06-18.
- Official Solana documentation currently lists 64 as the enforced maximum.
- The end-to-end test reproduced 65/68-account failures before GMX execution and successful 60/61-account variants.
