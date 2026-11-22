# CAPSTONE-081: Configure Project Funding Models

## Scope
- Implements on-chain funding model configuration primitive for CAPSTONE-081 (#1429).
- Contract: `contracts/capstone-funding-models`.
- Supports unfunded, grant, milestone, reimbursement, stipend, bounty, and sponsor-funded models with asset, payer, recipients, and fee configuration.

## On-chain Stores vs Off-chain Content
- On-chain: tenant/project/intent IDs, model kind, asset address, optional payer, bounded recipient share list (address + bps), optional fee recipient and fee bps, total amount, expected version, timestamp, scoped configurer roles, bounded model count.
- Off-chain: legal agreements, grant terms, invoices, reimbursement evidence, payout schedules, and descriptive funding narratives; only bounded amounts/IDs/enums/addresses stored on-chain.

## Authorization
- `initialize(admin)` one-time.
- `set_role` admin-only for scoped `Configurer` role per tenant/project.
- `configure_model` requires caller `require_auth()` and active scoped configurer role; deny by default.

## Idempotency
- `configure_model` keyed by `(tenant_id, project_id, intent_id)` with `expected_version == 0` on first write.
- Identical replay of the same intent payload returns existing model without duplication.
- Conflicting replay for same intent or stale expected version rejected (`ModelExists`, `VersionMismatch`).

## Errors
- Append-only `ContractError`: `AlreadyInitialized(1)`, `NotInitialized(2)`, `Unauthorized(3)`, `ModelExists(4)`, `ModelNotFound(5)`, `VersionMismatch(6)`, `InvalidAmount(7)`, `InvalidFee(8)`, `TooManyRecipients(9)`, `InvalidShare(10)`, `LimitReached(11)`, `InvalidModel(12)`, `ArithmeticError(13)`.

## Events
- `CFM_ROLE`: tenant, project, account, role, active.
- `CFM_CFG`: tenant, project, intent_id, model_kind, total_amount, fee_bps.
- Events carry only IDs/enums/amounts/bps.

## Privacy Boundaries
- No private financial documents, personal beneficiary details beyond public addresses, or free-text terms on-chain.
- Model selection and numeric configuration are minimized to required public settlement/attestation parameters.

## Dependencies
- Depends on project/tenant identifiers and downstream escrow, payout, or settlement primitives that may consume configured model commitments.
- Asset contracts and payer/recipient addresses must be valid in the target network context.

## Rollout/Rollback
- Rollout: deploy/initialize, grant configurer roles, integrate funding setup flows to submit deterministic `intent_id` and validated share/fee parameters.
- Rollback: stop new configuration writes in off-chain flows; existing models remain immutable records. Corrective changes require new intents/config records in dependent workflows (no in-place mutation).

## Corresponding Other-Repository Work
- Backend: funding policy capture, share/fee validation, intent generation, reconciliation and retry handling.
- Frontend: funding model configuration UI, validation feedback, and model status display.
- Settlement/treasury services: read on-chain model records to drive disbursement/escrow actions in separate components without storing sensitive terms on-chain.
