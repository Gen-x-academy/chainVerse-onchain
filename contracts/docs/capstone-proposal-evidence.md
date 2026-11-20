# Capstone Proposal Evidence Attachments Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-036] Add proposal evidence attachments
- Related issues: GitHub issue #1384 — Proposals track, shared work item CAPSTONE-036
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (file storage, malware scanning, access policy enforcement), chainVerse-frontend (attachment upload UI, deletion controls)

## Scope

This contract (`contracts/capstone-proposal-evidence`) implements the
on-chain primitive required by **CAPSTONE-036 — Add proposal evidence
attachments**.

The goal is to accept constrained references to evidence files with purpose,
consent, malware-scan state, access policy, retention, integrity metadata, and
deletion status.

**What the contract stores on-chain:**

- An attachment record per `(proposal_id, attachment_id)` containing:
  an integrity commitment (`BytesN<32>` SHA-256 over the off-chain file
  content), a `scan_state` enum (`Pending | Clean | Quarantined`), a
  `purpose` code (`BytesN<4>`, e.g. `b"SUPP"` for supporting evidence),
  an `access_policy` code (`BytesN<4>`, e.g. `b"REVW"` for reviewer-only),
  a `consent_version` reference (`u32`), a `retention_until` ledger
  timestamp, a `deleted` flag, and an owner address.
- A bounded count of live (non-deleted) attachments per `proposal_id` capped
  at `MAX_ATTACHMENTS` (20).
- A `scan_state` that can only be advanced forward: `Pending → Clean` or
  `Pending → Quarantined`.

**What stays off-chain:**

- The file itself: bytes, MIME type, file name, content. Only the SHA-256
  integrity commitment is stored.
- Malware scan results, scan reports, and scanner identity — only the outcome
  (`Clean` or `Quarantined`) is recorded on-chain.
- Access control enforcement: who may retrieve the file from the off-chain
  store. The `access_policy` code is a commitment to an off-chain policy.
- Consent terms text: `consent_version` references a version published by the
  proposal submission or questions contract.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `attach(owner, proposal_id, attachment_id, integrity_hash, purpose, access_policy, consent_version, retention_until)` | proposal author | Registers a new attachment in `Pending` scan state. Fails if `MAX_ATTACHMENTS` live attachments already exist. Idempotent on `attachment_id`. |
| `record_scan_result(scanner, proposal_id, attachment_id, scan_state)` | authorized scanner | Advances `scan_state` from `Pending` to `Clean` or `Quarantined`. Cannot move a `Clean` record back to `Pending`. |
| `delete_attachment(owner, proposal_id, attachment_id)` | proposal author | Sets `deleted = true`; decrements the live count. Does not remove the record. |
| `get_attachment(proposal_id, attachment_id)` | any | Returns `AttachmentRecord` or `None`. |
| `list_attachments(proposal_id)` | any | Returns a `Vec<BytesN<32>>` of all `attachment_id`s, including deleted ones. |
| `live_count(proposal_id)` | any | Returns the count of non-deleted attachments. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `AttachmentNotFound` | 1 | No record exists for `(proposal_id, attachment_id)`. |
| `AttachmentAlreadyExists` | 2 | `attach` called with an `attachment_id` that already has a record. |
| `TooManyAttachments` | 3 | `MAX_ATTACHMENTS` live attachments already registered for this proposal. |
| `InvalidScanTransition` | 4 | Attempted to move `scan_state` to an invalid successor (e.g. `Clean → Pending`). |
| `Unauthorized` | 5 | Caller is not the attachment owner or authorized scanner. |
| `AttachmentDeleted` | 6 | Mutating call attempted on a `deleted` record. |
| `InvalidIntegrityHash` | 7 | `integrity_hash` is the zero hash. |
| `RetentionInPast` | 8 | `retention_until` is earlier than the current ledger timestamp. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPEVATT` | `(proposal_id, attachment_id, owner, purpose)` | Attachment registered. |
| `CAPEVSCN` | `(proposal_id, attachment_id, scan_state)` | Scan result recorded. |
| `CAPEVDEL` | `(proposal_id, attachment_id, owner)` | Attachment deleted. |

### Storage layout

```
Persistent:
  Attachment(proposal_id: BytesN<32>, attachment_id: BytesN<32>)  → AttachmentRecord
  Scanner(scanner: Address)                                        → bool
  LiveCount(proposal_id: BytesN<32>)                              → u32
  Admin                                                            → Address
