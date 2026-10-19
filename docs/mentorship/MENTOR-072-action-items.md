# MENTOR-072: Assign and Track Action Items

**Track:** Goals | **Component:** on-chain | **Status:** design

Goal: Assign and track action items.

## On-chain

An action item per engagement holding its owner, due date, status, priority, originating session, reminder state and the hash of any completion evidence.

## Off-chain boundary

The task list, reminders and the completion evidence upload are served by the app.

## Rules

- An action item has exactly one owner. Reassignment is recorded as a transfer rather than a silent overwrite.
- Only the owner may change status. A coordinator may reassign but may not complete an item on someone's behalf.
- Source session is recorded when the item came out of a session, which is what lets the completion summary attribute it.
- Completion evidence is committed as a hash; the file or text itself stays off-chain.
- Completion requires evidence. An item cannot be marked done with an empty evidence set.
- Reminders are counted on-chain but dispatched off-chain, so the ledger records that a reminder was due without ever storing its text.
- An item may outlive its originating session but not its agreement.

## Events and errors

Events `action_item_created(agreement, item, owner)` and `action_item_completed(agreement, item, evidence_hash)`. Errors: `NotAuthorized`, `NotFound`, `AgreementClosed`, `EvidenceRequired`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-072 work lives in the other ChainVerse repositories.