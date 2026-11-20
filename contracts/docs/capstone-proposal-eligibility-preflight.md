# Capstone Proposal Eligibility Preflight Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-037] Run proposal eligibility preflight
- Related issues: GitHub issue #1385 — Proposals track, shared work item CAPSTONE-037
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (rule evaluation service, remediation guidance), chainVerse-frontend (preflight results UI, remediation prompts)

## Scope

This contract (`contracts/capstone-proposal-eligibility-preflight`) implements
the on-chain primitive required by **CAPSTONE-037 — Run proposal eligibility
preflight**.

The goal is to evaluate current call rules before submission and return a
deterministic `Pass`, `Fail`, or `NeedsInformation` result with safe,
actionable explanations and remediation.

**What the contract stores on-chain:**

- A versioned preflight rule record per `(call_id, version)` containing: a
  `rules_hash` (`BytesN<32>`) commitment over the full off-chain rule
  document, a list of up to `MAX_RULE_REFS` (16) `BytesN<32>` attestation
  type references that must be satisfied for `Pass`, a list of up to
  `MAX_REMEDIATION_REFS` (16) `BytesN<32>` commitments to off-chain
  remediation guidance documents, and the published-by address and timestamp.
- A cached preflight result record per `(call_id, subject)` containing: the
  `PreflightOutcome` (`Pass | Fail | NeedsInformation`), the rule version it
  was evaluated against, a `result_hash` commitment to the full off-chain
  evaluation report, and the evaluation timestamp. Results use a short TTL
  (~24 h).
- An owner record per `call_id` for rule publication authorization.

**What stays off-chain:**

- Rule logic, threshold values, prerequisite checks, and eligibility
  predicates. The `rules_hash` is a commitment over the full rule document.
- Explanation text for each pass, fail, or needs-information finding.
- Remediation guidance content. Only commitments to guidance documents are stored.
- Subject identity, qualifications, transcript, or financial data.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `publish_rules(authority, call_id, version, rules_hash, attestation_refs, remediation_refs)` | call owner | Publishes a versioned preflight rule set. Version must be strictly greater than current latest. |
| `record_result(evaluator, call_id, subject, rule_version, outcome, result_hash)` | authorized evaluator | Records a preflight evaluation result for a subject against a specific rule version. `rule_version` must match the current latest. |
| `get_result(call_id, subject)` | any | Returns the latest cached `PreflightResult` or `None`. |
| `get_rules(call_id, version)` | any | Returns `PublishedRules` for the given version or `None`. |
| `latest_rule_version(call_id)` | any | Returns the current latest rule version or `None`. |
| `invalidate_result(authority, call_id, subject)` | call owner | Removes a cached result, forcing re-evaluation. No-op if no result exists. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `InvalidVersion` | 1 | `version` is 0 or not strictly greater than the current latest. |
| `VersionAlreadyPublished` | 2 | A rule record already exists at this `(call_id, version)`. |
| `UnauthorizedOwner` | 3 | Caller is not the registered owner of this call. |
| `CallNotFound` | 4 | No rules have been published for this `call_id`. |
| `RuleVersionMismatch` | 5 | `record_result` supplied a `rule_version` that does not match the current latest. |
| `Unauthorized` | 6 | Caller is not the registered evaluator or call owner. |
| `InvalidRulesHash` | 7 | `rules_hash` is the zero hash. |
| `TooManyRefs` | 8 | `attestation_refs` or `remediation_refs` exceeds the bounded limit. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPPFRUL` | `(call_id, version, authority)` | Preflight rules published. |
| `CAPPFRES` | `(call_id, subject_commitment, outcome, rule_version)` | Preflight result recorded. `subject_commitment` is `sha256(subject_address)` — not the raw address. |
| `CAPPFINV` | `(call_id, authority)` | Cached result invalidated by call owner. |

### Storage layout

```
Persistent:
  Owner(call_id: BytesN<32>)                    → Address
  Rules(call_id: BytesN<32>, ver: u32)           → PublishedRules
  Latest(call_id: BytesN<32>)                    → u32
  Evaluator(evaluator: Address)                  → bool
  Admin                                          → Address

Temporary (TTL: 17_280 ledger ticks ≈ 24 h):
  Result(call_id: BytesN<32>, subject: Address)  → PreflightResult
