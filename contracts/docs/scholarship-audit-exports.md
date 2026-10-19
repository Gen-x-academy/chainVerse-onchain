# Scholarship Sponsor and Regulator Audit Exports

## On-chain manifest and off-chain package

`scholarship-audit-exports` records one immutable manifest per export ID and program. It commits to a canonical package hash, a source checkpoint hash, and separate governance, decisions, consent, funds, and payments section hashes, with a closed time range and generated-by identity. Exact retries with the same export ID and contents are idempotent; changing any field under an existing ID fails. Each program has a 64-export limit so persistent storage and publication cost are bounded.

The contract does not aggregate, copy, or return application records. The export service assembles files from the existing scholarship-core/programs/registry/admin, applications/eligibility/review/award, and finance/disbursement contracts, their transaction events, and verified payment-network receipts. It canonicalizes each section, computes the section hashes and package hash, records the source event/ledger checkpoint, and publishes the manifest. Auditors verify downloaded package bytes against the hashes and independently resolve cited on-chain records. A hash proves byte integrity, not completeness, truth, or authorization of a source record.

The contract events are `AUDPROG(program_id, sponsor)`, `AUDGRNT(program_id, regulator)`, `AUDRVK(program_id, regulator)`, and `AUDEXPORT(export_id, program_id, package_hash, period_start, period_end, generated_at)`. Export publication events intentionally omit the section hashes; authorized callers read the full manifest. Events are indexing signals, not access credentials.

No names, application answers, evidence documents, consent text, private financial details, or file locations are stored in this contract. Public events expose program IDs, export IDs, public addresses on access changes, package hashes, and period/timestamp metadata. Keep export IDs opaque where correlation itself is sensitive, and do not put private identifiers in event data.

## Authorization and scope

The admin registers a program and its sponsor once. The sponsor or admin may publish exports. The sponsor or admin may grant and revoke a regulator address for one program; grants do not transfer across programs. Export reads require caller authorization and allow only the program sponsor, the platform admin, or a currently granted regulator. Missing setup and missing grants fail closed. Revocation immediately blocks subsequent contract reads.

Off-chain download endpoints must enforce the same program-scoped grant state on every request and must not treat possession of an export ID or event as authorization. Recheck revocation before serving files; use expiring transport credentials and audit download activity outside this contract. The contract cannot revoke bytes already downloaded or authenticate the completeness of a third-party package.

## Dependencies and rollout

Source evidence is assembled from the existing scholarship governance, application/consent, review/decision, finance, award, and disbursement modules. This contract has no cross-contract invocation dependencies, so package consistency and source joins are an explicit indexer/export-service trust boundary. Include contract IDs, schema versions, relevant transaction/ledger checkpoints, query rules, and known gaps in a versioned off-chain package manifest.

Deploy and initialize this contract, register program sponsor addresses, grant regulator access, and run a test export before enabling production downloads. Keep the off-chain canonicalization/hash implementation versioned and test it against contract event fixtures. Export services should paginate source ledgers to a fixed checkpoint, generate all five sections from that same checkpoint, then publish; retries reuse the same export ID. If records change after the checkpoint, publish a new export rather than mutating a prior package. Renew persistent program, grant, count, and export entries before their Soroban TTLs lapse; preserve exported packages and transaction receipts off-chain for retention requirements.

This implements the on-chain evidence commitment and authorization primitive for sponsor/regulator exports. The downloadable evidence store, event indexer, legal retention policy, and completeness attestations remain off-chain work and must be coordinated with the corresponding scholarship application, governance, finance, and API repositories.