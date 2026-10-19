# Mentorship Conduct Contract

- Status: Implemented
- Owner: Mentorship On-chain working group
- Related issue: #1247 — `MENTOR-051` Enforce mentorship code of conduct
  (Agreements track)
- Depends on / relates to: `docs/mentorship/` design notes for the safety
  work items (`MENTOR-085` … `MENTOR-092`) and, off-chain, the conduct policy
  text and case-management system

## Scope

`contracts/mentorship-conduct` makes acceptance of the current mentorship
code of conduct a provable, versioned precondition for a mentorship
engagement. It implements the **On-chain** half of `MENTOR-051`:

- **Versioned policies.** `publish_policy` (admin-only) stores an immutable
  policy version with a `policy_hash` commitment, an audience
  (`Mentor`, `Mentee`, or `Both`), an `effective_at`, and a `mandatory`
  flag. Versions are strictly sequential, so clients can always tell whether
  they are looking at the newest policy. `retire_policy` retires a version
  without deleting it.
- **Provable acceptance.** `accept_policy(actor, version, role)` records a
  `Acceptance` proof: participant, role, version, the accepted policy hash,
  and the acceptance timestamp. Accepting twice is a no-op (retries never
  duplicate effects and never overwrite the original timestamp), and the
  proof survives retirement so `verify_acceptance` keeps working after the
  policy changes.
- **Activation gate.** `create_engagement` requires both parties'
  authorization, and `activate_engagement` fails with `AcceptanceRequired`
  unless each party has accepted the latest mandatory policy that applies to
  their role — or with `PolicyNotFound`/`PolicyRetired` when there is no
  acceptable current policy, so mentorship fails closed.
- **Re-acknowledgement.** Publishing a newer mandatory policy makes
  `engagement_compliant` return `false` and `requires_acknowledgement` return
  `true` until each party accepts the new version. `resume_engagement` after
  a suspension re-checks the same condition, so a paused relationship cannot
  bypass a policy update.
- **Breach references.** `record_breach` (admin-only) stores a case
  reference, the subject, the breached policy version, a severity, and an
  evidence hash; `resolve_breach` closes it. Duplicate case references are
  rejected, so a retried report cannot create a second case.

**Not implemented here:** the conduct policy text itself, breach narratives,
evidence files, case triage and protective measures, and any enforcement on
funds. Those are off-chain (see the `MENTOR-085` … `MENTOR-092` design notes,
and `contracts/mentorship-calendar` / `contracts/mentorship-slots` for the
scheduling side).

## Privacy

- Policy text is never stored: only its `policy_hash` commitment.
- Breach records carry commitments and public bookkeeping only — no
  narrative, no reporter identity, no evidence content. Evidence stays in the
  off-chain safety system behind `evidence_hash`.
- Participant addresses, roles, versions, and timestamps are inherently
  public ledger data. If a participant address is linkable to a real person,
  that linkage comes from off-chain data, not from this contract.
- Policy retirement does not erase acceptance proofs: acceptance is the
  privacy-minimal record that a person agreed to a specific text version and
  must remain provable.

## Ownership

- **Admin.** `initialize` sets a single admin that publishes and retires
  policies, suspends an engagement, and records/resolves breaches.
- **Participants.** Each party accepts policies for their own role; either
  party can create, activate, resume, or end an engagement. A non-party
  caller is rejected with `NotAuthorized`, and engagement creation requires
  both parties' authorization, which is the stronger condition.
- **Authorization is deny-by-default.** Admin entry points compare the caller
  against the stored admin and then require its signature; participant entry
  points require the participant's own signature. Nothing is readable or
  writable without an explicit check.

## Migration

New contract; no prior on-chain state. After deployment the admin must
`initialize`, then publish the first policy version before any engagement can
activate. Existing engagements created by other tooling have no on-chain
record here, so only engagements created after deployment are gated.
`CONTRACT_VERSION` is exposed as `version()` for upgrade validation.

Storage is versioned by the policy `version` counter (capped at
`MAX_POLICY_VERSION`, default 1,000) and by `EngagementState`, so future
entry points can add fields without changing the acceptance semantics.

## Operational impact

- **Storage/TTL.** Policies, acceptances, engagements, and breach records are
  `persistent` with a ~1-year TTL refreshed on read and write. The policy
  version cap bounds the policy registry; acceptances are one record per
  (actor, version, role); engagements and breaches are keyed by a caller
  supplied 32-byte id.
- **Events.** `POLPUB`, `POLRTD`, `ACCEPT`, `ENGNEW`, `ENGACT`, `ENGSUS`,
  `ENGRES`, `ENGEND`, `BREACH`, and `BRRES` carry the ids, versions, roles,
  and severities an indexer needs to build compliance timelines.
- **Rollout.** Ship behind the mentorship feature flag: publish the policy
  versions first, let participants accept them, and only then enable
  engagement activation. Because a newly published *mandatory* version
  immediately makes existing engagements non-compliant, publish new versions
  outside active session windows and expect an acceptance spike.
- **Failure codes.** Errors are stable and numbered
  (`PolicyNotFound = 5`, `AcceptanceRequired = 12`, `EngagementNotPending =
  16`, `BreachAlreadyRecorded = 21`, …) so clients can branch on them.
- **Cross-repository work.** The matching `MENTOR-051` work in the other
  ChainVerse repositories owns the policy text, the acceptance UI, and the
  case-management workflow that produces the hashes recorded here.
