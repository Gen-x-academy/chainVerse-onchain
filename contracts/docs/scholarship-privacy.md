# Scholarship Privacy: Correction and Erasure Requests (#1133)

- **Status**: Implemented
- **Owner**: Scholarships On-chain Working Group
- **Related issues**: #1133 (Handle correction and erasure requests), #1074 (duplicate prevention), #1075 (atomic submission), #1073 (consent)

---

## 1. Overview & Objectives

In compliance with data protection principles and applicant privacy rights, applicants require standard mechanisms to request:
1. **Data Correction**: Updating inaccurate off-chain application content commitments with a revised integrity hash.
2. **Data Erasure**: Redacting personal identifying content commitments (right to be forgotten) while lawfully retaining immutable financial and audit trails for disbursement compliance.

This contract implements the on-chain Soroban mechanisms to handle these requests within a strictly privacy-minimized architecture.

---

## 2. On-Chain Data Model & Storage Invariants

All personal information lives off-chain. On-chain state maintains only commitments (`BytesN<32>`), request IDs, timestamps, and status markers.

### Types & Enums
- **`PrivacyRequestType`**:
  - `Correction`: Proposes an updated off-chain answers commitment hash (`target_data_hash`).
  - `Erasure`: Requests tombstoning/redaction of the application's answers commitment.
- **`PrivacyRequestStatus`**:
  - `Pending`: Awaiting review/resolution.
  - `Approved`: Executed and confirmed.
  - `Denied`: Formally rejected with an on-chain commitment to the lawful denial reason.
- **`PrivacyRequest`**:
  - `id: u32`: Monotonic request identifier.
  - `applicant: Address`: Exact applicant address.
  - `program_id: BytesN<32>`: Associated scholarship program identifier.
  - `request_type: PrivacyRequestType`: Correction or Erasure.
  - `requested_at: u64`: Ledger timestamp when request was registered.
  - `target_data_hash: BytesN<32>`: Target commitment hash (corrected answers or zero hash).
  - `status: PrivacyRequestStatus`: Outcome state.
  - `processed_at: u64`: Ledger timestamp when processed.
  - `denial_reason_hash: BytesN<32>`: Commitment to documented lawful retention or policy denial basis.
  - `downstream_addressed: bool`: Proof that indexers and off-chain caches have been signaled.

---

## 3. Workflow & Invariant Guarantees

### A. Request Submission
- `request_correction(applicant, program_id, new_data_hash)`:
  - Requires `applicant.require_auth()`.
  - Enforces that an existing `Application` record exists.
  - Emits `(PRIVACY, REQ_CORR)` event.
- `request_erasure(applicant, program_id)`:
  - Requires `applicant.require_auth()`.
  - Enforces that an existing `Application` record exists.
  - Emits `(PRIVACY, REQ_ERAS)` event.

### B. Request Resolution (Admin Governance)
- `process_privacy_request(admin, request_id, approve, denial_reason_hash, downstream_addressed)`:
  - Requires `admin.require_auth()`.
  - Prevents replay: can only be processed once from `Pending` state (`RequestAlreadyProcessed`).
  - When Approved:
    - **Correction**: Updates `Application.data_hash` with the new commitment while preserving application timestamps, program ID, and consent version history.
    - **Erasure**: Sets `Application.data_hash` to zero hash (`[0u8; 32]`), achieving tombstone redaction of applicant payload commitments while preserving the immutable application ledger entry for financial audits.
  - When Denied:
    - Records `denial_reason_hash` (e.g., regulatory tax hold or active disbursement dispute).
  - Emits `(PRIVACY, RESOLVE)` event signaling downstream indexers.

---

## 4. Downstream Trust Boundary & Operational Impact

- **Downstream Copies**: Front-ends, indexers, and off-chain storage systems subscribe to `(PRIVACY, RESOLVE)` events. When `downstream_addressed = true`, indexers are expected to purge off-chain cache entries or refresh corrected projection records.
- **Audit Preservation**: Financial commitments, award disbursements, and ledger sequences remain immutable. Erasure redacts applicant data commitments without breaking financial reconciliation.
- **TTL & Rent Policy**: All privacy requests are stored in `persistent` storage with standard `RECORD_MIN_TTL` and `RECORD_MAX_TTL` extensions matching the contract's 1-year lifecycle policy.
