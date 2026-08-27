# ChainVerse Onchain — Contracts Overview

This document describes all smart contracts in the ChainVerse monorepo and how they interact.

## Contracts

| Contract | Path | Role |
|---|---|---|
| `escrow` | `contracts/escrow` | Holds buyer funds until delivery is confirmed or expiry |
| `escrow-vault` | `contracts/escrow-vault` | Multi-sig vault requiring threshold approvals before release |
| `certificates` | `contracts/certificates` | Mints and revokes on-chain course completion certificates |
| `chv_token` | `contracts/chv_token` | CHV ERC-20-style token with cumulative minting, circulating supply, burn, transfer, and bounded-TTL allowances |

### CHV Supply Counters

`total_minted` is the cumulative amount ever minted and does not decrease on burn. `circulating_supply` is the current supply: mint increases both counters, while burn decreases only `circulating_supply`. Existing deployments must initialize the new counter from an authoritative balance snapshot before upgrading; fresh deployments initialize both counters from the initial treasury supply.

### CHV Allowances

Allowance records use persistent storage with a bounded TTL of 100,000 to 200,000 ledgers. Creating, reading, or decrementing a nonzero allowance refreshes its TTL within that bound. An expired allowance is treated as absent: reads return `0`, and `transfer_from` returns `InsufficientAllowance`.

This is an internal storage-policy change; it does not alter the existing allowance ABI or events.

### CHV Storage Versioning

CHV storage is currently schema version `1`; unversioned legacy storage is source version `0`. The admin-only `migrate` entry point accepts an authoritative circulating-supply snapshot, validates the source version, and is idempotent for already-migrated storage. After installing the new WASM on an unversioned deployment, run `migrate` with source version `0`. Unsupported source versions and invalid snapshots are rejected before an upgrade is applied.
| `course_registry` | `contracts/course_registry` | Stores and manages course metadata |
| `payout-automation` | `contracts/payout-automation` | Batches token payouts to multiple recipients |
| `reward` | `contracts/reward` | Issues one-time rewards to users via signed backend proofs |
| `staking` | `contracts/staking` | Tiered token staking with lock periods and emergency unstake |
| `token` | `contracts/token` | Generic token with royalty support |
| `chainverse-core` | `contracts/chainverse-core` | Integration layer tying contracts together |

## Key Flows

### Course Purchase
1. Student calls `escrow::create_escrow` — funds held in contract
2. On completion, `escrow::release_escrow` sends funds to instructor
3. `certificates::mint` issues a certificate to the student

### Reward Claim
1. Backend signs a proof for an eligible user
2. User calls `reward::claim` with the proof
3. Contract verifies signature and transfers reward from treasury

### Staking
1. User calls `staking::stake_tokens` with a tier and amount
2. After lock period, user calls `staking::unstake`
3. Emergency unstake before lock period applies a penalty

### Token Royalties
1. The initializer becomes the token admin.
2. The admin calls `token::set_royalty(admin, recipient, bps)` to configure a royalty from 0 to 10,000 basis points.
3. `transfer` and `transfer_from` credit the recipient with the royalty and the destination with the net amount.

The royalty configuration is stored on the token contract and is included in the public ABI as `set_royalty` and `royalty`. Existing deployments do not have an admin or royalty configuration; deploy a new token instance or migrate balances before enabling this behavior.

### Token Administration
The initializer is the initial admin. Admin changes use `propose_admin(current_admin, new_admin)` followed by `accept_admin(new_admin)`, which must be authorized by the proposed address. The admin can call `pause` and `unpause`; while paused, `transfer` and `transfer_from` reject balance mutations. The admin-only `upgrade(admin, new_wasm_hash)` entrypoint updates the current contract WASM after authorization.
