# Escrow Contract

## Purpose

`EscrowContract` implements a simple two-party token escrow. A `buyer` deposits an
`amount` of a whitelisted token into the contract when creating an escrow for a given
`seller`, along with an `expiration` ledger timestamp. Before expiration, the buyer can
release the funds to the seller; after expiration, the buyer can instead reclaim the
funds via a refund. The contract also tracks the cumulative token volume that has
passed through it.

## Entry points

| Function | Parameters | Who can call | Returns |
|---|---|---|---|
| `whitelist_token` | `token: Address` | anyone (no auth check — simplified for composability; gate this at an admin layer in production) | `()` |
| `create_escrow` | `buyer: Address, seller: Address, token: Address, amount: i128, expiration: u64` | `buyer` (`require_auth`) | `Result<u64, EscrowError>` — new escrow id |
| `release_funds` | `escrow_id: u64` | `buyer` of that escrow (`require_auth`) | `Result<(), EscrowError>` |
| `refund_buyer` | `escrow_id: u64` | `buyer` of that escrow (`require_auth`) | `Result<(), EscrowError>` |
| `get_escrow` | `escrow_id: u64` | anyone | `Result<Escrow, EscrowError>` |
| `get_total_volume` | — | anyone | `i128` |
| `version` | — | anyone | `String` (currently `"1.0.0"`) |

## Storage layout

All keys live in **instance** storage (`env.storage().instance()`); no `extend_ttl` calls
are made anywhere in the crate, so TTL management is left to the default instance-storage
behavior.

| `DataKey` variant | Stored type | TTL |
|---|---|---|
| `Escrow(u64)` | `Escrow { buyer, seller, token, amount: i128, status: EscrowStatus, expiration: u64 }` | none (instance default) |
| `EscrowCount` | `u64` — monotonically incrementing id counter | none (instance default) |
| `TotalVolume` | `i128` — cumulative deposited amount | none (instance default) |
| `WhitelistedToken(Address)` | `bool` | none (instance default) |

`EscrowStatus` variants: `Pending`, `Completed`, `Cancelled`, `Disputed` (`Disputed` is
defined but never assigned by current logic).

## Events

| Event | Topics | Data |
|---|---|---|
| Escrow created | `symbol_short!("ESC_CRTD")` | `(escrow_id: u64, buyer: Address, seller: Address, amount: i128)` |
| Escrow released | `symbol_short!("ESC_RLSD")` | `(escrow_id: u64, seller: Address, amount: i128)` |
| Escrow refunded | `symbol_short!("ESC_RFND")` | `(escrow_id: u64, buyer: Address, amount: i128)` |

## Error codes

| Variant | Code | When returned |
|---|---|---|
| `NotFound` | 1 | `escrow_id` has no matching record |
| `NotPending` | 2 | Action attempted on an escrow that isn't `Pending` |
| `Expired` | 3 | `release_funds` called after `expiration` |
| `Unauthorized` | 4 | Reserved — not currently returned anywhere; auth failures instead trap via `require_auth()` |
| `TokenNotAllowed` | 5 | `create_escrow` called with a non-whitelisted token |
| `NotExpired` | 6 | `refund_buyer` called before `expiration` |
| `AlreadyReleased` | 7 | `release_funds` called twice on the same escrow |

## Testing

```
cargo test -p escrow
```

The package's real tests are the inline `#[cfg(test)] mod test` block in `src/lib.rs`
(total-volume, release, and refund flows). The `test_snapshots/` directory holds
auto-generated ledger fixtures for those tests — not hand-maintained.

## Example CLI invocations

```bash
# Deploy (see contracts/docs/setup.md for local network setup)
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/escrow.wasm \
  --source alice \
  --network local

export CONTRACT_ID=<id from deploy output>

# Whitelist a token
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  whitelist_token --token $TOKEN_ID

# Create an escrow
soroban contract invoke --id $CONTRACT_ID --source buyer --network local -- \
  create_escrow --buyer $BUYER --seller $SELLER --token $TOKEN_ID \
  --amount 1000 --expiration 1735689600

# Release funds to the seller
soroban contract invoke --id $CONTRACT_ID --source buyer --network local -- \
  release_funds --escrow_id 1

# Refund the buyer after expiration
soroban contract invoke --id $CONTRACT_ID --source buyer --network local -- \
  refund_buyer --escrow_id 1

# Read state
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  get_escrow --escrow_id 1
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  get_total_volume
```
