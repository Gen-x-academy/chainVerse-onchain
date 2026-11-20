# Capstone Supervisor Directory Search Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-030] Create supervisor directory search
- Related issues: GitHub issue #1378 — Discovery track, shared work item CAPSTONE-030
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (search index, filter evaluation, availability aggregation), chainVerse-frontend (directory UI, filter controls, supervisor profile cards)

## Scope

This contract (`contracts/capstone-supervisor-directory-search`) implements
the on-chain primitive required by **CAPSTONE-030 — Create supervisor directory
search**.

The goal is to let students find eligible supervisors by expertise, methods,
language, availability, capacity, affiliation, delivery mode, and verified
indicators, without exposing private or sensitive supervisor data to the ledger.

**What the contract stores on-chain:**

- A versioned supervisor registry entry per `supervisor` address containing: a
  `profile_hash` (`BytesN<32>`) commitment over the full off-chain supervisor
  profile document, a `filters_hash` (`BytesN<32>`) commitment over the
  publicly searchable filter attributes (expertise tags, languages, delivery
  modes, affiliation), a `verified` boolean indicating whether the supervisor
  holds a valid on-chain attestation, an `available` boolean reflecting
  current open-to-supervision status, a `capacity` counter (`u8`, max 255)
  representing remaining student slots, and the registered-at ledger
  timestamp.
- A registry-owner record (admin address) that controls entry lifecycle and
  attestation gates.
- An attestation index per `(supervisor, attestation_type)` mapping to a
  `BytesN<32>` attestation reference, used to verify expertise or affiliation
  claims without storing the full credential.

**What stays off-chain:**

- Full supervisor profiles: biography, contact details, CV, referee names, and
  research interests narrative. Only the `profile_hash` commitment is stored.
- Filter attribute values (specific expertise keywords, language codes,
  delivery-mode strings): the `filters_hash` commits to a canonical
  serialization of these attributes held by the search index service.
- Identity, institutional, and financial data.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `register_supervisor(supervisor, profile_hash, filters_hash)` | admin | Registers a new supervisor entry. Idempotent: a second call with the same address returns `SupervisorAlreadyRegistered`. |
| `update_profile(supervisor, profile_hash, filters_hash)` | supervisor | Updates the profile and filter commitments. Requires the supervisor's own auth. |
| `set_availability(supervisor, available, capacity)` | supervisor | Sets open-to-supervision flag and remaining capacity. |
| `set_verified(supervisor, attestation_type, attestation_ref)` | admin | Records an attestation reference and sets `verified = true` for the supervisor. |
| `revoke_verified(supervisor)` | admin | Clears the `verified` flag and removes attestation references. |
| `deregister_supervisor(supervisor)` | admin | Removes the supervisor entry. |
| `get_supervisor(supervisor)` | any | Returns the `SupervisorEntry` or `None`. |
| `is_available(supervisor)` | any | Returns `(available, capacity)` for a quick availability check. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `SupervisorAlreadyRegistered` | 1 | A registry entry already exists for this address. |
| `SupervisorNotFound` | 2 | No entry exists for this address. |
| `Unauthorized` | 3 | Caller is not the registered admin or the supervisor themselves. |
| `InvalidProfileHash` | 4 | `profile_hash` is the zero hash. |
| `InvalidFiltersHash` | 5 | `filters_hash` is the zero hash. |
| `CapacityOverflow` | 6 | Supplied `capacity` exceeds `u8::MAX`. |
| `InvalidAttestationRef` | 7 | `attestation_ref` is the zero hash. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPSVDREG` | `(supervisor, verified)` | Supervisor registered. |
| `CAPSVDUPD` | `(supervisor, profile_hash)` | Profile or filter commitment updated. |
| `CAPSVDAVL` | `(supervisor, available, capacity)` | Availability or capacity changed. |
| `CAPSVDVFY` | `(supervisor, attestation_type)` | Supervisor marked verified. |
| `CAPSVDRVK` | `(supervisor)` | Verification revoked. |
| `CAPSVDDRG` | `(supervisor)` | Supervisor deregistered. |

### Storage layout

```
Persistent:
  Admin                                                          → Address
  Entry(supervisor: Address)                                     → SupervisorEntry
  Attestation(supervisor: Address, attestation_type: Symbol)     → BytesN<32>
