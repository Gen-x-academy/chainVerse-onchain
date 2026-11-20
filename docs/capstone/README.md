# Capstone On-chain Specs: Governance Track

| Item | Issue | Doc | Contract (planned) |
| --- | --- | --- | --- |
| CAPSTONE-050 | #1398 | [Record charter acceptance](CAPSTONE-050-charter-acceptance.md) | `capstone-charters` |
| CAPSTONE-051 | #1399 | [Guardian consent](CAPSTONE-051-guardian-consent.md) | `capstone-guardian-consent` |
| CAPSTONE-052 | #1400 | [Amend active charters](CAPSTONE-052-charter-amendments.md) | `capstone-charters` |
| CAPSTONE-053 | #1401 | [Pause and resume projects](CAPSTONE-053-pause-resume.md) | `capstone-pauses` |

Order of implementation: 050 → 052 → 051 → 053. Each later item depends on the
state and checks introduced by the earlier ones.

Rules shared by all four specs:
- The chain stores only hashes, opaque references, addresses, enums, counters
  and timestamps. Charter text, reasons, identity evidence and amounts stay in
  the backend.
- Authorization is deny-by-default and scoped to a `tenant_id`.
- Every state-changing call that a signer authorizes includes a single-use
  nonce. Retries return a stable "already done" error, so they never repeat a
  side effect.
- Error codes are append-only and existing codes are never renumbered.
