# MENTOR-086: Mentorship Conduct Reports

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Report mentorship conduct concerns.

## On-chain

A report record: category, urgency, evidence reference hash, safe-contact preference flag and ledger time.

## Off-chain boundary

Evidence files and free-text details are stored off-chain; the reporter's identity is shown only to authorized responders.

## Rules

- Any participant may file a report about a profile, message, session, payment or offline conduct.
- Categories and urgency come from a fixed list.
- A repeated submission with the same reference creates no second report.

## Events and errors

Event `report_filed(report, category, urgency)`. Errors: `UnknownCategory`, `AlreadyProcessed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-086 work lives in the other ChainVerse repositories.
