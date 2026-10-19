# MENTOR-075: Build Progress Timeline

**Track:** Goals | **Component:** on-chain | **Status:** design

Goal: Build progress timeline.

## On-chain

An append-only ordered entry per engagement, each holding its kind, the referenced record id, the ledger timestamp that orders it and the visibility the reader must satisfy to see it.

## Off-chain boundary

Timeline rendering, grouping by day or milestone, and the wording attached to each entry are served by the app.

## Rules

- Entries are append-only and ordered by ledger timestamp. An entry is never edited or deleted after it is written.
- Each entry names exactly one kind - goal, session, milestone, deliverable, decision or agreement change - and the record it refers to.
- Visibility is resolved at read time against the reader, so a private goal never appears in the other participant's timeline.
- Entries the reader may not see are omitted entirely rather than returned with a placeholder, so their existence is not inferable.
- A coordinator timeline may include coordination entries that neither participant can see.
- The timeline is permission-filtered on every read. Filtering is never applied once at write time and then cached.

## Events and errors

Event `timeline_entry_appended(agreement, kind, entry)`. Errors: `NotAuthorized`, `NotFound`, `ReferenceInvalid`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-075 work lives in the other ChainVerse repositories.