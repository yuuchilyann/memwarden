# memwarden

Windows memory dashboard in Rust (egui 0.37). Read [docs/HANDOFF.md](docs/HANDOFF.md)
before changing code and update [docs/PLAN.md](docs/PLAN.md) progress when a task is done.

Key rules (details in HANDOFF.md):
- Only `priority.rs` and `trim.rs` may change other processes, only on user action, leaving no state that cannot be undone.
  `bridge.rs` only forwards user-triggered tab discard requests to the browser extension.
  `close.rs` only posts `WM_CLOSE` to an app's top-level windows after the user confirms that one app; never terminate processes.
  Only `integration.rs` writes the registry (two HKCU Native Messaging keys), only when the user toggles it.
  Open processes through `process::open_instance` (checks PID + start time); elsewhere use `PROCESS_QUERY_LIMITED_INFORMATION` only.
- Every user-visible string goes into all four `locales/*.yml` files.
- Never display, log or persist process command lines.
- Tab URLs and titles never leave the browser extension.
- egui 0.37 API differs from most online examples (see HANDOFF.md §6).
- `cargo build` must finish with no warnings.