```

`PublishedRules` fields:
- `call_id: BytesN<32>`
- `version: u32`
- `published_by: Address`
- `published_at: u64`
- `rules_hash: BytesN<32>`
- `attestation_refs: Vec<BytesN<32>>` — up to 16 attestation type references
- `remediation_refs: Vec<BytesN<32>>` — up to 16 remediation guidance commitments

`PreflightResult` fields:
- `call_id: BytesN<32>`
- `subject: Address`
- `outcome: PreflightOutcome` — `Pass | Fail | NeedsInformation`
- `rule_version: u32`
- `result_hash: BytesN<32>` — commitment to the full off-chain evaluation report
- `evaluated_at: u64`

Persistent rule records: TTL `[3_110_400, 6_220_800]`. Result records: short
temporary TTL `[17_280, 17_280]` (~24 h).

## Privacy

The contract never stores subject identity, qualification data, or evaluation
detail. The `CAPPFRES` event emits `sha256(subject_address)` rather than the
raw address to prevent public indexers from trivially enumerating which
subjects ran preflight against which calls.

Explanation text, remediation guidance, and pass/fail reasoning remain
off-chain. Only commitments to those documents appear on-chain.

## Authorization

- `publish_rules` calls `authority.require_auth()`. First publication for a
  `call_id` registers the caller as owner; subsequent versions require the
  same owner.
- `record_result` calls `evaluator.require_auth()` and verifies the evaluator
  is on the admin-managed allowlist.
- `invalidate_result` calls `authority.require_auth()` and verifies the caller
  is the registered call owner.
- `add_evaluator` / `remove_evaluator` are admin-only.

Authorization is deny-by-default: no mutating entry point has an open-access
path.

## Idempotency and replay resistance

`publish_rules`: each `(call_id, version)` is written exactly once;
`VersionAlreadyPublished` on replay. Strictly monotonic version numbers mean
a replayed old-version transaction always fails.

`record_result`: requires `rule_version` to match the current latest. A
replayed result with a stale `rule_version` returns `RuleVersionMismatch`.
Results are keyed by `(call_id, subject)` and overwrite, so duplicate
evaluation calls within the same rule version are safe — last writer wins.

`invalidate_result` is idempotent: no-op if no cached result exists.

## Outcome semantics

| Outcome | Meaning | Action |
|---|---|---|
| `Pass` | All rules satisfied as of evaluation time. | Subject may proceed to submit. |
| `Fail` | One or more rules not satisfied; missing information cannot change the outcome. | Subject should follow remediation guidance. |
| `NeedsInformation` | One or more rules could not be evaluated due to missing or pending attestations. | Subject should supply the requested information and re-run preflight. |

The off-chain evaluation service determines which outcome applies. The
contract records the stated outcome and a commitment to the full report.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| Rule logic and predicates | `rules_hash` commitment | Rule authoring, evaluation engine |
| Explanation and remediation text | `remediation_refs` commitments | Content storage, localisation, safe rendering |
| Attestation resolution | `attestation_refs` list | Fetching subject attestations from attestation contracts |
| Re-evaluation scheduling | `CAPPFRUL` event | Off-chain indexer triggers re-evaluation when rules change |
| Subject identity | `subject: Address` on `record_result` | Identity verification, authentication |

## Dependencies

- **capstone-identifiers**: `call_id` values should be valid public identifiers.
- **capstone-call-terms**: preflight rules reference the same `call_id` as
  published call terms and question schemas.
- **scholarship-eligibility** or equivalent attestation contract: the
  off-chain evaluation service fetches subject attestations to evaluate
  `attestation_refs`.
- **capstone-proposal-draft-revisions** (CAPSTONE-034): the draft autosave
  service should surface the latest cached preflight result to the author
  during drafting.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: `publish_rules` rejected for non-owner;
  `record_result` rejected for non-evaluator; `invalidate_result` rejected
  for non-owner.
- **Replay resistance**: double `publish_rules` same version →
  `VersionAlreadyPublished`; `record_result` with stale `rule_version` →
  `RuleVersionMismatch`.
- **Strict monotonicity**: versions 1 → 2 → 3; version 2 again → rejected.
- **Outcome coverage**: record `Pass`, `Fail`, and `NeedsInformation`; assert
  each is stored and returned correctly.
- **Result TTL**: assert result records use the short temporary TTL; assert
  rule records use the long persistent TTL.
- **Invalidation**: record a result, invalidate it, assert `get_result` returns
  `None`; assert `invalidate_result` is a no-op when no result exists.
- **Storage bounds**: `attestation_refs` length 17 → `TooManyRefs`;
  `remediation_refs` length 17 → `TooManyRefs`; both at 16 → succeed.
- **Zero hash guard**: `rules_hash = [0u8; 32]` → `InvalidRulesHash`.
- **Event schemas**: assert all three events; specifically assert `CAPPFRES`
  emits `sha256(subject_address)` not the raw address.
- **Error codes**: every `ContractError` variant has at least one test.
- **Resource budget**: smoke-test `publish_rules` and `record_result` within
  default Soroban limits.
- **Upgrade compatibility**: `PublishedRules` and `PreflightResult` additions
  must not break reads of earlier WASM records.

## Rollout considerations

1. Deploy after `capstone-identifiers` is live.
2. Add evaluator addresses (`add_evaluator`) before the off-chain evaluation
   service starts recording results.
3. Call owners must publish rules before any subject can run preflight.
4. Configure the evaluation service to subscribe to `CAPPFRUL` events and
   trigger re-evaluation on rule changes.
5. Align result TTL (~24 h) with the evaluation service's re-evaluation schedule.
6. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Rolling back removes the `record_result` entry point; coordinate the
  evaluation service to stop during rollback.
- Persistent rule records survive WASM rollback if the `PublishedRules` XDR
  schema is unchanged.
- Temporary result records expire within 24 h even if rollback cannot remove
  them explicitly.
- The evaluator allowlist persists through rollback.

## Corresponding CAPSTONE-037 work in other repositories

- **chainVerse-backend**: evaluation service — fetches subject attestations,
  evaluates rule documents, computes `result_hash`, calls `record_result`,
  handles `CAPPFRUL` events to trigger re-evaluation, generates structured
  remediation guidance keyed by `remediation_refs` commitments.
- **chainVerse-frontend**: preflight results UI — displays `Pass / Fail /
  NeedsInformation` badge, renders remediation guidance for failing checks,
  prompts subject to supply missing information, re-runs preflight after
  subject actions.
