# Iteration 006 — why Solana's account-lock limit is 64, not 32

Date: 2026-06-18

## Short answer

The number `64` is a runtime resource-protection policy chosen when Solana introduced versioned transactions and Address Lookup Tables. It is not derived from the 1,232-byte transaction packet size.

Before Address Lookup Tables, transaction size usually limited a legacy transaction to roughly 30–35 account addresses because every public key occupied 32 bytes in the serialized message. That was a serialization consequence, not a protocol rule saying “at most 32 accounts.”

Versioned transactions removed most of that serialization pressure by allowing a transaction to encode an ALT-resolved account with a one-byte index. This made as many as 256 account indices representable. Solana then needed a separate runtime limit to prevent one transaction from loading and locking an excessive number of accounts. The initial deliberately conservative limit selected by the runtime developers was 64.

Therefore:

- `32 bytes` is the size of a public key;
- roughly `30–35 accounts` is what commonly fit in a legacy 1,232-byte transaction;
- `64 accounts` is the current runtime account-lock policy;
- `256` is the address-index representation ceiling of the v0 design;
- `128` is an implemented but currently inactive raised account-lock limit.

These numbers come from different layers and should not be treated as alternative values for one formula.

## 1. Where the apparent “32-account limit” came from

A legacy transaction includes every account address inline. Its total serialized size is limited to 1,232 bytes.

The relevant size components are:

```text
transaction
├── signature-vector length
├── signatures: 64 bytes each
└── message
    ├── header: 3 bytes
    ├── account-vector length
    ├── account addresses: 32 bytes each
    ├── recent blockhash: 32 bytes
    └── compiled instructions
        ├── program index
        ├── account indices
        └── instruction data
```

Ignoring compact-vector length changes, a one-signer transaction consumes at least:

```text
1 byte    signature count
64 bytes  fee-payer signature
3 bytes   message header
1 byte    account count
32 bytes  recent blockhash
1 byte    instruction count
--------------------------------
102 bytes before account keys and instruction contents
```

Each inline account then costs 32 bytes, and each instruction also needs account indices, a program index, lengths, and instruction data.

The absolute account count is therefore transaction-shape dependent:

```text
serialized_size =
    fixed transaction fields
  + 64 × signer_count
  + 32 × inline_account_count
  + compiled_instruction_bytes
```

For example, `32` inline account keys alone consume:

```text
32 × 32 = 1,024 bytes
```

That leaves:

```text
1,232 - 1,024 = 208 bytes
```

for signatures, the blockhash, header, account indices, and instruction data. One signature and the other mandatory fields consume much of that remainder.

This is why developers historically described the legacy limit as approximately 32 accounts, or approximately 35 in particularly compact shapes. Neither number was an explicit account-lock constant. A transaction with more signers or more instruction data fitted fewer accounts.

## 2. Why Address Lookup Tables changed the problem

V0 transactions can place non-signer addresses in an Address Lookup Table.

On the wire:

- an inline public key costs 32 bytes;
- an ALT-resolved address is selected by a one-byte index;
- there is additional fixed overhead for identifying the ALT account and its index vectors.

For a sufficiently populated table, this saves approximately 31 bytes per looked-up account.

The important distinction is that compression ends before execution:

1. The client sends compact ALT indices.
2. The validator loads the ALT account.
3. The validator resolves every selected index to a complete 32-byte public key.
4. The resolved keys are appended to the static account keys.
5. The runtime validates, loads, and locks the complete resolved account set.

An ALT therefore allows many more addresses to fit into 1,232 network bytes, but it does not make those accounts free to load or lock.

## 3. Why a separate runtime cap became necessary

V0 instruction account indices are one byte. An ALT stores up to 256 addresses. Without a separate runtime cap, a compact transaction could request a very large account set even though its wire representation remained small.

Solana issue `#21748`, opened before v0 activation, describes the concern:

- versioned transactions could load up to 256 accounts;
- a transaction could lock many accounts;
- this could create denial-of-service and lock-contention problems for on-chain programs;
- the proposed response was a reasonable initial cap until the cost model and contention controls became more nuanced.

The corresponding implementation, PR `#22201`, introduced a maximum of 64 locked accounts per transaction.

The feature-gate record later stated that the 256-account representational capability should remain artificially restricted until appropriate cost measures were available.

The purpose of the cap is consequently to bound several validator costs:

- resolving and validating account keys;
- acquiring read/write account locks;
- loading account state;
- tracking account access during execution;
- committing writable state;
- reducing the contention surface where one transaction touches many accounts;
- limiting the amount of work represented by a compact transaction.

The account-lock limit is checked before the program runs. A transaction exceeding it is rejected with `TransactionError::TooManyAccountLocks`.

## 4. Why exactly 64

The available primary sources describe `64` as a “reasonable cap” and a deliberately artificial restriction. They do not publish a formula proving that 64 is an optimal value derived from hardware, MTU, compute units, or a security constant.

The defensible interpretation is:

