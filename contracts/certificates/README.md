# Certificates Contract

## Purpose

`CertificateContract` mints and manages soulbound (non-transferable) on-chain course
completion certificates. An admin initializes the contract and can pause/unpause
minting. While unpaused, a wallet owner mints a certificate for a `course_id` by
presenting an ed25519 signature ("proof") from a trusted backend attesting to
completion; the contract verifies that proof on-chain before recording the
certificate. Certificates cannot be transferred, and a wallet cannot mint the same
`course_id` twice.

## Entry points

| Function | Parameters | Who can call | Returns |
|---|---|---|---|
| `init` | `admin: Address` | `admin` (`require_auth`) — only once | `Result<(), ContractError>` |
| `toggle_pause` | `caller: Address, paused: bool` | admin only (`require_auth` + admin check) | `Result<(), ContractError>` |
| `is_paused` | — | anyone | `bool` |
| `mint` | `wallet: Address, course_id: u64, backend_public_key: Bytes, proof: Bytes` | `wallet` (`require_auth`) | `Result<(), ContractError>` |
| `get_certificate` | `wallet: Address, course_id: u64` | anyone | `Option<Certificate>` |
| `has_certificate` | `wallet: Address, course_id: u64` | anyone | `bool` |
| `transfer` | `_from: Address, _to: Address, _course_id: u64` | n/a | `Result<(), ContractError>` — always returns `Err(SoulboundTransferNotAllowed)` |

## Storage layout

| `DataKey` variant | Storage class | Stored type | TTL |
|---|---|---|---|
| `Admin` | instance | `Address` | none (no `extend_ttl` in crate) |
| `Paused` | instance | `bool` (defaults to `false` if unset) | none |
| `Certificate(Address, u64)` | persistent | `Certificate { wallet: Address, course_id: u64, issued_at: u64 }` | none |

## Events

| Event | Topics | Data |
|---|---|---|
| Certificate minted | `(symbol_short!("cert_mint"), wallet: Address)` | `course_id: u64` |

## Error codes

| Variant | Code | When returned |
|---|---|---|
| `AlreadyInitialized` | 1 | `init` called more than once |
| `NotInitialized` | 2 | `toggle_pause` called before `init` |
| `Unauthorized` | 3 | `toggle_pause` caller isn't the admin |
| `ContractPaused` | 4 | `mint` called while paused |
| `CertificateExists` | 5 | `mint` called twice for the same wallet + course |
| `InvalidProof` | 6 | Backend ed25519 signature fails verification |
| `SoulboundTransferNotAllowed` | 7 | `transfer` called at all — certificates are non-transferable |

## Testing

```
cargo test -p certificates --features testutils
```

`--features testutils` is required: the `pause_tests` and `security_tests` integration
targets are declared with `required-features = ["testutils"]` in `Cargo.toml` and are
skipped without it. Additional coverage lives in `tests/access-control.rs`,
`tests/cross_contract.rs`, `tests/types.rs`, and the inline `src/test.rs` module.

## Example CLI invocations

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/certificates.wasm \
  --source alice \
  --network local

export CONTRACT_ID=<id from deploy output>

# Initialize with an admin
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  init --admin $ADMIN

# Mint a certificate (proof/backend_public_key are hex-encoded bytes from your backend)
soroban contract invoke --id $CONTRACT_ID --source wallet --network local -- \
  mint --wallet $WALLET --course_id 42 \
  --backend_public_key $BACKEND_PUBKEY_HEX --proof $PROOF_HEX

# Check certificate state
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  has_certificate --wallet $WALLET --course_id 42
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  get_certificate --wallet $WALLET --course_id 42

# Pause / unpause minting (admin only)
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  toggle_pause --caller $ADMIN --paused true
```
