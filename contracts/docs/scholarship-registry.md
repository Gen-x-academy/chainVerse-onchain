# Scholarship Registry Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1057 (feature module and route hierarchy), #1056
  (platform boundaries and invariants — see
  `docs/adr/0002-scholarships-on-chain-boundaries.md`)

## Scope

This contract (`contracts/scholarship-registry`) implements #1057 as two
small, composable registries rather than an HTTP-style router (Soroban
contracts don't have routes in that sense):

- **Module registry.** `register_module()`/`get_module()` let an admin
  record and anyone resolve the current deployed address for a named
  scholarship module (e.g. `"core"`, `"programs"`, `"eligibility"`,
  `"applications"`). "The feature is registered" means: every scholarship
  contract has a discoverable, canonical name-to-address mapping here,
  instead of callers hardcoding addresses. Unlike the versioned records in
  the sibling contracts, a module registration is mutable in place — this
  registry tracks "what's current," not deployment history.
- **Role registry.** `grant_role()`/`revoke_role()`/`has_role()` implement
  the five actors named in the epic (`Student`, `Sponsor`, `Reviewer`,
  `Finance`, `Administrator`) as a flat, admin-managed allowlist per
  `(account, role)` pair. `require_role()` gives every caller — this
  contract's own future functions, another contract, or an off-chain
  service — a single, consistent `Unauthorized` error instead of each
  reinventing its own authorization-failure shape.

This directly answers #1057's acceptance criteria: the feature is
registered (module registry), role-scoped routes are reachable (a caller
resolves a module's address, checks `has_role`/`require_role`, then calls
the module directly), and unauthorized actors receive a consistent error
(`Unauthorized`, always the same discriminant, regardless of which role or
module was being checked).

See `docs/adr/0002-scholarships-on-chain-boundaries.md` for #1056's
architecture record — actors, state machines, invariants, failure modes,
privacy boundaries, and non-goals across the whole Scholarships On-chain
epic, not just this contract.

**Not implemented here:** this contract does not proxy, wrap, or forward
calls to the modules it registers, and none of the other four scholarship
contracts currently call into this one to check a role before acting —
that cross-contract enforcement is documented as follow-up work in the
ADR's Failure modes section.

## Privacy

This contract stores no applicant data — only contract addresses, role
grants keyed by `Address`, and role enum values. It has the same shape of
minimal footprint as the sibling contracts' allowlists (e.g.
`scholarship-eligibility`'s issuer allowlist).

## Ownership

A single admin (`initialize`) exclusively controls both registries. There
is no delegated "who can grant roles" beyond that admin in this pass —
consistent with #1059/#1060 (sponsor-org roles) not existing yet.

## Migration

New contract, no prior on-chain state. Recommended setup order for anyone
deploying the full epic: deploy `scholarship-core`,
`scholarship-programs`, `scholarship-eligibility`,
`scholarship-applications`, then this registry, then `register_module()`
each of the four under an agreed name.

## Operational impact

- Because module registration is mutable (unlike everything else in this
  epic, which is append-only/immutable-once-published), a `register_module`
  call silently redirects every future `get_module()` caller to the new
  address with no versioning or history — an indexer that needs "which
  address was `applications` pointed at during event X" must correlate
  against the `MODREG` event stream by timestamp, not query this contract
  for a past value.
- Revoking a role takes effect immediately for the next `has_role`/
  `require_role` call — there is no propagation delay, but there is also
  no automatic effect on actions already in flight in other contracts,
  since none of them check roles here (yet).
