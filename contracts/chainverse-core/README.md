# Chainverse Core Contract

## Purpose

`ChainverseCore` combines protocol admin/config management with a token-escrow system.
An admin is configured once at `initialize`, after which protocol behavior can be
gated with a pause switch and a supported-token allowlist. A depositor can lock a
fungible amount of a supported token in escrow for a recipient; the escrow can then be
released or cancelled by the depositor, or refunded by anyone once an optional expiry
has passed. The contract also tracks lightweight on-chain analytics (per-event
counters) and exposes read-only query helpers (get/search/paginate/list-by-party/active
escrows) plus a basis-point protocol-fee calculator.

> **Known issue:** `contracts/chainverse-core/tests/` contains several files
> (`math_test.rs`, `access_control.rs`, `execution.rs`, `init_test.rs`) that reference
> modules which do not exist in this crate's `src/` (e.g. `crate::rules_engine`,
> `crate::breach_detector`, `crate::errors::AccessControlError`) — they appear to be
> copy-pasted from unrelated compliance/governance work and are not wired into this
> contract. They will very likely break `cargo test -p chainverse-core`. This wasn't
> fixed as part of this documentation pass — flag it to the repo owner before relying on
> the Testing command below.
>
> Also dead/unused in the current code, not to be treated as active behavior:
> `src/escrow/status_validator.rs::validate_transition` references an `EscrowStatus::Released`
> variant that doesn't exist (the real variants are listed below), and is never called;
> `src/events.rs::CoursePurchasedEvent` is defined but never published.

## Entry points

| Function | Parameters | Who can call | Returns |
|---|---|---|---|
| `initialize` | `admin: Address, protocol_fee: u32, supported_tokens: Vec<Address>` | anyone, once (guarded by `AlreadyInitialized`) | `Result<(), ContractError>` |
| `is_paused` | — | anyone | `bool` |
| `pause` | `caller: Address` | admin (`require_auth`) | `Result<(), ContractError>` |
| `unpause` | `caller: Address` | admin (`require_auth`) | `Result<(), ContractError>` |
| `get_config` | — | anyone | `Result<Config, ContractError>` |
| `update_config` | `caller: Address, new_protocol_fee: Option<u32>, new_supported_tokens: Option<Vec<Address>>` | admin (`require_auth`) | `Result<(), ContractError>` |
| `transfer_admin` | `caller: Address, new_admin: Address` | admin (`require_auth`) | `Result<(), ContractError>` |
| `create_escrow` | `depositor: Address, recipient: Address, token: Address, amount: i128, expires_at: u64` | `depositor` (`require_auth`); blocked while paused | `Result<u64, ContractError>` |
| `release_escrow` | `caller: Address, id: u64` | `caller` must equal the escrow's depositor (`require_auth`); blocked while paused | `Result<(), ContractError>` |
| `cancel_escrow` | `caller: Address, id: u64` | `caller` must equal the escrow's depositor (`require_auth`); blocked while paused | `Result<(), ContractError>` |
| `buyer_cancel_escrow` | `buyer: Address, id: u64` | `buyer` must equal the escrow's depositor (`require_auth`), escrow must be `Pending`; blocked while paused | `Result<(), ContractError>` |
| `refund_expired_escrow` | `id: u64` | permissionless — requires `expires_at != 0`, current time ≥ `expires_at`, status `Pending`; blocked while paused | `Result<(), ContractError>` |
| `get_escrow` | `id: u64` | anyone | `Result<EscrowRecord, ContractError>` |
| `get_escrows_by_buyer` | `buyer: Address` | anyone | `Vec<EscrowRecord>` |
| `get_escrows_by_seller` | `seller: Address` | anyone | `Vec<EscrowRecord>` |
| `event_count` | `event: Symbol` | anyone | `u64` |
| `get_escrow_stats` | — | anyone | `analytics::Stats` |
| `search_escrows` | `token: Option<Address>, status: Option<EscrowStatus>` | anyone | `Vec<EscrowRecord>` |
| `get_active_escrows` | — | anyone | `Vec<EscrowRecord>` |
| `is_token_supported` | `token: Address` | anyone | `bool` |
| `calculate_fee` | `amount: i128` | anyone | `Result<i128, ContractError>` |

