# Scholarship domain test coverage

Implements issue [#1146](https://github.com/Gen-x-academy/chainVerse-onchain/issues/1146)
(unit tests) and, in a stacked follow-up,
[#1147](https://github.com/Gen-x-academy/chainVerse-onchain/issues/1147)
(integration tests).

## Why this branch is stacked

This branch is based on `devbackend513-ux`'s `feature/scholarship-test-suites`
(#1174), not on `main`. On `main` these contracts do not build:
`scholarship-applications` has duplicate error definitions and the
`scholarship-core`/`eligibility`/`programs` test modules are missing `Ledger`
testutils imports. #1174 fixes that.

Rather than duplicate ~30 files of repairs into a second PR, this work inherits
them. **Merge #1174 first.** GitHub will retarget this PR to `main` and show
only these commits once #1174 lands.

## What #1146 asked for, and what it found

#1146 requires that "every state transition and typed error is exercised".
Measuring that first, rather than writing tests and assuming, turned up
something worth reporting on its own:

**20 of the 55 declared error variants across the five contracts were asserted
by no test.** After this change, that number is **0**.

The 20 are now covered, but three of them turned out to be **unreachable** --
declared in the ABI and returned by no code path. That is a finding about the
contracts, not about the tests, and it is the sort of thing #1146 exists to
surface.

| Contract | Unreachable variant | Why |
| --- | --- | --- |
| `scholarship-core` | `NotAdmin` | no return site. Authorization is per-program (`NotProgramOwner`); the `Admin` key written by `initialize` is read back only to detect a repeat `initialize`, never to gate a call |
| `scholarship-core` | `NotInitialized` | no return site at all -- `initialize` raises `AlreadyInitialized` instead, and nothing else checks initialization |
| `scholarship-programs` | `BudgetExceeded` | present in source but logically dead; see below |

`NotInitialized` in `scholarship-core` is a different situation from
`NotInitialized` in the other four contracts. In those, it is raised on the
admin path and merely absent from the read paths -- which is the next section.
In `scholarship-core` it is never raised at all.

### `BudgetExceeded` cannot be reached

`reserve_award` returns `BudgetExceeded` when a new commitment would pass
`total_budget`. That cannot happen:

1. `configure_award_budget` refuses `max_recipients × per_award > total_budget`.
2. `reserve_award` stops at `awarded_count == max_recipients`, so
   `committed_amount ≤ max_recipients × per_award`.
3. Together: `committed_amount ≤ total_budget`, always. The `checked_add`
   cannot overflow either, since `total_budget` is a valid `i128`.
4. Reconfiguration is refused once anything is committed
   (`BudgetAlreadyCommitted`), so the invariant cannot be re-cut afterwards.

The check is cheap defence-in-depth and worth keeping. But it cannot be
tested, because no input reaches it. `test_budget_exceeded_cannot_be_reached_because_configuration_precludes_it`
asserts the *invariant* that makes it unreachable, which is the only honest way
to cover a branch that cannot be entered.

If the intent is for `BudgetExceeded` to be reachable, the fix is to drop the
`required > total_budget` rejection at configuration time -- which is a
product decision, not a test one, so it is not made here.

### Uninitialized reads are indistinguishable from missing records

`NotInitialized` guards the *admin* path in `scholarship-programs`,
`scholarship-eligibility`, `scholarship-applications`, and
`scholarship-registry`. Read paths are unguarded. So before `initialize`:

| Call | Returns |
| --- | --- |
| `get_program_window` | `WindowNotFound` |
| `get_latest_rule_version` | `NoRulePublished` |
| `get_form_schema` | `FormSchemaNotFound` |
| `get_module` (registry) | `ModuleNotFound` |

Both readings are true, and the second is the more actionable one. But an
operator debugging a fresh deployment sees a "not found" that looks like a
misconfiguration rather than a missing `initialize`. Adding a guard to the read
paths would be an ABI behaviour change, so it is documented rather than made.

## Design notes

**In-crate, not `tests/`.** These files live at `src/error_tests.rs` and reach
the contracts' private `DataKey` enums. That is deliberate: `VersionOverflow`
is only reachable by seeding a version counter at `u32::MAX`, since the counter
is only ever advanced by a `checked_add` in a publish call. Four billion
publishes is not a test. Seeding also lets the tests assert that a *refused*
write left storage untouched -- the property that actually matters for an
overflow guard.

Storage is only reachable through `env.as_contract(&id, ..)`, which is the
contract's own authority, so a test cannot write a key the contract could not.

**Coverage ledgers are enforced, not decorative.** Each file ends with
`test_every_error_variant_is_accounted_for`, which requires each declared
variant to be classified as covered-here, covered-in-`tests.rs`, or
unreachable-with-a-reason -- and requires the counts to add up, so a variant
cannot be double-counted. `test_declared_ledger_matches_the_contract` reads
`lib.rs` with `include_str!` and fails if the ledger names a variant the
contract does not declare. Adding an error variant without updating the ledger
breaks the build rather than silently reducing coverage.

This caught two mistakes during development, both of which had been written
from memory rather than read from the source: a variant named `InvalidRule`
that does not exist (the real names are `EmptyRule`, `RuleTooLarge`,
`NoRulePublished`), and a claim that `get_latest_rule_version` returns
`RuleNotFound` when it returns `NoRulePublished`.

**Table-driven where there is a boundary.** The description-length bound, the
`u64` window-overflow boundary, the i128 budget boundary, and the version
counters are each driven from one table with the accept/refuse edge explicit,
so an off-by-one is visible rather than incidental. Each table also asserts the
*reason* for stopping, not just that it stopped: the budget table checks
`committed ≤ total` after every case, so a test cannot pass by exhausting
capacity when it meant to exhaust budget.

## Results

140 tests across the five contracts, up from 91. Clippy clean under
`-D warnings`; `rustfmt` clean.

| Contract | Before | After |
| --- | --- | --- |
| `scholarship-applications` | 30 | 39 |
| `scholarship-core` | 13 | 21 |
| `scholarship-eligibility` | 18 | 29 |
| `scholarship-programs` | 19 | 34 |
| `scholarship-registry` | 11 | 17 |
| **total** | **91** | **140** |
| **unasserted error variants** | **20** | **0** |

The before/after counts and the 20-to-0 variant figure were both measured
against `c5bcbb3` rather than estimated.

## Ownership, privacy, migration

No production code changed except five one-line `mod error_tests;`
declarations, so ownership, privacy, and migration impact are nil: no
authorization, storage layout, or public interface is touched. Nothing in these
tests writes to a shared external resource; every test builds its own `Env` and
contract instance.
