# memwarden

Windows memory dashboard in Rust (egui 0.37). Read [docs/HANDOFF.md](docs/HANDOFF.md)
before changing code and update [docs/PLAN.md](docs/PLAN.md) progress when a task is done.

Key rules (details in HANDOFF.md):
- Only `priority.rs` and `trim.rs` may change other processes, only on user action, leaving no state that cannot be undone.
  `bridge.rs` only forwards user-triggered tab discard requests to the browser extension.
  Open processes through `process::open_instance` (checks PID + start time); elsewhere use `PROCESS_QUERY_LIMITED_INFORMATION` only.
- Every user-visible string goes into all four `locales/*.yml` files.
- Never display, log or persist process command lines.
- Tab URLs and titles never leave the browser extension.
- egui 0.37 API differs from most online examples (see HANDOFF.md §6).
- `cargo build` must finish with no warnings.
