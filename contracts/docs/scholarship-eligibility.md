# Scholarship Eligibility Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1066 (composable eligibility rules), #1068 (eligibility
  attestations with expiry), #1067 (prerequisite/exclusion rules — not yet
  implemented), #1069 (application drafts — not yet implemented, separate
  contract)

## Scope

This contract (`contracts/scholarship-eligibility`) implements two of the
"Scholarships On-chain/Eligibility" epic's behaviors:

- **#1066 — Build composable eligibility rules.** `publish_eligibility_rule()`
  publishes an immutable, versioned list of required attestation types for
  a program. The list is validated before publication (must be non-empty
  and at most `MAX_RULE_REQUIREMENTS = 10` entries — bounded storage), and
  republishing always creates a new version rather than mutating an
  existing one, so evaluation against a historical version stays
  reproducible.
- **#1068 — Add eligibility attestations with expiry.** `issue_attestation()`
  lets an authorized issuer (admin-managed allowlist) claim that a subject
  satisfies a named attestation type, scoped to a program (or globally, via
  `global_scope()`), valid until an explicit expiry. `revoke_attestation()`
  (issuer or admin only) invalidates it immediately, and
  `has_valid_attestation()`/`evaluate_eligibility()` check both revocation
  and expiry on every read — an expired or revoked attestation is invalid
  the moment either condition is true, with no separate cleanup step
  required.

`evaluate_eligibility()` ties the two together: a subject is eligible for a
program iff every attestation type required by that program's *latest*
published rule has a currently-valid attestation for that subject, scoped
either to the program or globally. This is a pure read over current state —
same inputs always produce the same output, satisfying "evaluation is
deterministic."

**Not implemented here** (tracked separately): prerequisite and exclusion
rules between programs (#1067) — cycle/contradiction detection across a
graph of program relationships is materially more complex than a flat
per-program requirement list, and application drafts (#1069), which belong
to a different contract (`scholarship-applications`) and lifecycle.

## Privacy

This contract never stores the evidence behind a claim — no grades, income
figures, documents, or other sensitive data. An attestation is only ever
"issuer X vouches that subject Y satisfies named type Z, until expiry E."
The actual evidence stays with the issuer off-chain. Attestation types are
plain `Symbol`s (e.g. `enrolled`, `income_band`) chosen by whoever designs
a program's rule — this contract has no opinion on what they mean, which
keeps it from needing to reason about sensitive categories at all.

## Ownership

The admin (`initialize`) exclusively controls the issuer allowlist and rule
publication. Only an allowlisted issuer can create attestations, and only
that same issuer (or the admin) can revoke one — a subject cannot revoke
their own attestation, and a non-issuing third party cannot revoke someone
else's.

## Migration

New contract, no prior on-chain state. Setup order: `initialize` →
`add_issuer` (one or more) → `publish_eligibility_rule` per program →
issuers can then `issue_attestation`. `evaluate_eligibility` fails with
`NoRulePublished` until a program has a rule.

## Operational impact

- Removing an issuer (`remove_issuer`) only blocks *new* attestations from
  them; attestations they already issued remain valid until their own
  expiry or an explicit `revoke_attestation` call. If an issuer is removed
  because they're no longer trusted, their existing attestations likely
  need to be revoked individually (or a rule bump used to stop relying on
  that attestation type) — this contract does not cascade-revoke.
- Republishing a program's rule does not re-evaluate or notify anyone;
  `evaluate_eligibility` simply starts reading the new version on its next
  call. A subject who was eligible under the old rule may become
  ineligible under the new one with no on-chain signal beyond the
  `RULEPUB` event — any off-chain UI should listen for that event and
  re-check affected subjects.
- Storage/TTL/events follow the same conventions as
  `scholarship-applications` (persistent storage, ~1-year TTL bump on
  write, no upgrade/pause admin function yet in this pass).
