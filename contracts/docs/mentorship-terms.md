# MENTOR-016: Versioned Mentorship Terms

## Contract boundary

`mentorship-terms` publishes immutable, versioned terms commitments for an upstream mentorship program and records a mentee's affirmative acceptance against a unique application reference. The storage schema starts at version 1; there is no pre-existing mentorship storage to migrate. Integrations must preserve the contract's `DataKey` and public struct encodings when deploying compatible upgrades. `storage_version()` is exposed for deployment checks.

The platform admin registers each opaque 32-byte `program_id` and its publisher once. Only that publisher can append terms versions; only the mentee can authorize their acceptance. The publisher cannot change or remove a prior version, and the admin cannot reassign the program owner through this contract. New acceptances always pin the latest version and are refused at or after its `expires_at`. A published version's expiry never invalidates an earlier acceptance.

## On-chain and off-chain data

On-chain state contains seven fixed-size SHA-256 commitments: eligibility, fees, cancellation, conduct, privacy, evidence, and completion. It also contains program and mentee addresses, the opaque application reference, timestamps, version numbers, and acceptance status by presence of a receipt. The complete published `TermsVersion` is copied into each acceptance so the accepted snapshot is independently readable. No agreement text, personal details, evidence, messages, or application answers belong on-chain.

Publishers should create canonical, versioned off-chain documents, hash each category, and distribute the documents and a human-readable old/new diff before publication. Clients must verify hashes before showing acceptance and retain the source documents under the platform's privacy and retention policy. The contract cannot validate the semantics of documents or prove that a presented off-chain document matches a hash without the client performing that check.

The event topics are `PROGREG` with `(program_id, owner)`, `TERMPUB` with `(program_id, version, hashes, expires_at)`, and `TERMACK` with `(program_id, application_ref, mentee, version, accepted_at)`. These public events reveal the existence and timing of a mentorship acceptance; application references must therefore be opaque commitments, not names, emails, or guessable identifiers.

## Bounds, TTL, and rollout

Each term record has a fixed-size layout; dynamic text and lists are excluded. A program can publish at most 64 versions, bounding the contract's version history. The cap is intentional and must be monitored; this deployment does not offer pruning because versions and receipts are immutable.

Persistent Soroban entries have finite TTL. Reads and writes extend touched entries, but they cannot guarantee indefinite retention if nobody accesses the contract. Run a keeper that renews each program registration, published version, active version pointer, and acceptance receipt within the network's TTL window. Keep exported copies of documents and transaction receipts off-chain as well. The acceptance snapshot protects against later publication changes, not against an acceptance entry expiring without keeper renewal.

For rollout, deploy this new contract independently, initialize the admin, register program IDs and publishers, and publish verified terms before enabling applications. Integrating application services must atomically require a successful `accept_terms` receipt before creating an application, then store its program ID, application reference, and version. Retries must reuse the same reference. Existing systems without a trusted terms receipt should not be backfilled as if consent had occurred; obtain fresh acceptance. Upgrade the schema only additively. The unit test verifies schema version 1 and decodes persisted records, but no earlier deployed mentorship schema exists for a historical fixture; future releases should add a fixture-based upgrade compatibility test before changing storage encodings.

MENTOR-016 is the on-chain work item in the Programs track. Matching work in the other ChainVerse repositories must provide the off-chain canonical terms/document store, hash and diff review workflow, application/API enforcement, privacy/retention controls, and user-facing consent UX. This contract does not implement those systems or payments, cancellation settlement, evidence adjudication, or completion verification.