# memwarden

Windows memory dashboard in Rust (egui 0.37). Read [docs/HANDOFF.md](docs/HANDOFF.md)
before changing code and update [docs/PLAN.md](docs/PLAN.md) progress when a task is done.

Key rules (details in HANDOFF.md):
- Only `priority.rs` may change other processes, only on user action, and every change must be reversible.
  Everywhere else open processes with `PROCESS_QUERY_LIMITED_INFORMATION` only.
- Every user-visible string goes into all four `locales/*.yml` files.
- Never display, log or persist process command lines.
- egui 0.37 API differs from most online examples (see HANDOFF.md §6).
- `cargo build` must finish with no warnings.
