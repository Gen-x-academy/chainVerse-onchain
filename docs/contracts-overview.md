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
