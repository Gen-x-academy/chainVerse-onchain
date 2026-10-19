# MENTOR-073: Submit Mentorship Deliverables

**Track:** Goals | **Component:** on-chain | **Status:** design

Goal: Submit mentorship deliverables.

## On-chain

A deliverable submission per milestone holding a monotonic version, a hash of the submitted content, the submission ledger time and the integrity metadata describing how that hash was produced.

## Off-chain boundary

The submission form, file and repository uploads and the diff view between versions are served by the app.

## Rules

- Submissions are versioned and append-only. Version n+1 must reference version n, so history cannot be rewritten.
- Integrity metadata records the content hash and how it was derived - file digest, commit reference or text hash. The content itself is never stored.
- The submitter must be the milestone owner. A designated reviewer accepts; a submitter cannot accept their own work.
- Declaring a submission complete is explicit and separate from submitting it.
- A superseded submission stays readable and remains attributable to its author.
- Only one submission may be under review at a time. A new submission supersedes the previous one by reference rather than replacing it.

## Events and errors

Events `deliverable_submitted(milestone, submission, version)` and `deliverable_accepted(milestone, submission)`. Errors: `NotAuthorized`, `MilestoneClosed`, `VersionConflict`, `SelfAcceptance`, `DeliverableNotFound`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-073 work lives in the other ChainVerse repositories.