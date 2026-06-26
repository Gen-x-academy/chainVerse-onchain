# Requirements Document

## Introduction

This feature adds comprehensive input validation to the token transfer operations in the Escrow contract. Currently, `create_escrow` accepts any `amount` and `expiration` values without bounds checking, which can lead to silent failures or exploitable edge cases (e.g., zero-amount escrows, escrows that expire in the past, or transfers to self). This feature hardens all transfer entry points by rejecting structurally invalid inputs before any state changes or token movements occur.

## Glossary

- **Escrow_Contract**: The Soroban smart contract managing escrow lifecycle (`create_escrow`, `release_funds`, `refund_buyer`).
- **Validator**: The input validation logic executed at the start of each Escrow_Contract entry point.
- **Transfer**: Any movement of tokens initiated by the Escrow_Contract, including escrow creation, fund release, and buyer refund.
- **Amount**: The `i128` token quantity specified in a `create_escrow` call.
- **Expiration**: The ledger timestamp (u64) after which an escrow may be refunded.
- **Caller**: The address invoking an Escrow_Contract entry point.
- **Whitelisted_Token**: A token address that has been explicitly approved for use in escrows via `whitelist_token`.

---

## Requirements

### Requirement 1: Reject Zero and Negative Transfer Amounts

**User Story:** As a protocol operator, I want zero and negative amounts rejected at the contract boundary, so that escrows always represent a real economic commitment.

#### Acceptance Criteria

1. WHEN `create_escrow` is called with an `amount` of zero, THE Validator SHALL return `EscrowError::InvalidAmount` without transferring tokens or writing state.
2. WHEN `create_escrow` is called with a negative `amount`, THE Validator SHALL return `EscrowError::InvalidAmount` without transferring tokens or writing state.
3. WHEN `create_escrow` is called with a positive `amount`, THE Validator SHALL allow the call to proceed to token transfer.

---

### Requirement 2: Reject Expired or Same-Ledger Expiration Timestamps

**User Story:** As a buyer, I want escrows to have a meaningful future expiration, so that I always have a window to release funds before the escrow can be refunded.

#### Acceptance Criteria

1. WHEN `create_escrow` is called with an `expiration` less than or equal to the current ledger timestamp, THE Validator SHALL return `EscrowError::InvalidExpiration` without transferring tokens or writing state.
2. WHEN `create_escrow` is called with an `expiration` strictly greater than the current ledger timestamp, THE Validator SHALL allow the call to proceed.

---

### Requirement 3: Reject Self-Transfers

**User Story:** As a protocol operator, I want escrows where the buyer and seller are the same address rejected, so that the contract cannot be used to launder state or game fee logic.

#### Acceptance Criteria

1. WHEN `create_escrow` is called with a `buyer` address equal to the `seller` address, THE Validator SHALL return `EscrowError::InvalidRecipient` without transferring tokens or writing state.
2. WHEN `create_escrow` is called with distinct `buyer` and `seller` addresses, THE Validator SHALL allow the call to proceed.

---

### Requirement 4: Reject Non-Whitelisted Tokens

**User Story:** As a protocol operator, I want only approved tokens accepted in escrows, so that the contract cannot be used with arbitrary or malicious token contracts.

#### Acceptance Criteria

1. WHEN `create_escrow` is called with a `token` address that is not whitelisted, THE Validator SHALL return `EscrowError::TokenNotAllowed` without transferring tokens or writing state.
2. WHEN `create_escrow` is called with a whitelisted `token` address, THE Validator SHALL allow the call to proceed to token transfer.

---

### Requirement 5: Validation Produces No Side Effects on Rejection

**User Story:** As a protocol operator, I want all validation failures to leave contract state unchanged, so that rejected calls cannot partially modify storage or emit spurious events.

#### Acceptance Criteria

1. IF the Validator returns any error, THEN THE Escrow_Contract SHALL NOT increment the escrow ID counter.
2. IF the Validator returns any error, THEN THE Escrow_Contract SHALL NOT emit any events.
3. IF the Validator returns any error, THEN THE Escrow_Contract SHALL NOT transfer any tokens.
4. THE Escrow_Contract SHALL perform all validation checks before executing any token transfer or state mutation.

---

### Requirement 6: Validation Error Codes Are Distinct and Documented

**User Story:** As a contract integrator, I want each validation failure to return a unique, named error code, so that callers can programmatically distinguish and handle each rejection reason.

#### Acceptance Criteria

1. THE Escrow_Contract SHALL expose `EscrowError::InvalidAmount` as a distinct error variant for amount validation failures.
2. THE Escrow_Contract SHALL expose `EscrowError::InvalidExpiration` as a distinct error variant for expiration validation failures.
3. THE Escrow_Contract SHALL expose `EscrowError::InvalidRecipient` as a distinct error variant for self-transfer validation failures.
4. WHEN multiple validation rules are violated simultaneously, THE Validator SHALL return the error corresponding to the first failing check in the order: amount → expiration → self-transfer → token whitelist.