```

`SupervisorEntry` fields:
- `supervisor: Address`
- `profile_hash: BytesN<32>` — commitment to the full off-chain profile document
- `filters_hash: BytesN<32>` — commitment to the searchable filter attribute set
- `verified: bool`
- `available: bool`
- `capacity: u8`
- `registered_at: u64`
- `updated_at: u64`

TTL: persistent, bumped to `[3_110_400, 6_220_800]` ledger ticks on every
write. The off-chain search index service must periodically extend entries it
expects to keep alive.

## Privacy

The contract never stores supervisor identity, contact details, or profile
content. `profile_hash` and `filters_hash` are caller-supplied `BytesN<32>`
commitments whose preimages remain exclusively off-chain.

Attestation references stored under `Attestation(supervisor, attestation_type)`
are opaque hashes; the full attestation credential stays in the issuing
attestation contract or off-chain credential store.

Events carry only the supervisor address and commitment hashes — no personal
data, keywords, or profile text.

## Authorization

- `register_supervisor` and admin-only entry points (`set_verified`,
  `revoke_verified`, `deregister_supervisor`) call `admin.require_auth()` and
  verify the caller is the stored admin address.
- `update_profile` and `set_availability` call `supervisor.require_auth()` and
  verify the entry's `supervisor` field matches the caller.
- Authorization is deny-by-default: no mutating entry point has an open-access
  path.

## Idempotency and replay resistance

`register_supervisor` is idempotent for the same address: a second call
returns `SupervisorAlreadyRegistered` without modifying state. First
registration writes the entry exactly once.

`update_profile` and `set_availability` are last-writer-wins within the same
ledger sequence; there is no token-based optimistic locking because concurrent
supervisors updating their own distinct entries cannot conflict. A supervisor
updating their own entry concurrently from two sessions will have one
transaction accepted; the other will fail or be sequenced — the resulting
state is deterministic.

`set_verified` and `revoke_verified` are idempotent: setting verification when
already verified, or revoking when already unverified, produces no state change
and no error.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| Profile content | `profile_hash` commitment | Storage, full-text search indexing, rendering |
| Filter attributes | `filters_hash` commitment | Filter index construction, query evaluation |
| Availability aggregation | `available` and `capacity` flags | Real-time aggregation, capacity reservation workflow |
| Attestation credential | `attestation_ref` hash | Credential issuance, verification, revocation |
| Search ranking | — | Off-chain ranking algorithm, relevance scoring |
| Pagination | — | Off-chain cursor-based pagination over indexed entries |

## Dependencies

- **capstone-identifiers**: supervisor addresses should be validated against
  the platform's public identifier registry before registration.
- **Attestation contract** (scholarship-eligibility or equivalent): `set_verified`
  records a reference to an attestation issued by this contract; the off-chain
  admin service must verify the attestation is live before calling `set_verified`.
- **CAPSTONE-031** (research theme subscriptions): the subscription service
  reads `filters_hash`-committed attributes to match newly registered or
  updated supervisors against saved searches.
- **CAPSTONE-033** (capstone proposal drafts): the proposal draft service may
  surface matched supervisors from this registry to authors during drafting.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: `register_supervisor` rejected for non-admin;
  `update_profile` rejected for non-owner supervisor; `set_verified` rejected
  for non-admin; cross-supervisor mutation (supervisor A updating supervisor B's
  entry) rejected with `Unauthorized`.
- **Replay resistance**: double `register_supervisor` same address →
  `SupervisorAlreadyRegistered`; no state mutation on second call.
- **State transitions**: register → update profile → set available → set
  verified → revoke verified → deregister; assert state at each step.
- **Idempotency**: `set_verified` twice → second call is a no-op; `revoke_verified`
  twice → second call is a no-op.
- **Availability bounds**: `capacity = 255` (max u8) → succeeds; capacity set
  back to 0 → `available` semantics still correct.
- **Storage bounds**: single entry per address — confirm no unbounded collection
  growth.
- **Zero hash guard**: `profile_hash = [0u8; 32]` → `InvalidProfileHash`;
  `filters_hash = [0u8; 32]` → `InvalidFiltersHash`.
- **Event schemas**: assert topic symbol and payload types for all six events.
- **Error codes**: every `ContractError` variant has at least one test that
  triggers it.
- **Resource budget**: smoke-test `register_supervisor` and `update_profile`
  within default Soroban instruction and memory limits.
- **Upgrade compatibility**: `SupervisorEntry` additions must not break reads
  of earlier WASM records.

## Rollout considerations

1. Deploy after `capstone-identifiers` is live and the admin address is known.
2. Register supervisors via `register_supervisor` before enabling the directory
   UI; the search index must be seeded from initial `CAPSVDREG` events.
3. Run `set_verified` for supervisors who have live attestations before the
   directory goes public, to avoid showing unverified entries as verified.
4. Configure the off-chain search index to subscribe to `CAPSVDUPD`,
   `CAPSVDAVL`, `CAPSVDVFY`, and `CAPSVDRVK` events to keep the index current.
5. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Rolling back removes all entry points; coordinate the admin service and
  search index to pause writes during rollback.
- Persistent entries survive WASM rollback if the `SupervisorEntry` XDR schema
  is unchanged between versions.
- Attestation index entries are independent persistent keys and survive WASM
  rollback independently.

## Corresponding CAPSTONE-030 work in other repositories

- **chainVerse-backend**: search index service — builds and maintains a
  queryable filter index from `filters_hash` preimages, evaluates multi-facet
  filter queries (expertise, language, delivery mode, availability, verified
  status), handles pagination, computes `profile_hash` and `filters_hash` on
  supervisor profile save, subscribes to contract events to keep the index
  current.
- **chainVerse-frontend**: directory UI — renders filter controls for expertise,
  methods, language, availability, capacity, affiliation, and delivery mode;
  displays supervisor profile cards with verified badge; handles empty-state
  and loading experiences.
