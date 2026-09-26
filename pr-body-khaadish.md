## Summary

This PR implements the scholarship finance operations contract addressing four financial issues:

### Implemented Features

1. **Export Sponsor Financial Statements** (#1109)
   - `generate_statement()` produces period statements for contributions, commitments, payments, fees, refunds, recoveries, and remaining balances
   - Statements reconcile to ledger entries via event log, use consistent currency metadata from program
   - Large exports run asynchronously (single contract call per statement; bulk export off-chain)
   - Documentation identifies ownership, privacy, migration, and operational impact

2. **Manage Recoveries and Clawbacks** (#1108)
   - `initiate_recovery()` / `complete_recovery()` track policy-authorized recovery requests after fraud, withdrawal, or milestone failure
   - Recovery never silently debits a wallet; reason and legal basis are recorded in `RecoveryRecord.legal_basis`
   - Collected amounts reconcile to open claims via `ProgramFinancials.total_recovered`
   - Adversarial contract tests included

3. **Process Refunds and Returned Payments** (#1107)
   - `initiate_refund()` / `complete_refund()` handle sponsor refunds, rejected transfers, overpayments, unused program balances
   - Refund authority restricted to admin; liabilities remain solvent
   - Original entries reversed rather than deleted (full audit trail via `RefundRecord`)
   - Proportional fee refund calculation

4. **Calculate Platform and Network Fees Transparently** (#1106)
   - `configure_fees()` with versioned `FeeBasis` (Percentage, Fixed, PercentageWithCap)
   - `calculate_fees()` pure preview returns `(platform_fee, network_fee, total_fee, net_amount)` — no state changes
   - Fee basis and rounding versioned in `FeeConfig.version`
   - Fee revenue separately accounted in `ProgramFinancials.total_fees_collected`

### Contract Added

- `contracts/scholarship-finance/` - New Soroban contract with:
  - Fee configuration and calculation
  - Refund processing with audit trail
  - Recovery/clawback management with legal basis
  - Financial statement generation
  - Program financial aggregates

### Documentation Added

- `contracts/docs/scholarship-finance.md` - Complete contract documentation covering:
  - Data model and events
  - TTL policy
  - Authorization model
  - Fee calculation details (rounding, versioning)
  - Refund/recovery flows
  - Statement fields and reconciliation
  - Migration & upgrade notes
  - Security considerations

### Tests Included

- Unit tests for all fee basis types (percentage, fixed, capped)
- Fee calculation validation (invalid percentages, negative net amounts)
- Authorization and error path coverage

### Closes

Closes #1109, #1108, #1107, #1106