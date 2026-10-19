# Mentorship Identity And Trust Indicators

`contracts/mentorship-identity` publishes explainable mentorship profile and
trust indicators without storing profile content, requests, sessions, reports,
or verification evidence. Soroban storage and invocation arguments are public;
all content commitments should be hashes of canonical data with high-entropy
salts, and private source data must stay off-chain.

## Indicator Model

- **Profile completion:** the mentor submits a section count from 0 to 6 and a
  profile commitment. The contract calculates equal-weight completion basis
  points as `completed_sections * 10,000 / 6`. The six documented sections are
  background, expertise, approach, availability, goals, and delivery
  preferences. The contract stores the count, not the section-level flags or
  their content.
- **Mentee learning profile:** the mentee authorizes a profile containing
  separate commitments for goals, current level, interests, preferred
  language, time zone, accessibility needs, and matching preferences. The
  contract stores no plaintext values. Matching services must resolve these
  values off-chain with the mentee's consent; this contract does not perform
  matching. Updating the profile preserves its creation timestamp and emits
  `MID_MENT` with only the subject and update time.
- **Verification:** the mentor starts a request by explicitly consenting to a
  versioned consent statement and submitting hashes of the provider reference
  and evidence. The verifier marks the request approved or rejected. Approval
  requires a future expiry; rejection requires a nonzero commitment to the
  off-chain reason, never public notes. `is_verified` is true only for a
  consented, approved, unexpired request. The mentor may withdraw consent at
  any time, cancelling a pending request or invalidating an approval.
- **Re-verification:** a pending request cannot be duplicated, and a still-valid
  approval cannot be replaced. Rejected, cancelled, or expired requests may be
  submitted again; the current record increments its attempt number and the
  append-only `VF_SUBMIT`, `VF_REVIEW`, and `VF_CANCEL` events retain the
  previous attempt history.
- **Response and delivery:** an authorized recorder attests rates in basis
  points for a closed measurement window and commits to the source data. The
  published measures are responses divided by eligible mentorship requests
  and delivered sessions divided by sessions due in that window. Raw event
  counts, request/session identifiers, and participant identities are never
  stored by this contract. The recorder must calculate rates from its
  auditable off-chain records.
- **Safety:** an authorized moderator publishes only `Unassessed`, `Clear`,
  `Review`, or `Restricted` and a commitment to the off-chain assessment.
  Reports, allegations, notes, and the reasons behind a status are not accepted
  or stored. A status is an attestation, not an automated finding or penalty.

## Authorization

Initialization is admin-authorized and configures one recorder, verifier, and
moderator. Admin can rotate those service addresses. Mentors authorize their
own profile completion updates. The designated recorder controls performance
attestations; the verifier controls verification attestations and revocation;
the moderator controls safety assessments. All role actions emit public events
containing the indicator, actor, and commitment needed for indexing and audit.
Mentors must authorize each verification submission and any consent withdrawal.

## ABI And Storage

The Soroban ABI is generated from the public contract methods, types, and
errors in the crate. Per-mentor records use persistent storage and receive a
roughly one-year TTL extension when written. `get_indicators` combines the
profile, current performance/verification/safety attestations, and a
verification validity calculation based on the current ledger timestamp.
For verification, that query returns `Expired` when the last approval's expiry
has passed, even though the stored approval record is retained until a
re-verification submission replaces it.