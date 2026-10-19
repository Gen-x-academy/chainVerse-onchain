# MENTOR-071: Create Mentorship Milestones

**Track:** Goals | **Component:** on-chain | **Status:** design

Goal: Create mentorship milestones.

## On-chain

A milestone per goal holding its order, due date, dependency ids, designated reviewer, completion rule and the optional payment amount linked to it.

## Off-chain boundary

The milestone planner, dependency visualisation and the reviewer's review screen are served by the app.

## Rules

- A milestone belongs to exactly one goal and its order within that goal is explicit.
- A dependency must reference a milestone in the same goal. Cycles are rejected at creation.
- A milestone cannot complete until every milestone it depends on is complete.
- The reviewer is designated at creation. Only that reviewer, or a coordinator, may accept a milestone.
- Completion rules are a closed set, so what "done" means for a milestone is never ambiguous at settlement time.
- A payment link is optional and, once set, references the amount from the agreement rather than storing an independent figure.
- A due date that has passed does not auto-fail a milestone; only the reviewer or a coordinator may close it.

## Events and errors

Events `milestone_created(goal, milestone)` and `milestone_completed(goal, milestone, reviewer)`. Errors: `NotAuthorized`, `GoalClosed`, `DependencyUnmet`, `DependencyCycle`, `MilestoneNotFound`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-071 work lives in the other ChainVerse repositories.