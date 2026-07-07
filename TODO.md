# Open-issue work plan

Triage of every open issue against master `3a2a408` on 2026-07-06. Each item tracks to closure, either a merged PR or a close-with-evidence comment on the issue. Checked items are done.

## Already fixed on master, close with evidence

- [x] #113 Long-running unviewed agents cause lag. Fixed by cursor-based activity polling (`7080ef2`): non-selected agents are polled via cheap output-cursor sequence checks, preview reads only the selected agent, and daemon buffers are capped. Closed with evidence; a request-count regression test lands with the mux cluster below.
- [x] #122 Crash/reboot does not renew old state. Child respawn from persisted state and conversation resume are both implemented on master (respawn walk in `src/app/handlers/sync.rs`, persisted `conversation_id` resume in `src/conversation.rs` / `src/runtime/mod.rs`). Branch switch deleting children is by design. Closed with evidence.

## Bugs to fix

- [x] #124 Review agents always compare against main/master. The base branch is now typed into Codex's `/review` picker instead of bracketed-pasted, with post-start verification of the exact selected base and a visible warning on colliding-name mismatches. Merged in #144. (S)
- [x] #102 Terminal agents have no tab-complete. Attached preview dispatch swallowed `Tab`/`BackTab` before forwarding; both are now forwarded while attached, with docs updated. Merged in #143. (S)
- [x] #121 Renaming an agent breaks remote tracking. Rename no longer touches the remote (git branch -m migrates tracking natively); push and open-PR flows are now upstream-aware, including ls-remote verification against stale tracking refs. Merged in #146. (M)
- [ ] #110 Stale PTY size after restart on a smaller screen. The daemon clamps every resize to the historical maximum. Make resize mean the exact requested size. (S)
- [x] #108 Large paste hard-locks the app and wedges restart. Fixed with a bounded per-window input queue drained by a writer pump (no blocking I/O under the window lock), atomic whole-send accept-or-reject semantics, graceful terminal-query degradation under backpressure, a 16 MiB validated IPC frame cap, and surfaced send errors. Merged in #149. (L)
- [ ] #112 Subagents lack full scrollback. Paused/scrolled preview renders from a client-side stream cache instead of the daemon's canonical scrollback. Read full history on scroll-up. (S)
- [x] #129 Worktree creation fails on stale directories. Fixed with a boundary classifier: registered worktrees reattach via the existing modal, stale empty or reciprocally-proven Tenex-owned directories are cleaned and recreated with a status notice, and symlinks, files, foreign, or ambiguous paths are refused with specific errors. Merged in #150. (M)

## Enhancements to implement

- [ ] #140 Move all tests out of app files, into the top-level `tests/` directory, organized. Corrected scope per owner review of #147: sibling `src/**/tests.rs` files are not the end state; all tests leave `src/` entirely. PRs #145/#147 (inline -> sibling extraction) stand as step one; a design pass now covers the `tests/` organization, the `test-support` feature surface for tests that need non-public access, and the main.rs/binary strategy, followed by gated migration batches until `src/` holds no test code. (XL)
- [ ] #12 Selective synthesis. Mark agents in the sidebar; `s` synthesizes marked descendants and falls back to all when nothing is marked. (M)
- [ ] #100 Ctrl+f project picker. Searchable picker of discovered git repos to spawn agents in other projects; multi-project support already exists in core. (M)
- [ ] #141 Property-based tests wherever a test can be property-based. proptest is a declared dev-dependency with zero uses today. Add property tests for the highest-value invariant surfaces: mux wire-protocol roundtrip, IPC framing, input-state cursor invariants, scroll clamping, branch-name generation, output decoders, and more. (L)
- [ ] #126 Add an API. Local control endpoint that exposes every keypress-evocable action through one canonical, validated wire contract with a generated OpenAPI-style schema. Lands after the behavior bugs so the API does not freeze buggy semantics. (XL)

## Coverage issues stay open for now

- [ ] #137 / #133 True theoretical-max coverage. Receipts are green on all three platforms, but `3fa2202` reached that state partly by excluding 23 src files (18 of them whole-file) from instrumentation via `cfg_attr(coverage_nightly, coverage(off))`; the measured set is 81 of 148 src files. The remaining work is removing those exclusions and genuinely covering the excluded modules, or explicitly justifying a minimal documented subset such as probe binaries. This is the capstone item. (XL)

## Merge order

1. Close #113 and #122 with evidence; land the small fixes #124, #102, #121.
2. #140 test extraction, four move-only PRs.
3. Mux cluster in dependency order: #108, then #110, then #112 plus the #113 regression tests.
4. #129, then #12 and #100.
5. #141 property tests.
6. #126 API.
7. #137/#133 coverage de-exclusion capstone.

Every PR runs the full pre-commit suite, regenerates cross-OS coverage receipts, and merges only with CI green on Linux, macOS, and Windows WSL2. No new `coverage(off)`, no lint-silencing attributes, no test-weakening.
