# Scholarship Service-Level Indicators

The `scholarship-sli` Soroban contract maintains bounded daily aggregates for
operational measurements across scholarship programs:

| Metric | Interval represented |
| --- | --- |
| `Operation` | Domain operation start to result; failed samples contribute to the error count |
| `QueueAge` | Queue insertion to processing |
| `NotificationLag` | Notification enqueue to provider acknowledgment |
| `PaymentFinalization` | Payment intent open to finalization |
| `Reconciliation` | Reconciliation start to observation, plus expected/actual amount drift |

## ABI

- `initialize(admin, recorder)`: one-time setup by an authorized admin.
- `set_recorder(admin, recorder)`: rotate the trusted measurement recorder.
- `record_sample(...)`: record an idempotent measurement for a program and
  metric. The event timestamp is bucketed by UTC day.
- `get_aggregate(program_id, metric, day)`: read a daily summary.

Each `SliAggregate` contains sample and failure counts, total and maximum
duration, and reconciliation sample count plus signed net drift and total and
maximum absolute drift in token base units. For non-reconciliation metrics,
amount arguments must be zero. `observed_at` cannot be after the current ledger timestamp,
`started_at` cannot be later than `observed_at`, and durations are capped at
90 days. There can be at most 50,000 samples for a program/metric/day.
The contract also caps all programs and metrics at 100,000 samples per UTC day.

The `SLIUPD` event announces the program, metric, day, and aggregate only.
Individual sample IDs are used for duplicate rejection but are never emitted.

## Measurement trust boundary

The recorder is a trusted operations service. It submits source-event
timestamps and outcomes from scholarship domain contracts, the durable
notification outbox/provider acknowledgments, payment intents, and settlement
reconciliation. The SLI contract validates timestamps, deduplicates samples,
and computes aggregate durations/error counts/drift; it cannot independently
verify a provider acknowledgment or an off-chain observation. Recorder keys
must therefore be tightly controlled and rotated by governance.

The contract stores no applicant, recipient, payment intent, or notification
payload. Per-sample IDs are persisted only as deduplication keys and must be
opaque high-entropy commitments that cannot be correlated with a person or
payment from their value. `program_id` should likewise be an opaque commitment.
Soroban ledger timestamps and this recorder attestation are the measurement
boundary; richer percentile distributions, provider-specific breakdowns,
alerting, and incident annotations remain off-chain.

Daily aggregates and deduplication markers use bounded TTLs and may expire
after the retention horizon. The operations indexer should retain event
history and export aggregates off-chain before expiry.
