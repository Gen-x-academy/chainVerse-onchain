# CAPSTONE-111: Capstone Abuse Cases & On-Chain Threat Model

## 1. Scope & Context
- **Work Item:** CAPSTONE-111 (Privacy Track)
- **Repository:** `Gen-x-academy/chainVerse-onchain`
- **Audit Baseline:** `main` @ `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb` (2026-11-18)

This document defines the threat model, trust boundaries, abuse scenarios, and on-chain/off-chain mitigations for the capstone and research-project workspace.

---

## 2. Protected Assets
1. **Escrow & Milestone Funding Pools:** Token balances locked for student/researcher capstone grants and milestone disbursements.
2. **Research Artifacts & Proposal Commitments:** Bounded cryptographic hashes (`BytesN<32>`) attesting authorship, timestamps, and submission integrity without exposing raw intellectual property.
3. **Participant & Minor Identity Privacy:** Off-chain identity records, minor guardian approvals, and safety flags kept strictly off public ledgers.
4. **Governance & Attestation Authority:** Tenant-scoped admin, reviewer, and advisor keys controlling state transitions and milestone approvals.

---

## 3. Trust Boundaries
- **Boundary A — Off-Chain PII / Content Storage vs. Soroban Ledger:** Raw proposals, datasets, research manuscripts, minor consent forms, and financial invoices stay off-chain. Only bounded hashes, state enums, amounts, and expiry ledgers cross into Soroban storage.
- **Boundary B — Tenant Isolation:** Multi-institution/tenant records are strictly partitioned by `tenant_id` (`BytesN<32>`). Cross-tenant reads or mutations are denied by default.
- **Boundary C — External Integrations & Indexers:** Off-chain indexers and webhook relays consume standardized contract events without privileged write keys.

---

## 4. Abuse Cases & Mitigations

| Abuse Category | Threat / Attack Vector | On-Chain & Architectural Mitigation |
| :--- | :--- | :--- |
| **Data Exfiltration** | Scraping public ledger state or events to harvest student identities, minor status, or unpublished research. | Store only bounded 32-byte commitment hashes and opaque IDs on-chain; enforce small-cohort suppression thresholds on aggregate views. |
| **Plagiarism & Attribution Theft** | Front-running or re-submitting another team's proposal hash to claim priority or milestone rewards. | Bind commitment hashes to authenticated submitter signatures (`require_auth`), tenant scope, and monotonic ledger sequence timestamps. |
| **Insider Risk & Collusion** | Rogue reviewer or coordinator self-approving milestones or altering historical records. | Enforce separation of duties (submitter cannot approve own milestone), multi-party governance thresholds, and append-only audit events. |
| **Funding Fraud & Double-Claiming** | Replaying payout intents or concurrent retries draining escrow balances multiple times. | Enforce idempotency nonces (`ExecutedIntent`), strict state-machine transitions (`Funded -> Claimed`), and explicit expiry windows. |
| **Minor & Vulnerable Participant Safety** | Unauthorized exposure of minor participants or unverified guardian consent in public metadata. | Keep minor flags and guardian PII off-chain; require bounded cryptographic attestation hashes of off-chain guardian consent before activation. |
| **Integration & Replay Exploits** | Malicious external callers invoking unverified contract addresses or replaying stale cross-contract calls. | Resolve contracts exclusively through the canonical registry with version/capability checks and domain-separated nonces. |
| **Storage Exhaustion / DoS** | Unbounded vector growth or expired entries causing ledger state bloat or zombie locks. | Bound all history collections (`MAX_HISTORY_ENTRIES`), enforce explicit TTL bumps (`extend_ttl`), and reject expired state transitions. |

---

## 5. Rollout, Rollback & Cross-Repository Dependencies
- **Cross-Repo Alignment:** Pairs with off-chain encryption, PII redaction, and minor-consent verification services in the backend and frontend repositories for `CAPSTONE-111`.
- **Rollout:** Deploy behind tenant-scoped feature flags; verify deterministic contract tests for authorization trees, replay resistance, and storage bounds prior to mainnet promotion.
- **Rollback:** Pause new state transitions via tenant governance emergency freeze while preserving read-only audit and refund withdrawal paths.