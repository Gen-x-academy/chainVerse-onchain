# Scholarship Audit Trail

## ABI and event contract

`scholarship-audit` accepts `record(source, domain, action, entity_hash, actor_hash, payload_hash)` calls from registered sources. Each successful call increments a global sequence and emits an `AUDIT` event keyed by that sequence. The event contains the source contract, domain/action, ledger timestamp, three 32-byte commitments, the preceding record hash, and the new record hash. The hash is SHA-256 over the XDR encoding of the record fields excluding the new hash.

Only an administrator can register or revoke sources. A source must authenticate every append. Records cannot be edited or deleted through the contract ABI; revocation prevents future appends but does not rewrite past events. `get_head` exposes the current sequence and hash for external checkpointing.

## Privacy and completeness boundary

Event fields are intentionally limited to workflow categories and commitments. Callers must not submit names, application answers, review text, or other personal data in any hash preimage exposed to the contract. Hash low-entropy or guessable values only with a high-entropy, off-chain salt; actor and entity hashes should be stable opaque identifiers where correlation is required.

This contract does not automatically observe calls to other contracts. To make the trail complete, each owning contract must call `record` in the same transaction as every security-relevant read and every program, application, review, decision, award, milestone, and finance mutation. A failed audit call must fail the operation as well. Reads that cannot be made through an atomic contract call must be recorded by an explicitly trusted backend/relayer, which is an off-chain trust boundary and must be monitored separately. The current contract suite is not fully integrated with this logger merely by deploying it.

The audit events are the canonical history; Soroban storage retains only the chain head and source registrations with finite TTL. Operators must archive and index `AUDIT` events, verify sequence continuity and hash links, checkpoint the head independently, and restore/validate contract state if instance storage expires. Blockchain event availability and trusted off-chain indexing are operational dependencies, not guarantees made by this contract.