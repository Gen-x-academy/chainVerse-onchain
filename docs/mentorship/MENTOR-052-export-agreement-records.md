# MENTOR-052: Export Participant Agreement Records

**Track:** Agreements | **Component:** on-chain | **Status:** implemented (v1)

Goal: Export participant agreement records.

## On-chain

`contracts/mentorship-agreement-export` provides a privacy-safe export
bundle containing the accepted terms version and hash, amendments,
participant approvals, session summaries and the financial summary.

- Only the agreement's mentor or mentee can request or download an
  export; grants are personal.
- Grant generation is rate-limited per participant in a rolling window
  and every request/download is appended to a bounded on-chain audit log.
- Grants expire; an expired link cannot download and must be replaced by a
  fresh grant.
- The manifest has no free-text fields, so private internal notes and
  messages are excluded by construction rather than by filtering.

## Off-chain boundary

Terms text, signed acceptance artifacts, amendment wording, private
notes, session summary prose and the rendered export document live in the
app. The chain stores commitments, versions, timestamps and amounts, and
returns them on demand for the participant to render. The audit log
records that an export happened and who downloaded it, never the content.

## Rules

- Only participants can export; a grant cannot be downloaded by anyone
  else.
- Quota: `max_exports_per_window` per rolling `window_seconds`; the next
  request fails with `ExportRateLimited`.
- Grant TTL is bounded to 60s–7 days.
- Retrying the same grant id and scope returns the same grant without
  consuming quota or duplicating audit entries; reusing a grant id for a
  different scope fails.
- Bounded storage: amendments, approvals, summaries and audit entries are
  all capped; a full audit log refuses further exports loudly.
- Events `EXPREQ`/`EXPDL` are emitted once per request/download.

## Events and errors

Events `EXPREQ(grant_id)`, `EXPDL(grant_id)`, `AGREG`, `APPROVED`,
`AMENDED`, `SUMMARY`, `EXPOLICY`. Errors: `NotAuthorized`,
`ExportRateLimited`, `GrantNotFound`, `GrantAlreadyExists`,
`GrantExpired`, `AuditLimitReached`, `InvalidPolicy`,
`AgreementNotFound`, `AgreementAlreadyRegistered`,
`AmendmentVersionNotIncreasing`, `AmendmentLimitReached`,
`DuplicateApproval`, `SessionSummaryLimitReached`, `InvalidAmount`.

## Dependencies and rollout

Depends on the agreements projection (MENTOR-045/046/047) and session
summaries (MENTOR-065); the registrar role moves to the canonical
agreements contract when it ships. Configure the export policy before
enabling the UI. The matching MENTOR-052 work (export UI, document
rendering, download expiry UX, audit view) lives in the other ChainVerse
repositories.
