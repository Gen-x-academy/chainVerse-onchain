# Escrow Vault Contract

## Purpose

`EscrowVault` is a multi-approver escrow. A depositor creates a `Vault` that locks an
`amount` of a `token` for a `recipient`, alongside a fixed set of `approvers`. Each
approver may cast one approval; once every approver has approved, the vault's status
moves from `Pending` to `Released`. Double-voting and approvals from non-approvers are
rejected.

Note: the contract only tracks vault state and approval votes in storage — it does not
itself call a token contract to move funds. Any actual token transfer on release must be
handled by the caller/integration layer.

## Entry points

| Function | Parameters | Who can call | Returns |
|---|---|---|---|
| `create_vault` | `depositor: Address, recipient: Address, token: Address, amount: i128, approvers: Vec<Address>` | `depositor` (`require_auth`) | `u64` — new vault id |
| `approve_release` | `caller: Address, vault_id: u64` | any address in the vault's `approvers` list (`require_auth`) | `Result<(), VaultError>` |
| `get_vault` | `vault_id: u64` | anyone | `Result<Vault, VaultError>` |

## Storage layout

Both keys live in **instance** storage; no `extend_ttl` calls exist in the crate.

| `DataKey` variant | Stored type | TTL |
|---|---|---|
| `Vault(u64)` | `Vault { depositor, recipient, token, amount: i128, approvers: Vec<Address>, approvals: Vec<Address>, status: VaultStatus }` | none (instance default) |
| `NextId` | `u64` — auto-incrementing vault id counter | none (instance default) |

`VaultStatus` variants: `Pending`, `Released`, `Cancelled` (`Cancelled` is defined but
never set by current logic).

## Events

None — this contract does not publish any events.

## Error codes

| Variant | Code | When returned |
|---|---|---|
| `NotFound` | 1 | `vault_id` has no matching record |
| `NotPending` | 2 | `approve_release` called on a vault that's already `Released`/`Cancelled` |
| `Unauthorized` | 3 | `caller` is not in the vault's `approvers` list |
| `AlreadyVoted` | 4 | `caller` has already approved this vault |

## Testing

```
cargo test -p escrow-vault
```

Covers: approving after release fails, unauthorized approval is rejected, authorized
approval succeeds, and release requires all approvers.

## Example CLI invocations

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/escrow_vault.wasm \
  --source alice \
  --network local

export CONTRACT_ID=<id from deploy output>

# Create a vault requiring approvals from two addresses
soroban contract invoke --id $CONTRACT_ID --source depositor --network local -- \
  create_vault --depositor $DEPOSITOR --recipient $RECIPIENT --token $TOKEN_ID \
  --amount 500 --approvers '["'$APPROVER_1'","'$APPROVER_2'"]'

# Each approver casts their vote
soroban contract invoke --id $CONTRACT_ID --source approver1 --network local -- \
  approve_release --caller $APPROVER_1 --vault_id 1
soroban contract invoke --id $CONTRACT_ID --source approver2 --network local -- \
  approve_release --caller $APPROVER_2 --vault_id 1

# Read vault state
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  get_vault --vault_id 1
```
