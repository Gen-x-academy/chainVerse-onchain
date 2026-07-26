# Payout Automation Contract

## Purpose

`PayoutAutomation` executes batch token payouts on behalf of an admin-managed
authorization list. An `admin` initializes the contract and can authorize other
addresses; any authorized address can then call `execute` to transfer a token (via the
standard Soroban token client) from the contract's own balance out to a list of
`(recipient, amount)` pairs in a single call.

> **Known issue:** `payout-automation` is not currently listed under
> `[workspace] members` in `contracts/Cargo.toml`, even though its own `Cargo.toml` uses
> `soroban-sdk = { workspace = true }`. Until it's added to the workspace, run tests from
> inside the crate directory (see Testing below) rather than `-p payout-automation` from
> the workspace root.

## Entry points

| Function | Parameters | Who can call | Returns |
|---|---|---|---|
| `initialize` | `admin: Address` | `admin` (`require_auth`) | `()` |
| `add_authorised` | `admin: Address, caller: Address` | `admin` (`require_auth`, checked against stored admin) | `Result<(), PayoutError>` |
| `execute` | `caller: Address, token: Address, payouts: Vec<PayoutEntry>` | any authorized address (`require_auth`) | `Result<(), PayoutError>` |

`PayoutEntry` is `{ recipient: Address, amount: i128 }`. An empty `payouts` vector is a
no-op that returns `Ok(())`.

## Storage layout

Both keys live in **instance** storage; no `extend_ttl` calls exist in the crate.

| `DataKey` variant | Stored type | TTL |
|---|---|---|
| `Admin` | `Address` | none (instance default) |
| `Authorised(Address)` | `bool` — `true` if that address may call `execute` | none (instance default) |

## Events

None — this contract does not publish any events.

## Error codes

| Variant | Code | When returned |
|---|---|---|
| `Unauthorized` | 1 | `add_authorised` called by a non-admin, or `execute` called by a non-authorized address |
| `NotInitialized` | 2 | `add_authorised` called before `initialize` |

## Testing

```
cd contracts/payout-automation
cargo test
```

(`cargo test -p payout-automation` will not find the package until it's added to the
workspace `members` list — see the known issue above.) Covers: valid batch payout to all
recipients, execution rejected from an unauthorized caller, and a graceful no-op on an
empty batch.

## Example CLI invocations

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/payout_automation.wasm \
  --source alice \
  --network local

export CONTRACT_ID=<id from deploy output>

# Initialize with an admin
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  initialize --admin $ADMIN

# Authorize another address to trigger payouts
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  add_authorised --admin $ADMIN --caller $OPERATOR

# Execute a batch payout
soroban contract invoke --id $CONTRACT_ID --source operator --network local -- \
  execute --caller $OPERATOR --token $TOKEN_ID \
  --payouts '[{"recipient":"'$RECIPIENT_1'","amount":"100"},{"recipient":"'$RECIPIENT_2'","amount":"200"}]'
```
