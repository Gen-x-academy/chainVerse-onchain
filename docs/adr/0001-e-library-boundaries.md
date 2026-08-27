# ADR 0001 — E-Library Boundary Invariants

- Status: Draft
- Deciders: E-Library / Foundation Working Group
- Date: 2026-08-27

## Context

A future on-chain e-library is being envisioned for the ChainVerse ecosystem.
Before any smart contract is written, we must pin down exactly which concerns
live on-chain (and therefore require consensus and honor the ecosystem's
multisig/role-overlap invariants) and which concerns remain off-chain. This ADR
records those boundaries so that the contract surface, storage, and authorization
model are fixed up front and reviewed consistently with the vault hardening work
(issues #865, #866, #867).

This document defines a *target architecture*. No e-library contract exists in
this repository yet; it is written as an ADR so that the eventual implementation
matches a reviewed contract boundary rather than drifting during development.

## Contracts

The e-library domain is split into three logical on-chain contracts, each a
separate `#![no_std]` Soroban contract under `contracts/`:

1. **LibraryRegistry** — the single source of truth for *catalog* records:
   work identifiers, edition ownership, lending metadata, and the canonical
   list of approved off-chain catalogs. Write access requires consensus.
2. **CheckoutVault** — escrows borrowed/lent digital-asset tokens for the
   duration of a loan, reusing the `escrow-vault` semantics (thresholded
   approvers, ConflictOfInterest exclusion, collision-resistant ids).
3. **RatingsOracle** — federated (multisig) ratings and reviews; each rating is
   a signed vote and consensus is required before a rating aggregate is
   recomputed on-chain.

The contracts may share a small `shared` crate for types and error enums, mirroring
the existing workspace layout.

## Actors

- **Librarian (Admin)** — bootstrap lifecycle owner; can set admins and upgrade
  contracts. Mirrors `EscrowVault::set_admin` / `upgrade`.
- **Curators** — a multisig set of approvers that collectively curate the
  catalog. Individual curation actions are thresholded; a curator may not vote
  on content they authored or benefit from (role-overlap exclusion, cf.
  `ConflictOfInterest`).
- **Patron** — a reader who checks out works. Patrons are identity/depositor
  actors, never approvers over content they consume.
- **Restorer / Rights Holder** — asserts content rights; grant/revoke of rights
  requires consensus (see "Rights requiring consensus").

## Trust boundaries

- The **contracts and their storage are objectively verifiable**: anyone can read
  the catalog, balances, and approval states on-chain.
- The **off-chain content pipeline** (PDFs, streaming URLs, metadata blobs) is
  *not* trusted as fact. On-chain records reference off-chain content by
  content-addressed identifiers (hash), never by embedding the content itself.
- **Identity** (KYC, patron identity, rights-holder identity) lives off-chain.
  On-chain actors are plain Soroban `Address`es; any real-world identity binding
  is off-chain and published only as an audit link, not as a consensus input.
- The **Foundation/Admin** is a bootstrap trust anchor solely for deployment and
  upgrades, constrained to timelocked, audited operations. Admin cannot rewrite
  catalog consensus; it can only upgrade the contract code.

## Invariants

- **I1 — No role overlap.** No actor may be both a curator/approver and the
  beneficiary (rights holder, author, or patron) of the item under decision.
  This mirrors the vault `ConflictOfInterest` rule and is enforced both at
  proposal time and at vote time.
- **I2 — Thresholded mutability.** Every state-changing catalog action
  (add, deprecate, re-grade, rights grant/revoke) requires a threshold of
  distinct curator votes ≥ 1 and ≤ the unique eligible curator count.
  Threshold zero is rejected.
- **I3 — Collision-free identifiers.** Every on-chain record id is unique
  regardless of ledger timestamp, using a monotonic instance-storage nonce
  mixed into a hash input (cf. `EscrowVault` id fix, #866).
- **I4 — Content addressed, not embedded.** Off-chain content is referenced by
  hash; the contract never stores or trusts raw file content.
- **I5 — Auditability & ttl.** All on-chain records are persistent with an
  explicit TTL and every mutation emits an event (`symbol_short!` topics) for
  the indexer, matching the vault event pattern.

## Rights requiring consensus

The following rights are **stateful and require curator consensus** to change:

- Authorization to list a work in the catalog (add/remove/deprecate).
- Grant or revocation of lending/redistribution/derivative rights per edition.
- Re-grading or reclassification of a work.
- Changing the threshold, curator set, or approval policy on a registry.

The following are **not** stateful consensus inputs:

- Actual content bytes, streaming URLs, covers, previews (off-chain, content-addressed).
- Patron identity / KYC attestations (off-chain).
- Usage/reading history and recommender signals (off-chain analytics).

## Failure modes

- **Vote replay / double count** — a single address approving twice; blocked by a
  per-(record, actor) vote key, as in `VotedKey`.
- **Threshold undershoot via dedup** — approval counted twice because of duplicate
  entries; avoided by deduplicating the eligible approver set before threshold checks.
- **Role-overlap exploit** — a beneficiary approving its own payout/reward;
  blocked by the `ConflictOfInterest` check at both proposal and vote time.
- **ID collision** — two records born in one ledger sharing an id; avoided by the
  monotonic nonce in id derivation (I3).
- **Admin capture** — admins re-writing history; mitigated by timelocked, code-only
  upgrade paths and immutability of catalog records (records are append-only; edits
  are new versions with a deprecation flag).

## Non-goals (explicitly out of scope for the on-chain contracts)

- **Full-text storage / DRM.** The chain does not store copyrighted content or
  enforce DRM; it only records rights and lending state.
- **On-chain identity / KYC.** No identity proofs are stored or verified on-chain.
- **Content review/quality scoring** as a consensus-on-chain value; that is off-chain
  and only surfaced as off-chain signals.
- **Payment processing beyond escrow.** The checkout escrow handles token custody
  during a loan; recurring billing, subscriptions, and fiat settlement are off-chain.

## ABI / Interface impact

- `#![no_std]` Soroban contracts using `contracttype` / `contracterror` /
  `contractimpl`, matching `escrow-vault`.
- Consistent `Result<_, ELibraryError>` error enum with unique discriminants.
- `Client` wrappers generated by `#[contractimpl]`; no hand-rolled ABI.

## Storage impact

- Instance storage: admin, curator set, threshold, nonce counter (per I3), and
  policy parameters.
- Persistent storage: catalog records keyed by content-id-derived `BytesN<32>`,
  vote/approval keys, and escrow balance records, each with explicit TTL.
- No contract holds large blobs; storage stays small and indexable.

## Event impact

- Every mutation publishes an event with a `symbol_short!` topic and serialized
  payload, matching the vault conventions (`VAULT_NEW`, `emrg_cncl`, etc.), so the
  existing `contract_event_indexer` can index e-library activity uniformly.

## Privacy impact

- Only consensus-relevant facts (catalog records, rights, balances, approvals) are
  public on-chain.
- Reading history, patron identity, and content are never stored on-chain; privacy
  is preserved by keeping them off-chain and content-addressed.

## Deployment & migration impact

- New contracts deployed separately from `escrow-vault`; no existing storage layout
  is modified.
- Upgrades use the same admin-gated `deployer().update_current_contract_wasm`
  path with a prior ADR-reviewed migration plan.
- Any future schema migration must bump record keys (never reuse ids) to preserve
  I3/I5 invariants.

## Consequences

- **Positive:** objective, auditable consensus over catalog rights; reuse of the
  hardened vault pattern; clear off-chain/on-chain split keeps chain state small.
- **Negative:** on-chain agreement is limited to catalog/rights facts; richer
  notions of content quality and identity must be handled off-chain.
