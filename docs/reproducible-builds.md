# Reproducible WASM Builds

## Overview

Smart contracts deployed on-chain are immutable once their WASM bytecode is uploaded. Anyone interacting with a ChainVerse contract — students, instructors, auditors — must be able to trust that the deployed bytecode corresponds exactly to the audited source code. This guarantee is only possible when the build process is **reproducible**: compiling the same source code twice, on two different machines or at two different times, must produce byte-for-byte identical WASM output.

Without reproducibility:

- A developer's local build could silently diverge from the CI build, making it impossible to verify what was actually deployed.
- An attacker who compromises a CI runner could substitute a malicious WASM without detection, because the output would differ from an independent build but there would be no automated check to catch it.
- Security auditors cannot confidently verify a deployed contract if they cannot reproduce the exact bytes that are on-chain.

Reproducible builds are a core supply-chain security property. The `reproducible-wasm` CI job enforces this automatically on every push and pull request.

---

## How the CI Check Works

The check lives in the `reproducible-wasm` job in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) and runs after `build-and-test` succeeds.

The job performs four logical phases:

1. **Build 1** — all contracts are compiled from a clean state:
   ```
   cargo build --locked --target wasm32-unknown-unknown --release
   ```
   The `--locked` flag ensures `Cargo.lock` is respected exactly, so no dependency version can silently drift.

2. **Hash Build 1** — SHA-256 digests are recorded for every `.wasm` file produced under `contracts/target/wasm32-unknown-unknown/release/` and saved to `/tmp/hashes_build1.txt`.

3. **Delete + Build 2** — the `.wasm` files are deleted (Rust intermediate artifacts are preserved so the second build only relinks, not recompiles, keeping CI fast). The contracts are compiled again with the identical command.

4. **Hash Build 2 + Diff** — SHA-256 digests are recorded to `/tmp/hashes_build2.txt`. The two hash files are compared with `diff`. If any hash differs, the job fails with an explicit error message explaining why non-determinism is a problem. If all hashes match, the job passes.

The hash file from build 1 is uploaded as a GitHub Actions artifact named **`wasm-hashes`** with a 30-day retention window. This artifact can be used to verify a deployed contract: download the artifact from the CI run that deployed the contract and compare its hashes against an independent local build.

---

## How to Reproduce Locally

The following commands replicate exactly what the CI job does. Run them from the repository root.

```sh
# 1. Ensure you are using the pinned toolchain (rust-toolchain.toml is read automatically)
rustup show   # should show 1.85.0 as the active toolchain

# 2. First build
cd contracts
cargo build --locked --target wasm32-unknown-unknown --release
cd ..

# 3. Hash the outputs
sha256sum contracts/target/wasm32-unknown-unknown/release/*.wasm | tee /tmp/hashes_build1.txt

# 4. Remove only the WASM outputs (keep Rust incremental cache)
find contracts/target/wasm32-unknown-unknown/release -name '*.wasm' -delete

# 5. Second build
cd contracts
cargo build --locked --target wasm32-unknown-unknown --release
cd ..

# 6. Hash again
sha256sum contracts/target/wasm32-unknown-unknown/release/*.wasm | tee /tmp/hashes_build2.txt

# 7. Compare — no output means identical
diff /tmp/hashes_build1.txt /tmp/hashes_build2.txt && echo "Reproducible ✓" || echo "NOT reproducible ✗"
```

To verify a deployed contract against a known-good CI hash file:

```sh
# Download wasm-hashes artifact from the relevant CI run, then:
sha256sum contracts/target/wasm32-unknown-unknown/release/<contract_name>.wasm
# Compare the printed hash against the value in the downloaded hashes_build1.txt
```

---

## Toolchain and Flags

### `rust-toolchain.toml`

The repository pins an exact Rust toolchain via [`rust-toolchain.toml`](../rust-toolchain.toml):

```toml
[toolchain]
channel = "1.85.0"
targets = ["wasm32-unknown-unknown"]
components = ["rustfmt", "clippy"]
profile = "minimal"
```

`rustup` reads this file automatically whenever any `cargo` or `rustc` command is run inside the repository. This means every developer and every CI runner uses the exact same compiler version, which is one of the most important factors for reproducibility — even minor patch releases of `rustc` can change codegen.

### `--locked`

`cargo build --locked` refuses to run if `Cargo.lock` does not match `Cargo.toml`. This prevents a scenario where a dependency is silently updated between two builds, which would change the compiled output. The lock file is committed to the repository and must be kept up to date by developers.

### `--target wasm32-unknown-unknown`

Soroban contracts run in a WASM virtual machine on the Stellar network. Compiling to `wasm32-unknown-unknown` produces a bare 32-bit WASM binary with no OS assumptions, which is the format Soroban expects. This target must be installed via `rustup target add wasm32-unknown-unknown` (the toolchain file handles this automatically in CI via the `targets` field).

### `--release`

Release mode enables LLVM optimizations (`opt-level = 3` by default) and strips debug symbols. Only release builds are deployed on-chain, so only release builds are checked for reproducibility. Debug builds are intentionally excluded because they embed file paths and other local-machine artifacts that are inherently non-reproducible.

---

## What to Do If Builds Differ

If the `reproducible-wasm` CI job fails with `ERROR: WASM artifacts are not reproducible`, work through the following checklist:

**1. Check for non-deterministic proc-macro or build script output.**
Some build scripts (`build.rs`) embed timestamps, git commit hashes, or random values. Search for calls to `std::time`, environment variables like `SOURCE_DATE_EPOCH`, or `git describe` in any `build.rs` file in the dependency tree. The `cargo build -vv` flag prints build script output and can help identify the offending crate.

**2. Check for embedded timestamps in generated code.**
Some code-generation crates write the current time into the output. Audit any crate that uses proc-macros or generates Rust source files at build time.

**3. Verify `Cargo.lock` is committed and up to date.**
Run `cargo generate-lockfile` and check if `Cargo.lock` changes. If it does, a dependency was added or updated without regenerating the lock file. Commit the updated lock file and re-run CI.

**4. Ensure the pinned toolchain is active.**
Run `rustup show` and confirm the active toolchain is `1.85.0`. If your local environment uses a different toolchain, the compiler may produce different output. The `rust-toolchain.toml` file should handle this automatically, but it can be overridden by `RUSTUP_TOOLCHAIN` or a parent-directory `rust-toolchain.toml`.

**5. Check for `RUSTFLAGS` differences.**
Non-default `RUSTFLAGS` (e.g., `-C target-cpu=native`) can produce machine-specific output. Ensure no such flags are set in `.cargo/config.toml`, environment variables, or shell profiles.

**6. Isolate which contract changed.**
The diff output will identify the specific `.wasm` file whose hash changed. Narrow the investigation to the corresponding crate and its direct build-time dependencies.

**7. Use `cargo vendor` for deep audits.**
If the source of non-determinism is in a third-party crate, `cargo vendor` can snapshot the full source tree so it can be inspected and compared between builds.

If you cannot identify the root cause, open an issue with the diff output from the failing CI run attached.
