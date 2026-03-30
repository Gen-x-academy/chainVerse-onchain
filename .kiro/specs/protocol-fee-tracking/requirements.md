# Requirements Document

## Introduction

This feature adds protocol fee tracking to the Chainverse smart contract platform. Currently, `chainverse-core` calculates protocol fees via `calculate_fee` and applies a `protocol_fee` basis-point rate, but collected fees are never recorded or queryable. This feature introduces persistent storage of fees collected per token, per transaction, and in aggregate, along with admin query and withdrawal capabilities.

## Glossary

- **Fee_Tracker**: The on-chain component responsible for recording and querying accumulated protocol fees.
- **Protocol_Fee**: A basis-point percentage (e.g. 100 = 1%) deducted from escrow amounts at release time and credited to the protocol.
- **Fee_Record**: A single entry capturing the token address, fee amount, escrow ID, and ledger timestamp for one fee collection event.
- **Fee_Ledger**: The persistent, append-only collection of all Fee_Records stored on-chain.
- **Admin**: The privileged address stored in `Config.admin`, authorized to query and withdraw accumulated fees.
- **Token**: A Soroban-compatible token contract address (must be in the supported-token list).
- **Basis_Point**: One hundredth of one percent (0.01%). Protocol fees are expressed in basis points.

## Requirements

### Requirement 1: Record Fee on Escrow Release

**User Story:** As a protocol operator, I want a fee to be recorded every time an escrow is released, so that I have an accurate on-chain history of all protocol revenue.

#### Acceptance Criteria

1. WHEN an escrow is released successfully, THE Fee_Tracker SHALL compute the protocol fee amount using the current `protocol_fee` basis-point rate and the escrow's token amount.
2. WHEN an escrow is released successfully, THE Fee_Tracker SHALL persist a Fee_Record containing the escrow ID, token address, fee amount, and ledger timestamp.
3. WHEN the computed protocol fee amount is zero, THE Fee_Tracker SHALL still persist a Fee_Record with a fee amount of zero.
4. IF an escrow release fails for any reason, THEN THE Fee_Tracker SHALL NOT persist any Fee_Record for that transaction.
5. THE Fee_Tracker SHALL increment the per-token accumulated fee total by the fee amount each time a Fee_Record is stored.

---

### Requirement 2: Query Accumulated Fees by Token

**User Story:** As a protocol operator, I want to query the total fees collected for a specific token, so that I can monitor revenue per asset.

#### Acceptance Criteria

1. THE Fee_Tracker SHALL expose a `get_fees_collected` query that accepts a token address and returns the total accumulated fee amount for that token as an `i128`.
2. WHEN `get_fees_collected` is called for a token that has no recorded fees, THE Fee_Tracker SHALL return zero.
3. THE Fee_Tracker SHALL ensure the value returned by `get_fees_collected` equals the sum of all Fee_Record amounts for that token.

---

### Requirement 3: Query Full Fee History

**User Story:** As a protocol operator, I want to retrieve the complete list of fee collection events, so that I can audit protocol revenue in detail.

#### Acceptance Criteria

1. THE Fee_Tracker SHALL expose a `get_fee_history` query that returns all stored Fee_Records in chronological order (ascending by ledger timestamp).
2. WHEN `get_fee_history` is called and no fees have been recorded, THE Fee_Tracker SHALL return an empty list.
3. THE Fee_Tracker SHALL ensure the count of records returned by `get_fee_history` equals the total number of successful escrow releases that generated a fee.

---

### Requirement 4: Admin-Only Fee Withdrawal

**User Story:** As a protocol admin, I want to withdraw accumulated protocol fees to a designated address, so that collected revenue can be claimed.

#### Acceptance Criteria

1. WHEN `withdraw_fees` is called with a valid caller, token, and recipient address, THE Fee_Tracker SHALL require authorization from the caller.
2. IF the caller is not the Admin, THEN THE Fee_Tracker SHALL return `ContractError::Unauthorized` and SHALL NOT transfer any tokens.
3. WHEN `withdraw_fees` is called by the Admin for a token with a positive accumulated balance, THE Fee_Tracker SHALL transfer the full accumulated fee balance of that token to the recipient address.
4. WHEN `withdraw_fees` completes successfully, THE Fee_Tracker SHALL reset the per-token accumulated fee total to zero.
5. IF `withdraw_fees` is called for a token with zero accumulated fees, THEN THE Fee_Tracker SHALL return `ContractError::InvalidPayment` and SHALL NOT perform a token transfer.
6. WHILE the contract is paused, THE Fee_Tracker SHALL reject `withdraw_fees` calls with `ContractError::ContractPaused`.

---

### Requirement 5: Emit Fee Collection Event

**User Story:** As an off-chain indexer, I want a Soroban event emitted for every fee collected, so that I can track protocol revenue without querying contract storage directly.

#### Acceptance Criteria

1. WHEN a Fee_Record is persisted, THE Fee_Tracker SHALL emit a Soroban event with topic `chainverse:fee:collected` and payload `(escrow_id: u64, token: Address, amount: i128)`.
2. WHEN `withdraw_fees` completes successfully, THE Fee_Tracker SHALL emit a Soroban event with topic `chainverse:fee:withdrawn` and payload `(recipient: Address, token: Address, amount: i128)`.
3. THE Fee_Tracker SHALL emit events using the existing `chainverse:{domain}:{event_name}` naming convention defined in the contract standards.

---

### Requirement 6: Fee Accumulation Correctness (Invariants)

**User Story:** As a protocol developer, I want the fee storage to maintain mathematical consistency at all times, so that the system can be audited and trusted.

#### Acceptance Criteria

1. THE Fee_Tracker SHALL ensure that for any token, the value returned by `get_fees_collected` is always greater than or equal to zero.
2. THE Fee_Tracker SHALL ensure that after N successful escrow releases, `get_fee_history` returns exactly N Fee_Records.
3. THE Fee_Tracker SHALL ensure that the sum of all Fee_Record amounts for a token equals the value returned by `get_fees_collected` for that token at all times (sum invariant).
4. WHEN `withdraw_fees` is called successfully, THE Fee_Tracker SHALL ensure that `get_fees_collected` for that token returns zero immediately after the call.
5. THE Fee_Tracker SHALL ensure that fee amounts stored in Fee_Records are non-negative.
