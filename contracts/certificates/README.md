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
| `init` | `admin: Address, backend_public_key: Bytes, minter: Address` | `admin` (`require_auth`) — only once | `Result<(), ContractError>` |
| `toggle_pause` | `caller: Address, paused: bool` | admin only (`require_auth` + admin check) | `Result<(), ContractError>` |
| `is_paused` | — | anyone | `bool` |
| `mint` | `recipient: Address, course_id: BytesN<32>, expires_at: u64, nonce: BytesN<32>, proof: Bytes` | caller supplies a signed proof; rejected when ledger time is `>= expires_at` | `Result<(), ContractError>` |
| `get_certificate` | `recipient: Address, course_id: BytesN<32>` | anyone | `Option<Certificate>` |
| `transfer` | `from: Address, to: Address, course_id: BytesN<32>` | n/a | `Result<(), ContractError>` — always returns `Err(SoulboundTransferNotAllowed)` |

## Storage layout

| `DataKey` variant | Storage class | Stored type | TTL |
|---|---|---|---|
| `Admin` | instance | `Address` | none (no `extend_ttl` in crate) |
| `Paused` | instance | `bool` (defaults to `false` if unset) | none |
| `Certificate(Address, u64)` | persistent | `Certificate { wallet: Address, course_id: u64, issued_at: u64 }` | none |
| `ConsumedNonce(BytesN<32>)` | persistent | `bool` | `MIN_TTL` to `MAX_TTL` |

The backend signs the XDR encoding of `(recipient, course_id, expires_at, nonce)`. A nonce is written to persistent storage only after signature and certificate checks pass, then retained with the certificate storage TTL to prevent replay.

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
| `ProofExpired` | 9 | `mint` called at or after `expires_at` |
| `NonceAlreadyConsumed` | 10 | `mint` reuses a nonce with an active replay-protection entry |

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
  init --admin $ADMIN --backend_public_key $BACKEND_PUBKEY_HEX --minter $MINTER

# Mint a certificate (proof/backend_public_key are hex-encoded bytes from your backend)
soroban contract invoke --id $CONTRACT_ID --source wallet --network local -- \
  mint --recipient $WALLET --course_id $COURSE_ID \
  --expires_at $EXPIRES_AT --nonce $NONCE --proof $PROOF_HEX

# Check certificate state
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  get_certificate --recipient $WALLET --course_id $COURSE_ID

# Pause / unpause minting (admin only)
soroban contract invoke --id $CONTRACT_ID --source alice --network local -- \
  toggle_pause --caller $ADMIN --paused true
```
