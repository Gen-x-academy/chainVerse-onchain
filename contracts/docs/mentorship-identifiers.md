# Mentorship Public Identifiers

The `mentorship-identifiers` Soroban contract assigns a stable, non-sequential
public ID and a correlation ID to each off-chain mentorship record.

## ABI

- `initialize(admin)`: one-time setup by the issuer address.
- `register(issuer, reference_hash)`: issuer-authenticated registration.
  Re-registering a reference returns the original record without another
  registration event.
- `get_by_reference(reference_hash)`: resolves a known internal-reference
  commitment.
- `get_by_public_id(public_id)`: resolves the public record without exposing
  the internal-reference commitment.

The `MentorshipIdentity` result contains `public_id`, `correlation_id`,
`created_at`, and `issued_by`. `MTRREG(public_id, correlation_id)` is emitted
only for a new registration. The Soroban contract macro supplies the ABI.

## Privacy and traceability

The caller keeps the original relational key and maps it off-chain to
`reference_hash`. This value must be a high-entropy commitment (for example, a
random per-record reference hashed with a private salt), never the raw key or
a plain hash of low-entropy personal data. The contract stores this 32-byte
commitment as an opaque lookup key; private profiles, session content, and
payment details do not belong in this contract.

Public and correlation IDs are SHA-256 digests with separate domain
separators. Their input includes the contract address and a checked
monotonically increasing issuance counter, so the returned IDs are not
sequential. Duplicate registration is idempotent. Keep the off-chain mapping
for relational traceability and use the emitted correlation ID to join
indexer and service logs.

## Storage lifetime

Persistent identity entries and instance-level issuer/counter data are
extended to the network-supported maximum TTL when used. Soroban state can
still expire after prolonged inactivity; deployments must periodically touch
active mappings and preserve their off-chain mapping and event history.