```

`AttachmentRecord` fields:
- `proposal_id: BytesN<32>`
- `attachment_id: BytesN<32>`
- `owner: Address`
- `integrity_hash: BytesN<32>`
- `purpose: BytesN<4>`
- `access_policy: BytesN<4>`
- `consent_version: u32`
- `retention_until: u64`
- `scan_state: ScanState` — `Pending | Clean | Quarantined`
- `deleted: bool`
- `registered_at: u64`
- `last_updated_at: u64`

TTL: persistent entries bumped to `[3_110_400, 6_220_800]` ledger ticks on every write.

## Privacy

The contract never stores file content, file names, MIME types, scan reports,
or access control membership lists. `integrity_hash` is a SHA-256 commitment
over the off-chain file. `purpose` and `access_policy` are opaque four-byte
codes whose meaning is defined off-chain.

Deleted records are soft-deleted: the record and its integrity hash remain
visible on-chain (for audit) but the `deleted` flag signals that the
off-chain file may have been removed per retention policy.

## Authorization

- `attach` calls `owner.require_auth()` and verifies the submitted `owner`
  matches the calling address.
- `record_scan_result` calls `scanner.require_auth()` and verifies the scanner
  is on the admin-managed allowlist.
- `delete_attachment` calls `owner.require_auth()` and verifies the stored
  `owner` matches the caller.
- `initialize` sets the admin; `add_scanner` / `remove_scanner` are admin-only.

Authorization is deny-by-default: none of the mutating entry points have an
open-access path.

## Idempotency and replay resistance

`attach` is idempotent on `attachment_id`: a second transaction with the same
`(proposal_id, attachment_id)` returns `AttachmentAlreadyExists` without
modifying state.

`record_scan_result` checks the current `scan_state` before writing. A
replayed `Clean` transaction on an already-`Clean` record returns
`InvalidScanTransition`.

`delete_attachment` on an already-deleted record returns `AttachmentDeleted`
without modifying state or count.

## Scan state machine

```
Pending  ──►  Clean
   │
   └────────► Quarantined
```

Both transitions are final. If a re-scan is needed, the owner must delete the
existing record and re-attach.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| File bytes | `integrity_hash` commitment | Storage, encryption at rest, CDN delivery |
| Malware scanning | `scan_state` outcome | Scanner execution, report storage |
| Access policy enforcement | `access_policy` code | Policy definition, retrieval authorization, access logs |
| Retention enforcement | `retention_until` timestamp | Scheduled deletion of off-chain file after retention expires |
| Consent terms | `consent_version` reference | Terms text, version history, re-consent flows |
| MIME / size validation | — | MIME sniffing, file size limits, type allowlist |

## Dependencies

- **capstone-identifiers**: `proposal_id` values should be valid public identifiers.
- **capstone-proposal-questions** (CAPSTONE-035): `consent_version` references
  a consent version published by the questions or submission contract.
- **capstone-proposal-draft-revisions** (CAPSTONE-034): attachment
  `attachment_id` values may appear as file references within the off-chain
  draft payload. The autosave service must verify that referenced attachments
  are `Clean` before submitting a draft.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: `attach` rejected for non-owner; `record_scan_result`
  rejected for non-scanner; `delete_attachment` rejected for non-owner.
- **Replay resistance**: double `attach` → `AttachmentAlreadyExists`; double
  `delete_attachment` → `AttachmentDeleted`.
- **State transitions**: `Pending → Clean` succeeds; `Pending → Quarantined`
  succeeds; `Clean → Pending` → `InvalidScanTransition`; `Quarantined → Clean`
  → `InvalidScanTransition`.
- **Storage bounds**: 20 attachments → succeed; 21st → `TooManyAttachments`;
  deleting one → allows a 21st.
- **Zero hash guard**: `integrity_hash = [0u8; 32]` → `InvalidIntegrityHash`.
- **Retention guard**: `retention_until` in the past → `RetentionInPast`.
- **Event schemas**: assert topic and payload types for all three events.
- **TTL extension**: assert TTL bump on every write.
- **Error codes**: every `ContractError` variant has at least one test.
- **Resource budget**: smoke-test `attach` and `list_attachments` within
  default Soroban limits at the maximum record count.
- **Upgrade compatibility**: `AttachmentRecord` additions must not break reads
  of records written by earlier WASM versions.

## Rollout considerations

1. Deploy after `capstone-identifiers` is live.
2. Add scanner addresses (`add_scanner`) before any proposal submission flow
   that requires a `Clean` scan state.
3. Configure the off-chain malware scanning service to call
   `record_scan_result` after each scan completes.
4. Agree on `purpose` and `access_policy` four-byte codes before deployment.
5. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Soft-deleted records persist through a rollback; the `deleted` flag and live
  count remain consistent as long as the WASM interprets them the same way.
- If a rollback removes the `record_scan_result` entry point, attachments
  remain in `Pending` state indefinitely; coordinate the scanner service.
- The scanner allowlist is unaffected by WASM rollback.

## Corresponding CAPSTONE-036 work in other repositories

- **chainVerse-backend**: file storage service — upload endpoint, MIME and
  size validation, encryption at rest, malware scanner integration,
  `record_scan_result` call after scan, access policy enforcement, retention
  scheduler, `integrity_hash` computation (file bytes → SHA-256).
- **chainVerse-frontend**: attachment UI — upload progress, scan state display,
  deletion controls, access policy selector, consent acknowledgement at upload
  time.
