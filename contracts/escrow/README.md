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
| `whitelist_token` | `token: Address` | admin | `()` |
| `create_escrow` | `buyer: Address, seller: Address, token: Address, amount: i128, expiration: u64` | `buyer` (`require_auth`) | `Result<u64, EscrowError>` — new escrow id |
| `fund_escrow` | `escrow_id: u64` | `buyer` of that escrow | `Result<(), EscrowError>` |
| `release_escrow` | `escrow_id: u64` | `buyer` or admin | `Result<(), EscrowError>` |
| `partial_release` | `escrow_id: u64, amount: i128` | `buyer` or admin | `Result<(), EscrowError>` |
| `dispute_escrow` | `escrow_id: u64` | `buyer` | `Result<(), EscrowError>` |
| `refund_escrow` | `escrow_id: u64` | `buyer` | `Result<(), EscrowError>` |
| `get_escrow` | `escrow_id: u64` | anyone | `Option<Escrow>` |
| `get_by_buyer_index` | `buyer: Address` | anyone | `Vec<u64>` |
| `get_by_seller_index` | `seller: Address` | anyone | `Vec<u64>` |
| `set_protocol_fee_bps` | `admin: Address, bps: u32` | admin | `Result<(), EscrowError>` |
| `get_protocol_fee` | `token: Address` | anyone | `i128` |
| `withdraw_fees` | `caller: Address, token: Address, recipient: Address, amount: i128` | admin | `Result<(), EscrowError>` |
| `upgrade` | `new_wasm_hash: BytesN<32>` | admin | `Result<(), EscrowError>` |

## State graph

One canonical status per phase (#859). `Created` is the post-create, pre-deposit
phase; once the buyer deposits the escrow becomes `Funded`. Only a `Funded` escrow
may be released (fully or partially), disputed, or refunded. Disputing moves a
`Funded` escrow to `Disputed`; release and refund are blocked while disputed.
A full release or a partial release that empties the balance completes the escrow;
a refund after the deadline cancels it. `Completed` and `Cancelled` are terminal.

```
Created ──fund──▶ Funded ──release/partial_release (full)──▶ Completed
                   │  │
                   │  └───dispute────▶ Disputed ──(resolution)──▶ Completed
                   └─refund (after expiration)──▶ Cancelled
```

`EscrowStatus` canonical variants: `Created`, `Funded`, `Completed`, `Cancelled`,
`Disputed`.

## Storage layout

All escrow records and index lists live in **persistent** storage
(`env.storage().persistent()`) with their TTL extended. Configuration (admin,
whitelist, fee bps, paused, accumulated fees, volume) lives in instance storage.

| `DataKey` variant | Stored type | Bucket |
|---|---|---|
| `Admin` | `Address` | instance |
| `Escrow(u64)` | `Escrow { buyer, seller, token, amount: i128, status: EscrowStatus, expiration: u64 }` | persistent |
| `EscrowCount` | `u64` — monotonically incrementing id counter | instance |
| `TotalVolume` | `i128` — cumulative deposited amount | instance |
| `WhitelistedToken(Address)` | `bool` | instance |
| `ProtocolFees(Address)` | `i128` — accrued fees per token | instance |
| `TokenIndex(Address)` | `Vec<u64>` — escrow ids by token | persistent |
| `BuyerIndex(Address)` | `Vec<u64>` — escrow ids by buyer | persistent |
| `SellerIndex(Address)` | `Vec<u64>` — escrow ids by seller | persistent |
| `FeeHistory` | `Vec<FeeRecord>` | persistent |

Every escrow is indexed exactly once by its token, buyer, and seller at creation
(#858).

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
| `NotPending` | 2 | Retained for backward compatibility (legacy name for an invalid lifecycle state) |
| `InvalidEscrowState` | 15 | Action attempted on an escrow not in the required lifecycle state (e.g. releasing an unfunded/`Created` or `Disputed` escrow) |
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
