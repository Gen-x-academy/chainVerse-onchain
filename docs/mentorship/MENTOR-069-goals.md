# MENTOR-069: Create Engagement Goals and Success Criteria

**Track:** Goals | **Component:** on-chain | **Status:** design

Goal: Create engagement goals and success criteria.

## On-chain

A goal per engagement holding its priority, owner, baseline, target value, target date, visibility and the agreement it was agreed under, plus the measurable success criteria attached to it.

## Off-chain boundary

The goal editor, the wording of each success criterion and the progress charts are served by the app.

## Rules

- A goal belongs to an agreement and cannot outlive it.
- Exactly one participant owns each goal; the other may be a collaborator but never a co-owner.
- Success criteria must be measurable, carrying a baseline and a target. A criterion with neither is rejected.
- Priority is explicit and determines the order goals are presented in.
- Visibility is set per goal. A private goal is visible only to its owner.
- Adding, amending or closing a goal is versioned, so progress recorded against an earlier version stays attributable.
- Goals are ordered by priority, and ties break on target date then creation order.

## Events and errors

Events `goal_created(agreement, goal)` and `goal_amended(agreement, goal, revision)`. Errors: `NotAuthorized`, `NotFound`, `AgreementClosed`, `CriterionNotMeasurable`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-069 work lives in the other ChainVerse repositories.