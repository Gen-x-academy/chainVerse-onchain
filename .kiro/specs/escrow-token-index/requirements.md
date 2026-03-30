# Requirements Document

## Introduction

This feature adds a token-level index to the escrow contract, enabling analytics queries that return all escrows associated with a specific token address. Currently, escrows can only be retrieved by their numeric ID. By maintaining a per-token index, callers can efficiently query the full set of escrows for any whitelisted token, supporting dashboards, reporting, and protocol-level analytics.

## Glossary

- **Escrow_Contract**: The Soroban smart contract that manages escrow lifecycle (create, release, refund).
- **Escrow**: A record holding buyer, seller, token, amount, status, and expiration fields.
- **Token**: A Stellar asset contract address used as the payment denomination inside an escrow.
- **Token_Index**: A persistent mapping from a token address to the ordered list of escrow IDs that use that token.
- **Escrow_ID**: A monotonically increasing `u64` identifier assigned to each escrow at creation time.
- **Caller**: Any external account or contract invoking a query on the Escrow_Contract.

## Requirements

### Requirement 1: Maintain Token-to-Escrow Index on Creation

**User Story:** As a protocol developer, I want every new escrow to be recorded in a per-token index, so that token-level queries always reflect the complete set of escrows.

#### Acceptance Criteria

1. WHEN a new escrow is created via `create_escrow`, THE Escrow_Contract SHALL append the resulting Escrow_ID to the Token_Index entry for the escrow's token address.
2. WHEN a new escrow is created and no Token_Index entry exists for that token, THE Escrow_Contract SHALL initialise a new Token_Index entry containing only the new Escrow_ID.
3. THE Escrow_Contract SHALL maintain the Token_Index in insertion order, preserving the creation sequence of Escrow_IDs for each token.

---

### Requirement 2: Query Escrows by Token

**User Story:** As an analytics consumer, I want to query all escrows for a given token address, so that I can compute token-level metrics such as total volume, active escrow count, and status distribution.

#### Acceptance Criteria

1. WHEN a Caller invokes `get_escrows_by_token` with a valid token address, THE Escrow_Contract SHALL return the ordered list of Escrow_IDs indexed under that token.
2. WHEN a Caller invokes `get_escrows_by_token` with a token address that has no associated escrows, THE Escrow_Contract SHALL return an empty list.
3. THE Escrow_Contract SHALL expose `get_escrows_by_token` as a read-only query that does not modify contract state.

---

### Requirement 3: Index Consistency Across Escrow Lifecycle

**User Story:** As a protocol developer, I want the token index to remain accurate regardless of escrow status changes, so that analytics reflect the true historical record.

#### Acceptance Criteria

1. WHEN an escrow transitions to `Completed` status via `release_funds`, THE Escrow_Contract SHALL retain the Escrow_ID in the Token_Index for that token.
2. WHEN an escrow transitions to `Cancelled` status via `refund_buyer`, THE Escrow_Contract SHALL retain the Escrow_ID in the Token_Index for that token.
3. THE Escrow_Contract SHALL never remove an Escrow_ID from the Token_Index after it has been inserted.

---

### Requirement 4: Index Integrity Under Multiple Tokens

**User Story:** As a protocol developer, I want each token to maintain its own independent index, so that querying one token does not return escrows denominated in a different token.

#### Acceptance Criteria

1. THE Escrow_Contract SHALL store a separate Token_Index for each distinct token address.
2. WHEN escrows are created using two different token addresses, THE Escrow_Contract SHALL ensure that `get_escrows_by_token` for token A returns only Escrow_IDs whose token field equals token A.
3. FOR ALL token addresses T, every Escrow_ID returned by `get_escrows_by_token(T)` SHALL correspond to an Escrow record whose `token` field equals T (index correctness invariant).

---

### Requirement 5: Error Handling for Invalid Inputs

**User Story:** As a Caller, I want the query to handle invalid inputs gracefully, so that my client receives a clear error rather than a panic or silent failure.

#### Acceptance Criteria

1. IF a Caller invokes `get_escrows_by_token` with an address that is not a valid contract address format, THEN THE Escrow_Contract SHALL return an `InvalidToken` error.
2. IF a Caller invokes `get_escrows_by_token` with a token address that has never been whitelisted, THEN THE Escrow_Contract SHALL return an empty list rather than an error, because the absence of escrows is a valid state.
