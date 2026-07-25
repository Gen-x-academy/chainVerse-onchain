# ChainVerse Onchain — Contracts Overview

This document describes all smart contracts in the ChainVerse monorepo and how they interact.

## Contracts

| Contract | Path | Role |
|---|---|---|
| `escrow` | `contracts/escrow` | Holds buyer funds until delivery is confirmed or expiry |
| `escrow-vault` | `contracts/escrow-vault` | Multi-sig vault requiring threshold approvals before release |
| `certificates` | `contracts/certificates` | Mints and revokes on-chain course completion certificates |
| `chv_token` | `contracts/chv_token` | CHV ERC-20-style token with mint, burn, and transfer |
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
