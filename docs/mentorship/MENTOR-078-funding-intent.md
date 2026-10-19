# MENTOR-078: Create Mentorship Escrow Funding Intent

**Track:** Payments | **Component:** on-chain | **Status:** design

Goal: Create mentorship escrow funding intent.

## On-chain

A funding intent keyed by a payer-scoped nonce, holding the agreement, amount, currency or asset, expiry and the settlement conditions that must hold for the funds to be applied.

## Off-chain boundary

The payment initiation screen, the wallet signature prompt and the confirmation receipt are served by the app.

## Rules

- An intent is keyed by payer and nonce. Replaying the same nonce returns the original intent rather than creating a second one.
- The intent is bound to one agreement, one payer, one amount and one currency or asset. None of these may be altered after creation.
- The expiry is enforced against ledger time. An expired intent cannot be funded and must be replaced by a new one.
- Settlement conditions are committed with the intent, so what the funds are for is fixed before any money moves.
- An intent may be cancelled by its payer while unfunded. A funded intent can no longer be cancelled by the payer.
- Creating an intent for an agreement with no accepted pricing model is rejected.

## Events and errors

Events `funding_intent_created(agreement, payer, nonce, amount)` and `funding_intent_cancelled(agreement, payer, nonce)`. Errors: `NotAuthorized`, `IntentExpired`, `PricingNotAccepted`, `AmountInvalid`, `AlreadyFunded`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-078 work lives in the other ChainVerse repositories.