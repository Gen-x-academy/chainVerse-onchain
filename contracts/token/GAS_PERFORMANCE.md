# Gas & Performance Profiling for AcademyVestingContract

## Benchmark Results

- Use the benchmarking scripts in `gas_bench.rs` to measure gas usage for:
  - `grant_vesting`
  - `claim`

## Worst-case Gas Estimates

- `grant_vesting`: Worst-case is when storage is empty and a new schedule is created (includes storage write, event emission).
- `claim`: Worst-case is when a schedule is claimed for the first time (includes storage read/write, token transfer, event emission).

> Actual gas numbers should be filled in after running the benchmarks in your environment.

## Optimization Notes

- Storage access is minimized by batching reads/writes where possible.
- All state changes are atomic to avoid redundant storage operations.
- No unnecessary computation in hot paths.

## Regression Testing

- All existing tests in `test.rs` cover the main contract logic.
- Add new tests if further optimizations are applied.

---

**To update this file:**
- Run the benchmarks and record the gas usage for each entrypoint.
- Update the worst-case gas numbers above.
