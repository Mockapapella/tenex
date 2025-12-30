# Pre-commit and CI Alignment Specification

## Executive Summary

Align the pre-commit hooks with CI so that any commit passing pre-commit will also pass CI. Currently, CI is stricter in several ways:

1. **Clippy**: CI fails on warnings (`-D warnings`); pre-commit does not
2. **Rustfmt**: CI checks formatting; pre-commit auto-formats (different behavior)
3. **Coverage flags**: CI uses `--all-targets`; pre-commit alias uses `--workspace`
4. **Rust version**: CI pins 1.91.1; pre-commit uses system default

After this change, pre-commit will be byte-for-byte equivalent to CI in strictness.

---

## Requirements

### Functional Requirements

| ID | Requirement | Rationale |
|----|-------------|-----------|
| FR-1 | Pre-commit rustfmt must fail if code is not formatted (no auto-formatting) | Match CI's `--check` behavior |
| FR-2 | Pre-commit clippy must fail on any warning | Match CI's `-D warnings` flag |
| FR-3 | Pre-commit coverage must use `--all-targets` flag | Match CI's coverage command |
| FR-4 | Project must pin Rust version to 1.91.1 | Ensure consistent behavior across dev and CI |
| FR-5 | All pre-commit hooks must run on every commit | User preference for strict enforcement |
| FR-6 | `cargo-check` hook must be retained | User preference, despite redundancy with clippy |

### Non-Functional Requirements

| ID | Requirement | Rationale |
|----|-------------|-----------|
| NFR-1 | Bypassing pre-commit with `--no-verify` is never allowed | Organizational policy |
| NFR-2 | Hook execution order: fmt -> clippy -> check -> test -> coverage | Fail fast on simple issues |

---

## Gap Analysis

### Current State vs. Target State

| Check | CI Command | Current Pre-commit | Gap | Target Pre-commit |
|-------|------------|-------------------|-----|-------------------|
| Rustfmt | `cargo fmt --all -- --check` | `cargo fmt --all && git add -u` | Auto-format vs check-only | `cargo fmt --all -- --check` |
| Clippy | `cargo clippy --all-targets --all-features -- -D warnings` | `cargo clippy --all-targets --all-features --workspace` | Missing `-D warnings` | `cargo clippy --all-targets --all-features --workspace -- -D warnings` |
| Check | (subsumed by clippy) | `cargo check --all-targets --all-features --workspace` | N/A (keeping per user request) | No change |
| Test | `cargo test --all-targets --all-features -- --test-threads=1` | `cargo test --all-targets --all-features --workspace -- --test-threads=1` | Equivalent | No change |
| Coverage | `cargo llvm-cov --all-targets --all-features --profile coverage --fail-under-lines 90 --fail-under-functions 90 -- --test-threads=1` | `cargo cov` (alias: uses `--workspace` not `--all-targets`) | Missing `--all-targets` | Update alias to use `--all-targets` |
| Rust Version | 1.91.1 (via dtolnay/rust-toolchain) | System default | No enforcement | Add `rust-toolchain.toml` |

---

## Implementation

### File Changes

#### 1. Create `rust-toolchain.toml`

**Path:** `/home/quinten/Documents/Software_Development/tenex/rust-toolchain.toml`

```toml
[toolchain]
channel = "1.91.1"
components = ["clippy", "rustfmt", "llvm-tools-preview"]
```

#### 2. Update `.pre-commit-config.yaml`

**Path:** `/home/quinten/Documents/Software_Development/tenex/.pre-commit-config.yaml`

```yaml
repos:
  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt --check
        entry: cargo fmt --all -- --check
        language: system
        types: [rust]
        pass_filenames: false

      - id: cargo-clippy
        name: cargo clippy (all targets, all features, deny warnings)
        entry: cargo clippy --all-targets --all-features --workspace -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false

      - id: cargo-check
        name: cargo check (all targets, all features)
        entry: cargo check --all-targets --all-features --workspace
        language: system
        types: [rust]
        pass_filenames: false

      - id: cargo-test
        name: cargo test
        entry: cargo test --all-targets --all-features --workspace -- --test-threads=1
        language: system
        types: [rust]
        env:
          TENEX_STATE_PATH: /tmp/tenex-pre-commit-state.json
        pass_filenames: false

      - id: cargo-coverage
        name: cargo llvm-cov (90% lines + functions)
        entry: cargo cov
        language: system
        types: [rust]
        env:
          TENEX_STATE_PATH: /tmp/tenex-pre-commit-state.json
        pass_filenames: false
```

#### 3. Update `.cargo/config.toml`

**Path:** `/home/quinten/Documents/Software_Development/tenex/.cargo/config.toml`

Change the `cov` alias from:
```toml
cov = "llvm-cov --workspace --all-features --profile coverage --fail-under-lines 90 --fail-under-functions 90 -- --test-threads=1"
```

To:
```toml
cov = "llvm-cov --all-targets --all-features --profile coverage --fail-under-lines 90 --fail-under-functions 90 -- --test-threads=1"
```

---

## Verification

After implementation, verify alignment by running:

```bash
# Pre-commit should now match CI behavior
pre-commit run --all-files

# Compare with CI commands directly
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features --workspace
cargo test --all-targets --all-features -- --test-threads=1
cargo llvm-cov --all-targets --all-features --profile coverage --fail-under-lines 90 --fail-under-functions 90 -- --test-threads=1
```

---

## Policy

### Bypass Policy

**Bypassing pre-commit hooks with `git commit --no-verify` is never allowed.**

All commits pushed to the repository must pass all pre-commit checks. If a developer uses `--no-verify`, CI will catch the issue and block the PR.

---

## Out of Scope

- Platform-specific considerations (pre-commit runs on developer's OS; CI tests Linux/macOS/Windows separately)
- Timeout handling for long-running hooks
- Feature flag variations (all checks use `--all-features`)
- Pre-push hooks (only pre-commit hooks are covered)

---

## Open Questions

None. All requirements have been clarified through the interview process.
