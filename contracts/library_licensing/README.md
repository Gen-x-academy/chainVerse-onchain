# Library Licensing: Access Grants

## Issues #946 and #949

Access grants are **soul-bound** to their patron. The ABI exposes `transfer_access_grant(caller, grant_id, new_patron)` as an explicit policy surface; it always returns `NonTransferable` for the current patron and performs no storage or event mutation. A non-patron caller receives `Unauthorized` before authentication or mutation. Institutional reassignment therefore requires a separate governed workflow rather than a bearer-style transfer.

Each `AccessGrant` is bound to the authoritative `license_id`/`loan_id`, the patron (`grantee`), the rendition identifier, and an exclusive expiry window. The `commitment` is a SHA-256 digest over the authoritative loan ID, patron address XDR, rendition ID, and expiry. `access_grant_commitment(grant_id)` returns only this fixed-size digest, while `verify_access_grant(grant_id, patron, rendition_id)` checks the stored binding, commitment, parent-license status, and both validity windows.

Returning a loan is governed by the administrator through `return_loan(caller, loan_id)`, which delegates to the existing revocation path. Verification becomes false immediately when the parent loan is returned or revoked, while the grant record and commitment remain queryable for audit and indexer reconciliation. Returned or expired grants cannot be used to authorize access.

## Impact notes

| Area | Impact |
|---|---|
| ABI | Adds `loan_id`, `rendition_id`, and `commitment` to `AccessGrant`; adds `verify_access_grant`, `access_grant_commitment`, `transfer_access_grant`, and `return_loan`. The `GRANT_NEW` event now includes rendition and commitment data. |
| Storage | Existing grant records gain three fields. The parent `License` remains authoritative; no access URL or secret is stored. The licensing crate is registered in the contract workspace for consistent builds and ABI generation. |
| Events | Grant creation emits the grant ID, authoritative license ID, patron, rendition ID, start, expiry, and commitment. Rejected transfers emit nothing. Return/revocation uses the existing `LIC_REVK` event. |
| Privacy | Only a hash commitment and opaque 32-byte identifiers are exposed for backend verification. Access URLs, secrets, and raw rendition locations remain off-chain. |
| Deployment | Rebuild the licensing WASM and regenerate the contract specification/clients before deployment. Existing deployments require an explicit migration or replacement deployment because the serialized `AccessGrant` schema has changed. |
| Migration | Legacy grants without commitment/binding fields must not be treated as verified. A migration may populate fields only from an authoritative loan source; otherwise legacy records should be invalidated and reissued. Preserve old grant IDs where audit continuity is required, but never infer secrets or URLs on-chain. |

The tests cover successful commitment generation and verification, wrong patron and rendition failures, unknown grant handling, expiry boundaries, returned-loan invalidation, authorization failures, distinct non-transferable errors, and atomic rejection without mutation.

## Validation

Run the licensing tests for the native host target with the repository-compatible Rust toolchain:

```text
cd contracts
rustup run 1.85.0-x86_64-unknown-linux-gnu cargo test -p library-licensing --target x86_64-unknown-linux-gnu
```

The contract library itself can be checked for the wasm build with:

```text
cargo check -p library-licensing --lib
```

No PR is created by this change; the implementation branch is intended to be reviewed through the PR creation link supplied by the contributor.

> **Compatibility note:** The current lockfile is version 4, so Cargo 1.85 or newer is required for local validation.

## License model

A license window is inclusive at `not_before` and exclusive at `expires_at`. A grant is valid only while its own window and its authoritative parent license window are active and the parent status is `Active`.

Institutional transfer policy is intentionally not conflated with patron transfer. If an institution needs to issue a new access grant, it must do so through an authorized license/loan operation and produce a new grant commitment bound to the new authoritative record.

## Change history

- **#946:** Explicitly encode soul-bound patron access and governed return/revocation behavior.
- **#949:** Add queryable grant commitments and verification against the authoritative loan without exposing access URLs.

## ABI regeneration

After building the contract, regenerate the Soroban specification using the project’s normal release/deployment workflow. The generated ABI must include the new methods and `AccessGrant` fields; do not manually edit generated artifacts.
