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
- [ ] #108 Large paste hard-locks the app and wedges restart. The daemon writes unbounded input to the PTY while holding the window lock, so a filled PTY buffer deadlocks every other request including startup liveness checks. Bounded input queue, no blocking I/O under the lock, IPC frame cap. (L)
- [ ] #112 Subagents lack full scrollback. Paused/scrolled preview renders from a client-side stream cache instead of the daemon's canonical scrollback. Read full history on scroll-up. (S)
- [ ] #129 Worktree creation fails on stale directories. Creation only detects Git-registered worktree conflicts; classify stale states at the boundary and reattach, clean after confirmation, or refuse foreign paths with a clear modal. (M)

## Enhancements to implement

- [ ] #140 Move all tests out of app files. 87 inline `#[cfg(test)] mod tests` blocks (~60k lines) move to sibling `tests.rs` files following the existing convention. Four move-only PRs, landed early to give everything else a stable rebase base. Progress: action+tui merged in #145, git/mux/runtime merged in #147; top-level/config and app-core/handlers batches remain. (XL)
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
