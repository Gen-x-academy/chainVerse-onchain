# MENTOR-079: Confirm Escrow Funding

**Track:** Payments | **Component:** on-chain | **Status:** design

Goal: Confirm escrow funding.

## On-chain

A funding confirmation keyed by intent, holding the observed provider or ledger reference, the observed amount and asset, the finality depth reached and the ledger time it was recorded.

## Off-chain boundary

Payment provider webhooks, the confirmation spinner and the receipt shown to the payer are served by the app.

## Rules

- Confirmation matches an existing funding intent by intent id. An observation that matches no intent is rejected rather than creating an escrow balance.
- The observed amount and asset must match the intent exactly. A partial or over payment is recorded as unmatched, never applied.
- Funds are marked available only once the configured finality depth is reached. Below that depth the confirmation is pending.
- Confirmation is idempotent. A repeated observation for the same intent returns the original result and marks available no further.
- Exactly one intent may be confirmed per agreement at a time. A second confirmed intent against the same agreement is rejected.
- A payer cannot confirm their own funding. Confirmation is performed by the provider adapter or a coordinator.
- An intent that expired before confirmation can never become available; it must be replaced.

## Events and errors

Events `funding_confirmed(intent, amount, finality)` and `escrow_funds_available(agreement, amount)`. Errors: `NotAuthorized`, `IntentNotFound`, `IntentExpired`, `AmountMismatch`, `AssetMismatch`, `FinalityNotReached`, `AlreadyConfirmed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-079 work lives in the other ChainVerse repositories.