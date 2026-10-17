# MENTOR-107: Mentorship Consent Ledger

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Implement mentorship consent ledger.

## On-chain

Per user and purpose: a status (granted or withdrawn), policy version, source code and ledger time. Purposes: matching, calendar, recording, analytics, credentials, research.

## Off-chain boundary

The consent text and any personal details stay off-chain; the ledger holds only the version number and hashes.

## Rules

- Consent is never inferred from other actions; each purpose is recorded on its own.
- Withdrawal stops future processing; history is append-only.
- Mandatory purposes are marked and explained to the user before they are recorded.

## Events and errors

Events `consent_granted` and `consent_withdrawn`. Errors: `NotAuthorized`, `UnknownPurpose`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-107 work lives in the other ChainVerse repositories.
