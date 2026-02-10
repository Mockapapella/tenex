# Per-client Preview Viewport (Issue #98)

## Problem

With multiple Tenex clients connected to the same mux session, the preview can look fine on a large client while appearing clipped on a smaller one. Input still works, but the preview doesn’t render to the smaller client’s viewport.

## Goals

- No horizontal scrolling in the Preview pane.
- Each client renders Preview output to its own preview pane size (cols/rows).
- Cursor position in Preview is correct for each client while attached.
- Preserve existing “follow output vs. paused scrolling” behavior.

## Constraints

- A single PTY-backed program can only observe one terminal size at a time.
- **Resize policy B (chosen):** the PTY size for a target can grow to accommodate larger clients, but it never shrinks (monotonic max per target).

This means Tenex can provide per-client *rendering*, but programs that draw full-screen UIs will still be laid out for the shared PTY size.

## Design

### 1) Monotonic PTY resize (mux daemon)

The daemon tracks the maximum `cols`/`rows` ever requested per target (`session` or `session:index`). A resize request updates that maximum and only triggers an actual PTY resize when it increases either dimension.

### 2) Output streaming API (mux daemon ↔ client)

The daemon stores recent **raw PTY output bytes** per window with a monotonically increasing byte sequence number (`u64`).

- Each window maintains:
  - `seq_end`: the total number of output bytes observed so far.
  - `seq_start`: the earliest sequence number still available for streaming.
  - a bounded in-memory buffer of bytes for `[seq_start, seq_end)`.
  - an optional checkpoint `(checkpoint_seq, checkpoint_bytes)` used to resync clients that fall behind.
- When the buffer exceeds `OUTPUT_MAX_BYTES`, the daemon:
  - produces a checkpoint byte stream from the current terminal state (`vt100::Screen::state_formatted()`),
  - sets `checkpoint_seq = seq_end`,
  - clears the buffer and sets `seq_start = seq_end`.

#### IPC

Add an IPC request to pull output deltas:

- `ReadOutput { target, after, max_bytes }`

Responses:

- `OutputChunk { start, end, data_b64 }` where `data_b64` is base64 for the raw bytes in `[start, end)`.
- `OutputReset { start, checkpoint_b64 }` when `after < seq_start`. Clients must reset local state, process `checkpoint_b64`, set `after = start`, then resume reading chunks.

Base64 is used to avoid JSON “array-of-bytes” overhead.

### 3) Per-client terminal emulation (Tenex TUI)

Each Tenex client keeps a local `vt100::Parser` per target, sized to its preview pane.

Per-target state:

- `parser: vt100::Parser` (rows/cols = current preview pane size, scrollback = `DEFAULT_SCROLLBACK`)
- `after: u64` (last consumed output sequence number)
- `dims: (cols, rows)` (to detect local viewport changes)

On preview refresh:

1. Determine the mux target (`session` or `session:index`).
2. If local preview dimensions changed, recreate the parser and set `after = 0`.
3. Call `ReadOutput` in a loop to fetch/process output deltas.
   - On `OutputReset`, recreate the parser, process checkpoint bytes, set `after = start`.
4. Render Preview from the local parser:
   - while following: render the last ~300 lines (for smooth refresh),
   - while paused: render the full local scrollback.
5. Cursor position is read from the local parser’s screen.

Because parsing happens at the client’s pane size, wrapping and cursor positioning are naturally per-client and do not require horizontal scrolling.

## Behavior notes

- Full-screen terminal UIs may still look odd on smaller clients because the program is drawing for the shared PTY size.
- If a client falls behind enough to require `OutputReset`, the checkpoint is taken at the shared PTY size, so the reconstructed view may not perfectly match a continuously-following client.

