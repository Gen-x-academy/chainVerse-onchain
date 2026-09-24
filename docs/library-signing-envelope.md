# Canonical library signing envelope

Tracks issue #1007. Implementation: `contracts/shared/src/signing.rs`.

A signature the library accepts must mean exactly one thing. Without domain
separation, a signature gathered for a fine waiver on testnet is also a valid
signature for an issuer offer on mainnet against a different contract — the
bytes signed are identical, so whoever holds one authorization holds all of
them.

Every off-chain signature the library verifies is taken over
`signing_payload()`, a SHA-256 digest over a fixed 150-byte preimage.

## Preimage layout

| Offset | Size | Field                                                  |
|--------|------|--------------------------------------------------------|
| 0      | 18   | `ChainVerse.Library` (ASCII domain tag)                 |
| 18     | 4    | envelope version, `u32` **big-endian**                  |
| 22     | 32   | network id — `sha256(network passphrase)`               |
| 54     | 32   | `sha256(contract address XDR)`                          |
| 86     | 32   | `sha256(message type XDR)`                              |
| 118    | 32   | body hash — `sha256` of the message-specific payload    |

Total: 150 bytes. `signing_payload = sha256(preimage)`.

Every field is fixed-width, so there is no length-prefix ambiguity and no way
for two different field sets to serialize to the same bytes. The contract
address and message type are hashed rather than inlined because their XDR
encodings are variable-length; hashing them first keeps every later field at a
fixed offset.

## What each field defends against

| Field         | Attack it prevents                                             |
|---------------|-----------------------------------------------------------------|
| domain tag    | Collision with any other ChainVerse signing scheme               |
| version       | An old signature silently remaining valid under a new layout     |
| network id    | Replaying a testnet signature on mainnet                         |
| contract      | Replaying a signature against a different deployment             |
| message type  | An issuer offer doubling as a membership claim                   |
| body hash     | Substituting a different payload under the same authorization    |

## Message types

`MessageType` covers `IssuerOffer`, `MembershipClaim`, `FineAssessment`,
`DelegatedSession`, and `DebtSettlement`. Each maps to a spelled-out `Symbol`
tag via `MessageType::tag()` rather than to its enum discriminant, so
**reordering the enum can never silently change a payload hash**. Adding a
variant means adding a tag; it does not disturb existing ones.

## Generating matching fixtures off-chain

`canonical_preimage()` is `pub` specifically so a fixture generator can emit the
preimage rather than a second implementation of the layout drifting from this
one. A TypeScript backend producing the same payload must:

1. compute `networkId = sha256(utf8(networkPassphrase))`;
2. compute `contractHash = sha256(xdr(contractAddress))`;
3. compute `messageTypeHash = sha256(xdr(Symbol(tag)))`;
4. concatenate the six fields in the order and widths above;
5. `sha256` the result.

The Rust and TypeScript fixtures are comparable byte-for-byte at step 4, before
the final hash, which is the useful place to diff when they disagree.

## Versioning

`ENVELOPE_VERSION` is `1`. `check_version()` runs before anything is hashed, so
an unknown or forged version can never reach the hashing path and produce a
payload that looks legitimate — `canonical_preimage()` and `signing_payload()`
both return `SigningError::UnsupportedVersion` rather than a digest.

Changing the layout means bumping the version. Signatures from the previous
version then produce a different digest and stop verifying, which is the
intended behaviour: a layout change is exactly when old signatures must stop
being honoured.

## Adopting the envelope

Adding this module changes nothing on its own. Switching a contract that
already verifies signatures over to `signing_payload()` **is** a breaking change
for signatures already in flight, so that migration should land as its own
change with its own version bump and a documented cutover.

## Impact

Additive library code in the `shared` crate. No ABI, storage, event, privacy,
deployment, or migration impact: no contract entrypoint changes and no storage
key is introduced. `SigningError` occupies discriminants 220–221, chosen so
they cannot collide with `ContractError` or `MathError`.
