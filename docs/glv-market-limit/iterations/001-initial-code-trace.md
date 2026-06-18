# Iteration 001 — initial code trace

Date: 2026-06-18

## Question

Why do GLV operations begin failing when one GLV manages roughly 12 markets, and why is the threshold approximate rather than a protocol-level `12`?

## Initial findings

1. The on-chain GLV data structure is not capped at 12 markets. `Glv::MAX_ALLOWED_NUMBER_OF_MARKETS` is 96.
2. Deposit, withdrawal, and pricing instructions require the complete GLV portfolio in one instruction:
   - `N` market accounts;
   - `N` market-token mint accounts;
   - then oracle feeds and action-dependent swap/virtual-inventory accounts.
3. Therefore every additional GLV market adds at least two unique transaction accounts.
4. Execute-deposit and execute-withdrawal also have a large fixed account set. The SDK normally merges a close instruction into the same transaction, which may introduce additional unique receiver/token accounts.
5. Address Lookup Tables reduce serialized address bytes but do not reduce the number of loaded/locked accounts.
6. The existing reproduction test expects `TransactionError::TooManyAccountLocks`, verifies that no GMX program logs are emitted, and checks that the transaction remains below `PACKET_DATA_SIZE`. This points to transaction admission/account locking, not program compute, account data size, or packet serialization size.

## Current root-cause hypothesis

The practical limit is the Solana transaction account-lock limit for the active local validator/runtime configuration. A GLV operation has:

`unique accounts = fixed transaction accounts + 2 * GLV market count + action-dependent accounts`

The observed `~12` is the point where common production transaction shapes cross the runtime's unique-account lock ceiling. It is not an intrinsic GLV maximum and can move lower or higher with swap paths, oracle-provider accounts, virtual inventories, merged close instructions, duplicated accounts, and runtime feature configuration.

## Evidence already present in the repository

- Commit `5fef9157` adds a targeted 12-market fixture and captures unique loaded accounts from the compiled versioned message.
- The test distinguishes total instruction references from unique loaded accounts.
- It asserts `TooManyAccountLocks` and empty execution logs for failing shapes.
- It demonstrates the same 12-market GLV can succeed for a baseline shape and fail for an account-heavy shape.
- It demonstrates that removing the merged close instruction can bring the same operation back under the limit.

## Evidence still required

- Run the reproduction and preserve its exact account counts and error output.
- Identify the exact account-lock ceiling used by this Solana/Agave version and whether a feature can raise it.
- Produce a per-account breakdown for representative deposit, withdrawal, shift, and pricing transactions.
- Confirm whether the reported production failure always means `TooManyAccountLocks`, or whether packet-size/compute limits can become the first failure for some shapes.
- Evaluate whether splitting close is only a tactical mitigation or a complete fix.

## Early solution candidates

1. SDK-only mitigation: never merge close when account headroom is insufficient.
2. SDK guardrail: compile before signing, count loaded unique accounts, and fail/split deterministically.
3. Protocol redesign: replace full-portfolio synchronous valuation with cached/incremental GLV valuation state.
4. Protocol redesign variant: permissionless batched refresh of per-market valuation snapshots, followed by execution against a coherent committed snapshot.
5. Operational mitigation: cap markets per GLV based on worst-case transaction shape, not a constant inferred from current deployments.

No recommendation is final at this iteration.
