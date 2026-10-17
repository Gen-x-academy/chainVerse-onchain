# MENTOR-112: Mentorship Threat Model

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Threat-model mentorship abuse cases.

## On-chain

Nothing is stored. The model names the assets the contract protects: escrowed funds, agreement hashes and status flags.

## Off-chain boundary

Profiles, messages, calendars, evidence and anything about minors stay off-chain; each is listed with its trust boundary and owner.

## Rules

- Attacker paths covered: payment fraud, impersonation, replayed calls, scraping of linked data, harassment through contact fields.
- Each threat gets a mitigation and a named owner; unowned threats block rollout.
- The model is reviewed whenever a mentorship entry point is added.

## Events and errors

Mitigations map to stable errors such as `NotAuthorized` and `AlreadyProcessed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-112 work lives in the other ChainVerse repositories.
