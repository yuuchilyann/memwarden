# memwarden

Windows memory analysis and management tool written in Rust.

> Status: early prototype. The current version is **read-only**: it reports
> memory usage and never modifies, suspends or terminates any process.

## Features (current)

- System physical memory, **commit charge**, cache and kernel pool usage
- Per-application memory (private bytes / working set), grouped by executable
  so multi-process apps such as Chrome or Electron show as one line

## Roadmap

1. Idle-process detection and classification (browsers, Electron/WebView2, VMs)
2. Commit-charge monitoring and alerts
3. Lowering memory priority of idle processes (`ProcessMemoryPriority`)
4. Optional working-set trimming for long-idle background apps
5. Browser companion extension for tab-level analysis and tab discarding

## Build

```powershell
cargo build --release
.\target\release\memwarden.exe
```

Requires Windows 10 or later. Run as administrator to read all processes.

## Privacy

memwarden output can include process names, file paths and user names of the
machine it runs on. Do not attach raw output to public issues without
reviewing it first. Generated reports (`reports/`, `snapshots/`, `*.dmp`)
are excluded by `.gitignore`.

## License

Licensed under either of MIT or Apache-2.0 at your option.
