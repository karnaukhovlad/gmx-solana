# GLV market-limit audit risk register

## Highest-risk area: valuation coherence

The current implementation reads every contributing market, every corresponding market-token mint supply, and every required price in one transaction. A batched or cached fix can silently mint or burn GLV against a portfolio assembled from different slots or price rounds.

This is not a normal stale-data issue. An attacker can deliberately mutate a market between valuation batches, choose the favorable ordering of updates, or wait for an oracle move. Any design that changes atomic valuation needs a formal economic specification, not only freshness checks.

## Risk register

| Risk | Failure mode | Required control |
|---|---|---|
| Incomplete active set | Caller omits a nonzero-balance market and understates/overstates NAV | On-chain derivation/validation of the exact ordered active set from GLV state; never trust an SDK-supplied count |
| Zero-to-nonzero transition | Deposit target is omitted because its pre-execution balance is zero | Always include and validate the current target market separately or as part of the required set |
| Nonzero-to-zero transition | Withdrawal changes the active set during execution | Define whether validation uses pre-state or post-state and test full withdrawal/rounding dust |
| Duplicate/reordered accounts | Market and mint arrays no longer align | Canonical ordering, exact length checks, duplicate rejection, and key-by-key validation |
| Current-market aliasing | Current writable market/mint also appear in portfolio accounts | Verify Solana message deduplication and Anchor mutability behavior; test duplicate privilege promotion |
| Oracle token-set drift | SDK collects feeds for a different active set than the program validates | Program derives required tokens from validated market accounts; stale SDK hints must fail safely |
| Maximize/minimize regression | Deposits or withdrawals use the wrong side of prices/PnL caps | Golden-vector tests for deposit-maximize and withdrawal-minimize behavior |
| Revertible-state mismatch | Deposit values the current market after its internal market deposit while using the pre-addition GLV balance | Preserve the existing special handling of the current market; compare event values before and after refactor |
| Split-close stranded state | Execute succeeds but close fails, leaving completed action accounts and output tokens in escrow | Permissioned recovery path, idempotent close retries, monitoring, and explicit SDK result that distinguishes executed from closed |
| Retry double execution | Client retries after an ambiguous RPC result | Action-state checks and idempotent orchestration; never infer failure solely from a transport timeout |
| Wrong account metric | Guardrail counts instruction references or static keys but ignores ALT-loaded keys | Count resolved unique keys in the compiled versioned message |
| Cluster-limit assumption | Client assumes 128 because the SDK constant is 128 | Default to 64 while the feature is inactive; make the limit explicit and observable |
| Packet-size confusion | An account fix creates a transaction larger than 1,232 bytes | Enforce both independent limits in tests and builder diagnostics |
| CPI caller breakage | Liquidity-provider `stake_glv` forwards the old full-portfolio account layout | Version/update every CPI caller and generated IDL/client together |
| Old/new instruction ambiguity | Deployed clients construct one account layout against another program version | Use a new instruction/versioned interface or a tightly controlled coordinated upgrade |
| External mint-supply changes | A proposal mirrors market-token supply in program state, but holders can burn SPL tokens directly | Do not cache supply unless external changes are prevented or the token/accounting model is explicitly migrated |
| Global invalidation contention | A global version counter is made writable by every market mutation | Benchmark and model account contention; avoid this design unless throughput loss is accepted |

## Tests required before production

1. Boundary tests at exactly 64 and 65 resolved accounts, using v0 messages and ALTs.
2. Deposit and withdrawal with:
   - zero active balances;
   - target market initially zero;
   - target market becoming exactly zero;
   - one-unit rounding dust;
   - maximum supported active count;
   - long and short swap paths;
   - virtual inventories;
   - each oracle provider;
   - merged and split close.
3. Deliberately stale SDK hints after a balance/config update.
4. Reordered, duplicated, missing, and extra portfolio accounts.
5. Ambiguous send result followed by retry and recovery.
6. CPI coverage for liquidity-provider GLV staking and any other downstream program.
7. Event-value equivalence against the current implementation for identical bank state.

## Audit recommendation

Treat active-only valuation and adaptive close splitting as bounded changes that can be compared directly with current semantics. Treat cached/batched NAV as a new protocol with a separate threat model and audit scope.