## Storage layout

No `extend_ttl` calls exist anywhere in the crate; every write uses the default TTL of
its storage class.

| Key enum / variant | Storage class | Stored type |
|---|---|---|
| `storage::DataKey::Config` | persistent | `Config { admin: Address, protocol_fee: u32, supported_tokens: Vec<Address> }` |
| `admin::AdminKey::Paused` | instance | `bool` |
| `analytics::AnalyticsKey::EventCount(Symbol)` | instance | `u64` (per-event counter) |
| `escrow::EscrowKey::Record(u64)` | persistent | `EscrowRecord { id, depositor, recipient, token, amount: i128, status: EscrowStatus, created_at: u64, expires_at: u64 }` |
| `escrow::EscrowKey::NextId` | persistent | `u64` (monotonic id counter) |

`EscrowStatus` variants: `Pending`, `Completed`, `Refunded`, `Cancelled`, `Expired`.

## Events

| Event | Topics | Data |
|---|---|---|
| Analytics counter update | `(symbol_short!("analytics"), event: Symbol)` | `u64` — new counter value |

`event` is one of these `symbol_short!` constants, recorded from the listed entry points:

| Symbol constant | Value | Recorded in |
|---|---|---|
| `EVT_ESCROW_CREATED` | `"ESC_NEW"` | `create_escrow` |
| `EVT_ESCROW_RELEASED` | `"ESC_REL"` | `release_escrow` |
| `EVT_ESCROW_CANCELLED` | `"ESC_CAN"` | `cancel_escrow`, `buyer_cancel_escrow`, `refund_expired_escrow` |
| `EVT_CONFIG_UPDATED` | `"CFG_UPD"` | `update_config` |
| `EVT_ADMIN_CHANGED` | `"ADM_CHG"` | `transfer_admin` |

## Error codes

| Variant | Code | When returned |
|---|---|---|
| `Unauthorized` | 1 | Caller doesn't match required admin/depositor |
| `AlreadyInitialized` | 2 | `initialize` called more than once |
| `NotInitialized` | 3 | Config/admin read before `initialize` |
| `ContractPaused` | 4 | Mutating escrow call made while paused |
| `InvalidAmount` | 5 | Escrow amount invalid (e.g. non-positive) |
| `UnsupportedToken` | 6 | Token not in `supported_tokens` |
| `EscrowNotFound` | 7 | `id` has no matching escrow record |
| `InvalidEscrowState` | 8 | Action attempted on an escrow in the wrong status |
| `EscrowNotExpired` | 9 | `refund_expired_escrow` called before `expires_at` |

## Testing

```
cargo test -p chainverse-core --features testutils
```

`--features testutils` is required for the `security_tests`, `governance_dao_tests`,
and `fraud_prevention_tests` integration targets (declared with
`required-features = ["testutils"]`). See the known-issue note above — some files
under `tests/` may currently fail to compile regardless of this flag.

## Example CLI invocations

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/chainverse_core.wasm \
  --source alice \
  --network local

export CONTRACT_ID=<id from deploy output>

# Initialize
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  initialize --admin $ADMIN --protocol_fee 250 --supported_tokens '["'$TOKEN_ID'"]'

# Create / release / cancel an escrow
soroban contract invoke --id $CONTRACT_ID --source depositor --network local -- \
  create_escrow --depositor $DEPOSITOR --recipient $RECIPIENT --token $TOKEN_ID \
  --amount 1000 --expires_at 1735689600
soroban contract invoke --id $CONTRACT_ID --source depositor --network local -- \
  release_escrow --caller $DEPOSITOR --id 1
soroban contract invoke --id $CONTRACT_ID --source depositor --network local -- \
  cancel_escrow --caller $DEPOSITOR --id 1

# Refund an expired escrow (anyone may call)
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  refund_expired_escrow --id 1

# Read state
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  get_escrow --id 1
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  get_escrow_stats
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  calculate_fee --amount 1000
```
