# `library-rights` resource budgets

Tracks issue #1017.

A WASM size cap says nothing about what an individual call costs. Soroban
rejects a transaction at submission time when it exceeds the per-transaction
CPU-instruction or memory limit, so an entrypoint that quietly grows past its
share is a production outage rather than a slow test.

The gate lives in `contracts/library-rights/src/tests/budgets.rs` and runs in
CI as the **Resource budgets (library-rights)** step of
`.github/workflows/contracts.yml`.

## How a budget is measured

Each test resets the host budget with `env.cost_estimate().budget().reset_default()`,
makes exactly one call, and reads back `cpu_instruction_cost()` and
`memory_bytes_cost()`. Every entrypoint is exercised twice where the ABI allows
a variable-size input: once at a nominal call and once at the largest input the
ABI admits (for example a 32-character `Symbol` and `u32::MAX` limits for
`put_policy`).

Rejection paths are measured too. A call that fails must not be dramatically
cheaper than the one it guards, or it becomes an attractive thing to spam.

## Where the ceilings come from

Soroban's per-transaction limits are **100,000,000 CPU instructions** and
**41,943,040 memory bytes** (40 MiB). A single library call has to leave room
for the rest of the transaction, so each entrypoint gets a fraction of the
network limit according to its class:

| Class     | Share | CPU ceiling | Memory ceiling | Applies to                                     |
|-----------|-------|-------------|----------------|------------------------------------------------|
| `Read`    | 5 %   | 5,000,000   | 2,097,152      | One or two storage reads, no cross-contract     |
| `Write`   | 10 %  | 10,000,000  | 4,194,304      | Storage write plus TTL extension and an event   |
| `Complex` | 25 %  | 25,000,000  | 10,485,760     | Multi-key writes, cross-contract calls, or loops |

A separate aggregate test asserts that a realistic five-call lending cycle
(`place_hold` → `claim_hold` → `return_work` → `borrow_work` → `return_work`)
fits inside one transaction's full network limit.

## Tolerance, and why it is generous

The ceilings are deliberately loose. This gate exists to catch an
order-of-magnitude regression — an accidental unbounded loop, a per-call clone
of a growing collection, a cross-contract call added to a hot path — not to
police single-digit percentage drift, which is noisy between SDK versions
because the host's own cost model changes.

`Budget::cpu_instruction_cost` is documented to **underestimate** native
execution relative to WASM. A native measurement that already breaches its
ceiling is therefore unambiguously a real regression, never a measurement
artefact.

## Current class assignments

| Entrypoint                       | Class     | Notes                                          |
|----------------------------------|-----------|------------------------------------------------|
| `bootstrap`                      | `Complex` | Writes five role keys in one call               |
| `put_policy` (insert/update/max) | `Write`   | Update path re-reads before writing             |
| `get_role`                       | `Read`    |                                                 |
| `get_policy`                     | `Read`    |                                                 |
| `put_work` (insert/overwrite)    | `Write`   | Reads the policy to validate the link           |
| `get_work`                       | `Read`    |                                                 |
| `borrow_work`                    | `Write`   | Hot path                                        |
| `return_work` (clear and no-op)  | `Write`   | The idempotent no-op is measured separately     |
| `place_hold`                     | `Write`   | Hot path                                        |
| `claim_hold`                     | `Complex` | Writes a loan, removes the hold, two events     |

## Changing a ceiling

Raising one is a deliberate, reviewable act.

1. Change the constant in `contracts/library-rights/src/tests/budgets.rs`.
2. Say in the PR **what** made the call more expensive and **why** that is
   acceptable against the network limit.

Do not raise a ceiling to make CI green. If a call genuinely needs a larger
share, that is a design conversation about the entrypoint, not a test fix.

## Adding an entrypoint

Any new entrypoint should arrive with a budget test in the same PR. Pick the
class from the table above by what the call actually touches, not by what it is
named.

## Impact

Test and CI only. No ABI, storage, event, privacy, deployment, or migration
impact: no entrypoints are added or changed and no new storage keys are written.
