# Mentorship Agreement Export Contract

- Status: Implemented (v1)
- Owner: On-chain Mentorship working group
- Related issue: #1248 (MENTOR-052, export participant agreement records)
- Depends on / relates to: the canonical agreements contracts
  (MENTOR-045 accepted terms, MENTOR-046 acceptance/approvals,
  MENTOR-047 amendments — not yet in this repository), whose registrar
  address is intended to project agreement state here; session summaries
  come from MENTOR-065, and this contract's export bundle is the on-chain
  half of MENTOR-100's achievement/record export

## Scope

`contracts/mentorship-agreement-export` provides a privacy-safe,
auditable export of a mentorship agreement record:

- **Only authorized participants can export.** `request_export` and
  `get_export` require the caller to be the agreement's mentor or mentee
  (`require_auth`); grants are personal, so downloading somebody else's
  grant fails with `NotAuthorized`.
- **Generation is rate-limited and audited.** Each participant gets
  `max_exports_per_window` grants per rolling `window_seconds`; the next
  request fails with `ExportRateLimited`. Every successful request and
  download appends to a bounded per-agreement audit log
  (`MAX_AUDIT_ENTRIES`, 96), and a full log fails loudly with
  `AuditLimitReached` rather than silently dropping history.
- **Expired links cannot download.** A grant carries `expires_at`
  (`grant_ttl_seconds`, bounded to 60s–7 days); `get_export` after that
  instant fails with `GrantExpired`. There is no in-place renewal — a
  fresh grant must be requested and counts against the quota again.
- **Private internal notes are excluded by construction.**
  `ExportManifest` has no free-text field, and no entry point accepts
  free text: every piece of content is a hash commitment
  (`terms_hash`, `approval_hash`, `reason_hash`, `summary_hash`), a
  status, a timestamp, or an amount. Notes (MENTOR-064) and messages
  (MENTOR-101) stay off-chain; there is nothing for them to leak into.
- **Safe retries.** Re-requesting with the same grant id and scope
  returns the same grant without consuming quota twice or duplicating the
  audit entry; reusing a grant id under a different scope fails with
  `GrantAlreadyExists`.

## Privacy boundary

On-chain: participant addresses, terms/amendment versions, acceptance
and approval timestamps, commitments, session timing and the financial
summary (currency, gross, fees, net in the smallest unit). Off-chain:
the accepted terms and amendment text, signed acceptance artifacts,
private notes, session summary prose and any document rendering. The
export manifest is assembled on demand from on-chain facts and returned
to the participant; rendering it as a file happens off-chain. The audit
log exposes *that* an export happened, to whom a grant was issued, and
when — never the content.

## Storage layout (instance vs persistent)

- Instance: `Admin`, `Registrar`, `Policy`.
- Persistent (TTL-bounded): `Agreement(agreement_id) ->
  AgreementRecord` (amendments <= 32, approvals <= 32, summaries <=
  64), `Grant(grant_id) -> ExportGrant`,
  `ExportQuota(participant) -> ExportQuota`, `Audit(agreement_id) ->
  Vec<AuditEntry>` (<= 96).

## Ownership and authorization

Deny-by-default:

- Admin (`initialize`): `set_registrar`, `set_policy`, `upgrade`.
- Registrar (defaults to the admin; movable via `set_registrar`):
  `register_agreement`, `record_amendment`,
  `record_session_summary`. When the agreements contracts ship, their
  address takes the role and this record becomes a projection.
- Participants: `record_approval` (own approval only), `request_export`
  (own grant), `get_export`/`get_grant` (own grant), `get_audit` (either
  participant).

## Events and error codes

- `EXPREQ (grant_id)` -> `(agreement_id, requester, expires_at)`
- `EXPDL (grant_id)` -> `(agreement_id, requester, timestamp)`
- `AGREG (agreement_id)`, `APPROVED (agreement_id, approver, version)`,
  `AMENDED (agreement_id, version)`, `SUMMARY (agreement_id,
  session_id)`, `EXPOLICY (max, window, ttl)`, `REGISTRAR (address)`,
  `UPGRADED (wasm_hash)`

Errors: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAdmin=3`,
`NotRegistrar=4`, `NotAuthorized=5`, `AgreementNotFound=6`,
`AgreementAlreadyRegistered=7`, `AmendmentVersionNotIncreasing=8`,
`AmendmentLimitReached=9`, `DuplicateApproval=10`,
`SessionSummaryLimitReached=11`, `InvalidAmount=12`,
`ExportRateLimited=13`, `GrantNotFound=14`, `GrantAlreadyExists=15`,
`GrantExpired=16`, `InvalidPolicy=17`, `AuditLimitReached=18`.

## Upgrade compatibility

`upgrade` is admin-gated and swaps the WASM under the same contract
address; `version()` reports ABI generation 1. Additive changes keep
`version()` and never alter existing entry points or event schemas;
`ExportManifest` fields may only be appended in a coordinated client
release, since the struct is returned whole.

## Dependencies and rollout

Deploy after the agreements projection is available (or bootstrap with
the registrar role held by an operator key). Set the export policy with
`set_policy` before enabling the UI — the defaults used by tests
(2 grants/hour, 15-minute links) are illustrative. Off-chain, the web
app requests a grant, renders the manifest into a signed PDF/JSON, and
delivers it through the existing document store; the indexer projects
`EXPREQ`/`EXPDL` into the mentor/mentee activity feed. The matching
MENTOR-052 work (export UI, rendering, download expiry UX, audit view)
lives in the other ChainVerse repositories.
