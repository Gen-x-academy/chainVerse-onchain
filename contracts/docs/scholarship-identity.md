# scholarship-identity

Contract: `scholarship-identity`  
Version: 1  
Audited against: `Gen-x-academy/chainVerse-onchain` `origin/main` commit `82c681a1fbdf5984b4b6adaee50c053af21bbe09`

---

## Purpose

Verifies applicant identity and active enrollment through approved providers,
retaining the minimum data required on-chain. Provider credentials and
evidence never reach this contract.

---

## Trust model

```
Admin
 ├─ registers Providers (by hash of their public ID — never the credential)
 ├─ registers ClaimTypes (controlled vocabulary)
 └─ authorizes Verifiers (manual fallback)

Provider (approved)
 └─ calls issue_claim(issuer, subject, provider_id, scope, claim_type, expiry)

Verifier (approved, fallback)
 └─ calls issue_manual_claim(verifier, subject, scope, claim_type, expiry)

Admin / original issuer
 └─ calls revoke_claim(...)
```

### Off-chain trust boundary

The evidence backing a claim (grade transcript, enrollment record, income
document) lives entirely with the issuer. This contract stores only that an
approved issuer vouched for a named claim type for a subject, within a scope,
until an expiry. Raw provider API keys or credentials must never appear as
call arguments; they are referenced only by their hash (`provider_id`).

---

## Claim key structure

Claims are keyed by `(subject, issuer, scope, claim_type)`.

- **Issuer-scoped**: claims from different issuers never collide.
- **Scope-specific**: a program-scope claim and a global-scope (all-zero)
  claim are separate records. `is_verified` checks both.
- **Expiring**: `expiry` must be strictly greater than the current ledger
  timestamp at issuance. Expired claims return `false` from `has_valid_claim`
  without any contract call.

---

## Storage

| Key | Storage tier | TTL |
|---|---|---|
| `Admin` | Instance | Contract lifetime |
| `ClaimType(symbol)` | Instance | Contract lifetime |
| `Provider(BytesN<32>)` | Persistent | ~1–2 years |
| `Verifier(Address)` | Persistent | ~1–2 years |
| `Claim(subject, issuer, scope, type)` | Persistent | ~1–2 years |
| `ClaimCount(subject)` | Persistent | ~1–2 years |

---

## Bounds

- `MAX_CLAIMS_PER_SUBJECT = 32` — prevents unbounded per-subject storage.
- Claim type vocabulary controlled by admin — no arbitrary strings on-chain.
- Zero-hash provider_id rejected at registration.

---

## Events

| Symbol | Payload | When |
|---|---|---|
| `CTYPEREQ` | `(claim_type)` | Claim type registered |
| `PROVREG` | `(provider_id, admin)` | Provider registered |
| `PROVRVK` | `(provider_id)` | Provider revoked |
| `VRFYADD` | `(verifier)` | Verifier added |
| `VRFYRVK` | `(verifier)` | Verifier removed |
| `CLMISSUE` | `(issuer, subject, claim_type, scope)` | Claim issued |
| `CLMRVK` | `(caller, subject, claim_type, scope)` | Claim revoked |

---

## Privacy

- No PII stored — only `Address` and `Symbol` claim-type names.
- Evidence lives entirely off-chain.
- Revoked claims set `revoked = true`; they are not deleted (audit trail).
- `revoked_at` timestamp is stored, never the reason or content.

---

## Ownership

Contract admin controls the claim-type vocabulary, provider registry, and
verifier allowlist. Per-claim revocation is available to the original issuer
or the admin.

---

## Migration

- New claim types: add via `register_claim_type`; existing claims unaffected.
- Provider hash rotation: register a new provider hash and re-issue claims;
  revoke the old provider to block new issuance under it.
- Claim key structure is stable; no migration needed when adding issuers.

---

## Operational impact

- `issue_claim`: 2 persistent reads (provider, existing claim) + 1–2 writes.
- `has_valid_claim`: 1 persistent read; suitable for hot-path checks.
- `is_verified` (multi-issuer): O(n) reads where n = number of issuers passed;
  keep the issuer list bounded at call sites.