1. Legacy transactions already operated in a practical range around 30–35 accounts due to packet size.
2. V0/ALTs were intended to support materially larger atomic transactions.
3. Allowing the full representable 256 immediately would create unbounded new loading and lock-contention exposure relative to legacy traffic.
4. `64` approximately doubled the old practical account range while remaining a conservative, easily enforceable bound.
5. The cap could later be raised behind a feature gate after related runtime and cost-model work.

The statement that 64 “approximately doubled the old range” is an inference from the transaction formats and project history. The project record does not say that doubling was the formal selection algorithm.

It is also reasonable engineering practice to use power-of-two capacity limits, but the reviewed primary sources do not establish “because it is a power of two” as the reason for choosing 64. That should not be presented as an official rationale.

## 5. Why not 32

Setting the runtime cap to 32 would have confused the previous serialization bottleneck with runtime resource control.

It would also have undercut a principal purpose of v0 transactions:

- legacy transactions already struggled around that range;
- ALTs were introduced specifically to allow more account references in one atomic operation;
- a hard 32-account runtime cap would have provided little additional composability despite the new format.

Additionally, 32 was never a stable legacy maximum. Some compact transactions could fit more; transactions with several signatures or significant data could fit less.

The runtime needed a deterministic count independent of serialized shape. The chosen 64-account ceiling gave v0 transactions useful additional capacity while bounding their validator impact.

## 6. Why not allow all 256

The one-byte index format can address 256 positions, but format capacity is not the same as safe runtime capacity.

Allowing 256 loaded accounts would mean that a small wire transaction could cause validators to:

- resolve hundreds of addresses;
- acquire hundreds of locks;
- load hundreds of accounts;
- expose hundreds of accounts to program execution;
- potentially block concurrent transactions touching any writable account in that set.

The historical issue explicitly identifies this as a denial-of-service and lock-contention concern. The initial cap intentionally prevented the new compact encoding from multiplying runtime work without corresponding cost controls.

## 7. Why 128 exists but 64 remains active

In 2022, Solana added feature `increase_tx_account_lock_limit`, address:

```text
9LZdXeKGeBV6hRLdxS1rHbHoEUsKqesCC2ZAPTPKJAbK
```

That feature changes the effective maximum from 64 to 128. Its feature record says 64 was too restrictive for use cases such as Neon EVM.

The SDK constant is now 128 and records that 128 was the minimum needed by the Neon EVM implementation. However, the bank only applies that raised ceiling when the feature is active.

As of 2026-06-18:

- the official Solana transaction documentation lists 64 as the enforced maximum;
- it describes 128 as conditional on the currently inactive feature;
- direct feature-status queries reported the feature inactive on mainnet-beta, devnet, and testnet.

This shows that even 128 is a policy choice tied to an identified use case and rollout requirements, not a fundamental limit implied by the message format.

## 8. The limits are independent

A transaction must satisfy both constraints:

```text
serialized transaction size <= 1,232 bytes
resolved unique account count <= 64
```

Possible outcomes:

| Transaction shape | Packet size | Resolved accounts | Result |
|---|---:|---:|---|
| Legacy, many inline keys | Above 1,232 | Below 64 | Rejected for size |
| V0 with effective ALT compression | Below 1,232 | Above 64 | `TooManyAccountLocks` |
| V0, compact and bounded | Below 1,232 | At most 64 | Can proceed to normal validation/execution |

The GLV reproduction is the second case: failing messages were only 784 and 1,029 bytes, but resolved to 65 and 68 unique accounts.

## 9. Implication for GLV

The GLV issue is not caused by choosing 64 instead of 32. A 32-account cap would make GLV fail substantially earlier.

Nor should GLV rely on a future 128-account activation. That would increase headroom but preserve the same linear account growth:

```text
unique_accounts =
    fixed_accounts
  + 2 × GLV_market_count
  + feeds
  + route_accounts
  + virtual_inventories
  + close_accounts
  - duplicates
```

At 128, the threshold moves; the architecture does not become unbounded. A 96-active-market GLV still cannot fit because its portfolio term alone approaches 192 market and mint accounts.

The production fix should therefore reduce required accounts or partition the operation, rather than treating the cluster limit as an application capacity guarantee.

## Sources

Primary sources:

- Solana transaction limits: <https://solana.com/docs/core/transactions>
- Transaction serialization and 1,232-byte calculation: <https://solana.com/docs/core/transactions/transaction-structure>
- V0 and ALT resolution: <https://solana.com/docs/core/transactions/versioned-transactions>
- Original problem statement for unbounded v0 account loading: <https://github.com/solana-labs/solana/issues/21748>
- PR introducing the 64-account limit: <https://github.com/solana-labs/solana/pull/22201>
- Feature-gate record for the initial limit: <https://github.com/solana-labs/solana/issues/24046>
- Feature-gate record proposing 128: <https://github.com/solana-labs/solana/issues/27241>

Local runtime sources used by this repository:

- `solana-sdk-2.1.21/src/transaction/sanitized.rs`
- `solana-feature-set-2.1.21/src/lib.rs`

