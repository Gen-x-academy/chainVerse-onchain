## Summary

This PR implements the scholarship disbursement enhancements contract addressing four disbursement issues:

### Implemented Features

1. **Handle Failed Payout Recovery** (#1101)
   - `initiate_recovery()` / `complete_recovery()` for failed payments
   - Failures preserve award eligibility
   - Retries use new transaction envelopes but the same intent
   - Operators receive actionable diagnostics via failure_reason
   - Recovery types: TrustlineMissing, BadDestination, InsufficientFunds, NetworkExpiry, NetworkError, Other

2. **Track Pending and Finalized Transactions** (#1100)
   - `create_transaction_record()` / `update_transaction_state()` for transaction lifecycle
   - State machine: Submitted → Pending → Confirming → Confirmed → Finalized
   - Configurable confirmations per transaction
   - Payment cannot finalize twice (enforced)
   - Failure tracking with retry_count and failure_reason

3. **Execute Scheduled Scholarship Payments** (#1099)
   - `execute_batch()` processes due payments in bounded batches (max 100)
   - Only authorized automation executes
   - Partial batch outcomes reconcile per intent
   - Successful payments record ledger references via TransactionRecord
   - BatchExecutionRecord tracks executed/failed counts

4. **Support Stellar Asset Configuration** (#1098)
   - `configure_asset()` / `get_asset_config()` for native/issued assets
   - Native (XLM): 7 decimals, no issuer, no trustline
   - Issued assets: configurable issuer, decimals (1-18), trustline required
   - Unsupported assets fail early at intent creation
   - Configuration changes governed; recipients receive trustline guidance

### Contract Added

- `contracts/scholarship-disbursement-enhancements/` - New Soroban contract with:
  - Asset configuration and validation
  - Batch payment execution with bounded batches
  - Transaction state tracking with confirmations
  - Failed payment recovery with retry envelopes

### Documentation Added

- `contracts/docs/scholarship-disbursement-enhancements.md` - Complete contract documentation

### Tests Included

- Unit tests for asset configuration validation (native/issued)
- Transaction state machine transitions (valid/invalid)
- Batch execution and recovery flows
- Authorization boundary tests

### Closes

Closes #1101, #1100, #1099, #1098