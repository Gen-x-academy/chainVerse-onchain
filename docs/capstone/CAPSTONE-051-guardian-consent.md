# CAPSTONE-051: Support Guardian Consent When Required

**Track:** Governance | **Component:** on-chain | **Issue:** #1399 | **Status:** specified (not yet implemented)

Goal: Support guardian consent when required.

## Baseline

Revalidated against `main` at `4471755`. Nothing in `main` models guardian
relationships or scoped consent for capstone projects. The consent primitive in
`mentorship-applications` covers accepting a terms version only. It has no
third-party guardian and no material-change handling.

## On-chain

New crate `contracts/capstone-guardian-consent`. It records which minor
participants need guardian consent, binds each one to a guardian relationship
that was verified off-chain, and records scoped consent that can expire or be
revoked.

### Data model

| Type | Fields | Notes |
| --- | --- | --- |
| `GuardianLink` | `tenant_id`, `participant: Address`, `guardian: Address`, `relationship_ref: BytesN<32>`, `verified_by: Address`, `verified_at: u64`, `valid_until: u64`, `active: bool` | `relationship_ref` is an opaque commitment to the verification record held off-chain. |
| `ConsentRequirement` | `project_id`, `participant`, `required: bool`, `scope_mask: u32` | Set by a coordinator. `required = false` means no guardian consent is needed. |
| `GuardianConsent` | `project_id`, `participant`, `guardian`, `scope_mask: u32`, `charter_version: u32`, `charter_hash`, `consent_nonce`, `granted_at`, `expires_at`, `revoked_at: Option<u64>` | |

The scope bits are fixed: `PARTICIPATE = 1`, `DATA_PROCESSING = 2`,
`PUBLICATION = 4`, `EXTERNAL_PARTNER = 8`, `TRAVEL_ONSITE = 16`. Unknown bits are
rejected.

A participant can have at most `MAX_GUARDIANS = 2` links. A participant needs
one active consent per project (any linked guardian can give it).

### Entry points

- `link_guardian(verifier, tenant_id, participant, guardian, relationship_ref, valid_until)`:
  the verifier must hold the `GuardianVerifier` role for the tenant. The chain
  stores only the reference, never the evidence.
- `unlink_guardian(verifier, participant, guardian)`: revokes the link. It also
  ends every consent given through that link.
- `set_requirement(coordinator, project_id, participant, required, scope_mask)`.
- `grant_consent(guardian, project_id, participant, scope_mask, charter_version, charter_hash, consent_nonce, expires_at)`:
  the guardian signs (`require_auth`) and the signed arguments bind the consent
  to that charter version and hash.
- `revoke_consent(guardian, project_id, participant)`: always allowed while
  consent is active, and takes effect immediately.
- `has_valid_consent(project_id, participant, needed_scope) -> bool`: checked by
  `capstone-charters` and `capstone-pauses` before any guarded action.

### Lifecycle

```
None ──grant──▶ Active ──expires_at──▶ Expired
                  │ ├──revoke──▶ Revoked
                  │ └──link revoked / valid_until passed──▶ Void
                  └──material charter change (052)──▶ ReconsentRequired ──grant(new version)──▶ Active
```

A **material change** means a charter amendment (CAPSTONE-052) marked
`material = true`, or a scope it requires that the current consent does not
cover. In either case `has_valid_consent` returns `false` until the guardian
grants consent against the new `charter_version` and `charter_hash`.

## Authorization and tenant boundaries

- Deny by default: a guardian can only consent if an active, unexpired
  `GuardianLink` exists in the same tenant.
- A participant cannot consent for themselves when `required = true`, and a
  guardian cannot give consent outside the scopes they signed for.
- Verifiers, coordinators and guardians are separate roles. Holding one does
  not grant another.

## Replay and idempotency

- `consent_nonce` is single-use per guardian (`NonceUsed`).
- Granting again with the same `(charter_version, scope_mask)` while consent is
  active returns `AlreadyConsented`. The client treats that as success.
- A consent signed for an older charter version returns `StaleCharter`.

## Off-chain boundary

| Stays off-chain | On-chain |
| --- | --- |
| Birth dates, age, ID documents, legal relationship evidence | `relationship_ref` commitment only |
| Guardian and participant names and contact details | Addresses only |
| Jurisdictional rules for when consent is required | `required` flag and `scope_mask` set by the coordinator |
| Consent form wording | `charter_hash` plus the scope bits |

The chain never records that someone is a minor. `required` only says that a
consent gate applies.

## Events and errors

| Event | Topics | Data |
| --- | --- | --- |
| `GDLINK` | `(participant,)` | `(guardian, valid_until)` |
| `GDUNLNK` | `(participant,)` | `(guardian,)` |
| `GDREQ` | `(project_id, participant)` | `(required, scope_mask)` |
| `GDGRANT` | `(project_id, participant)` | `(guardian, scope_mask, charter_version, expires_at)` |
| `GDREVOK` | `(project_id, participant)` | `(guardian,)` |

`ContractError`: `Unauthorized = 1`, `WrongTenant = 2`, `LinkNotFound = 3`,
`LinkInactive = 4`, `TooManyGuardians = 5`, `InvalidScope = 6`,
`ScopeNotCovered = 7`, `StaleCharter = 8`, `NonceUsed = 9`,
`AlreadyConsented = 10`, `ConsentNotFound = 11`, `ConsentExpired = 12`,
`ConsentRevoked = 13`, `ReconsentRequired = 14`, `InvalidExpiry = 15`,
`SelfConsentNotAllowed = 16`.

## Required tests

- Authorization: unlinked guardian, guardian from another tenant, a guardian
  whose link has expired, a participant trying to consent for themselves, and a
  non-verifier calling `link_guardian`.
- Replay: reused nonce, and consent signed for charter v1 submitted after v2 is
  current.
- Transitions: every edge in the diagram, including the cascade that voids
  consent when a link is revoked.
- Expiry: consent at `expires_at` is valid, and at `expires_at + 1` it is not.
  The same check applies to the link's `valid_until`.
- Storage bounds: `MAX_GUARDIANS + 1` links, and scope bits outside the mask.
- Event schema snapshots, error-code assertions, the resource budget for
  `has_valid_consent`, and upgrade compatibility of `GuardianConsent`.

## Dependencies, rollout and rollback

- Depends on `capstone-charters` (CAPSTONE-050/052) for charter versions and the
  material-change flag, and on the tenant role registry.
- Used by `capstone-charters` (guardian acceptance counts toward the threshold)
  and `capstone-pauses` (resuming with a changed scope).
- Rollout: start with `required = false` everywhere, then turn it on per tenant
  once the backend verification flow is live.
- Rollback: set `required = false` (no migration needed). Links and consents
  stay on-chain as inert records.
- Other repositories: the backend handles identity and relationship
  verification, jurisdiction policy and the evidence vault. The frontend
  handles the guardian invite, the consent screen and the re-consent prompt.
