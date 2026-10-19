# Mentor credential claims

`mentorship-credentials` provides an on-chain attestation workflow for MENTOR-012. Mentors submit individual credential or experience claims, and platform-authorized reviewers approve, reject, expire, or revoke each claim independently.

## Privacy and record shape

The contract stores a mentor address, opaque claim ID, category code, experience years, a claim-content hash, an evidence-reference hash, timestamps, status, reviewer address, and a numeric reason code. Experience years may be zero when the credential claim does not assert an experience duration. Claim descriptions, credential details, document contents, issuer correspondence, and evidence locations must remain off-chain. Hashes should be commitments to canonicalized content and opaque references, not direct personal identifiers or URLs.

Claims are publicly readable attestations. Submission events expose the mentor address, opaque claim ID, category code, and experience years. Decision events expose the claim ID, reviewer, status-specific reason code where applicable, and decision timestamp. Do not put private explanations in reason codes or event data. Reason-code meanings are defined by the integrating application.

Each mentor may submit at most 64 claims, and claim IDs cannot be reused for that mentor. A submitted claim is immutable apart from its review state. Review decisions are terminal: only submitted claims may be approved, rejected, or expired; only an approved claim may be revoked. Expiring and revoking are distinct explicit reviewer decisions, each requiring a nonzero reason code. Approval has no reason code. The contract does not interpret category or reason codes, attest to evidence truth, or automatically expire a claim based on wall-clock time.

## Authorization and workflow

The admin initializes the contract and enables or disables reviewer addresses. Mentors authorize their own submissions. Enabled reviewers must authorize every approve, reject, expire, and revoke call. Disabling a reviewer blocks subsequent decisions but does not change earlier attestations. Read access is public so profile clients can verify status without exposing the underlying evidence.

The application should only display a claim as verified while its status is `Approved`. Rejected, expired, and revoked claims remain available for auditability and are never deleted. Keep evidence retrieval off-chain and enforce its own access controls; possession of a claim ID or evidence hash is not authorization.

## ABI and events

Contract ABI methods:

- `initialize(admin)`
- `set_reviewer(admin, reviewer, enabled)`
- `submit_claim(mentor, claim_id, category_code, experience_years, claim_hash, evidence_ref_hash)`
- `approve_claim(reviewer, mentor, claim_id)`
- `reject_claim(reviewer, mentor, claim_id, reason_code)`
- `expire_claim(reviewer, mentor, claim_id, reason_code)`
- `revoke_claim(reviewer, mentor, claim_id, reason_code)`
- `get_claim(mentor, claim_id)`
- `storage_version()`

Topics are `REVGRNT` and `REVRVK` for reviewer access; `CLMSUB` for submission; and `CLMAPR`, `CLMREJ`, `CLMEXP`, and `CLMRVK` for decisions. Events are indexing signals, not authorization tokens.

Persistent records use Soroban TTL extension and must be renewed operationally before expiry. Test reviewer onboarding, each terminal transition, duplicate submissions, and event indexing on the target network before production rollout.
