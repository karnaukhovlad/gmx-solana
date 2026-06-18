# Iteration 005 — final review and decision record

Date: 2026-06-18

## Root cause disposition

Confirmed.

The limit is Solana's 64 resolved-account transaction lock ceiling, reached by GLV's full-portfolio synchronous valuation. It is not:

- the GLV's 96-entry storage capacity;
- transaction packet size;
- compute budget;
- an Address Lookup Table capacity;
- an Anchor remaining-account count;
- a hard-coded 12-market check.

The number 12 emerges from the remaining account budget after fixed, feed, route, virtual-inventory, and close accounts are included. The same 12-market fixture produced both successful and rejected transactions.

## Decision

Recommend the bounded path:

1. exact compiled-message account diagnostics;
2. adaptive split-close;
3. active-balance-only portfolio valuation;
4. conservative operation envelopes;
5. multiple GLVs for horizontal scale.

Do not use cached/batched NAV as the immediate production fix.

## Why

The bounded path preserves the existing security property: every nonzero GLV holding is valued atomically from live market state, mint supply, and validated prices.

Cached/batched NAV can remove the linear account dependency, but no reviewed design preserved atomic coherence without at least one of:

- loading all market versions again;
- globally serializing market mutations;
- freezing markets;
- trusting an off-chain NAV signer;
- accepting mixed-slot/stale values.

Those are protocol-level economic decisions and exceed the scope of a safe account-layout fix.

## Most easily overlooked implementation issue

The active-set optimization must be validated on-chain from recorded balances. If the SDK is allowed to choose the set, omitting one positive-balance market directly corrupts GLV pricing.

The next most dangerous issue is split-close orchestration. A successful execute followed by a failed close is not a failed action: tokens may be in escrow and the action marked completed. Retrying execute could be incorrect; retrying close is the recovery path. The SDK/API must expose this state explicitly.

## Verification completed

- Solana/Agave 2.1.21 source inspected.
- Mainnet-beta, devnet, and testnet feature status queried on 2026-06-18.
- Official transaction-limit documentation checked.
- Store program and SDK account assembly traced.
- Liquidity-provider GLV pricing CPI identified as a downstream caller.
- End-to-end 12-market reproduction passed with expected 65/68-account rejection and 60/61-account split-close success.
- Rust formatting check passed.
