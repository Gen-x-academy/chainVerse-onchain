# CAPSTONE-071: Integrate Source-Control References

**Track:** Workspace | **Component:** on-chain | **Issue:** #1419 | **Status:** implemented
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree.
**Contract:** `contracts/capstone-source-refs`

## Scope

Register revocable provider connections, branch/commit/release commitments, and
monotonic webhook delivery receipts for a project. Repository names, branch
names, URLs, tokens, webhook payloads, and contribution text remain off-chain.
The chain records only provider enums, reference-kind enums, hashes, scope bit
masks, delivery sequence numbers, and state.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin`, `Role(tenant_id, account, role)`, `RoleCount(tenant_id)` | Deployment admin and tenant-scoped `Maintainer`, `Integrator`, `Viewer` roles (32 grants per tenant). |
| `Version(tenant_id, project_id)`, `Request(...)` | Optimistic version and idempotency record for every mutation. |
| `Connection(tenant_id, project_id, connection_id)` | Provider, credential commitment, scope mask, active/revoked state, creation time, version. |
| `ConnectionIndex(...)` | Connection ids, capped at 16. |
| `Ref(tenant_id, project_id, ref_id)` | Connection id, provider, ref kind, ref commitment, optional proof commitment, proposed/verified state, creation time, version. |
| `RefIndex(...)` | Reference ids, capped at 64. |
| `Hook(tenant_id, project_id, hook_id)` | Provider, hook commitment, last delivery sequence, state, creation time, version. |
| `HookIndex(...)` | Hook ids, capped at 16. |

Off-chain: OAuth tokens and refresh tokens, provider account names, repository
URLs, branch and tag names, commit messages, diffs, contributor identities,
raw webhook bodies, and signature verification details. `credential_hash`,
`ref_hash`, `proof_hash`, and `hook_hash` must be salted commitments produced by
the source-control service.

## Authorization

`initialize` authenticates the admin. The admin grants tenant-scoped roles;
every mutating call authenticates the caller and checks the stored role.
`Maintainer` connects/revokes providers and registers references;
`Integrator` verifies references and records webhook deliveries; reads accept
either role plus `Viewer` or the admin. Provider connections cannot be used
across tenants or projects because all keys include both ids.

## Idempotency

`request_id` plus `expected_version` is stored with a domain-separated input
commitment. Replays return the recorded version and emit no event. A different
payload for the same request gets `RequestConflict`; a new request with a stale
`expected_version` gets `VersionConflict`. Webhooks add a second guard:
`delivery_seq` must be strictly greater than the stored sequence, so a
redelivered provider event with a fresh request cannot advance state twice.
All counters and sequence checks use checked arithmetic. Administrative role
grants/revokes are boolean writes on deterministic keys and are naturally
idempotent, so they do not consume a `request_id`.

## Errors

Append-only `ContractError` codes: `1 NotInitialized`, `2 AlreadyInitialized`,
`3 Unauthorized`, `4 VersionConflict`, `5 VersionOverflow`, `6 RequestConflict`,
`7 NotFound`, `8 AlreadyExists`, `9 InvalidInput`, `10 InvalidState`,
`11 ConnectionRevoked`, `12 ProviderMismatch`, `13 DeliveryReplay`,
`14 CollectionFull`, `15 InvalidPage`.

## Events

`SRINIT(admin)`, `ROLEGR(tenant_id, account, role)`,
`ROLERV(tenant_id, account, role)`, `SRCONN(tenant_id, project_id,
connection_id, provider, scopes_mask)`, `SRRVK(tenant_id, project_id,
connection_id, state)`, `SRREF(tenant_id, project_id, ref_id, provider, kind)`,
`SRVRFY(tenant_id, project_id, ref_id, state)`, and
`SRHOOK(tenant_id, project_id, hook_id, delivery_seq)`. Payloads are ids, enums,
masks, and counters only; no repository or branch text is emitted.

## Privacy boundaries

Private repositories and contributor identity are never encoded in the chain.
Scope masks are bit flags, not permission descriptions. A revoked connection
blocks new references and verifications but does not erase prior commitments;
erasure is handled by dropping the off-chain salt and documents. Persistent
records are TTL-extended on reads and writes; the indexer maintains long-term
projections.

## Dependencies

Depends on `capstone-identifiers` and CAPSTONE-038 for project identity, the
off-chain provider connector for OAuth/webhook handling, and CAPSTONE-072 for
feed entries derived from reference events. Corresponding frontend work owns
the connector setup UI and webhook status display.

## Rollout / rollback

Deploy behind the workspace flag, initialize, grant integrator/maintainer
roles, register connections, then enable reference linking. Existing
repositories are backfilled as `Ref` records with fresh commitments and an
off-chain migration marker; no raw repository names go on-chain. Rollback:
revoke connections and stop new registrations while keeping reads available;
storage layouts and error codes are append-compatible. Contract removal is a
data-retention decision and requires the same review as credential deletion.

## Corresponding work in other repositories (CAPSTONE-071)

- **Backend/source-connector:** OAuth storage, token refresh, repository
  enumeration, salted commitment generation, webhook signature checks.
- **Frontend:** connect/revoke provider screens, commit/branch/release pickers,
  verification status, webhook health view.
- **Mobile:** read-only reference status and reconnection prompts.
- **Docs/indexer:** event projection schema, salt management, deletion runbook.
