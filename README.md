# memwarden

Windows memory dashboard written in Rust.

> Status: early prototype. The current version is **read-only**: it reports
> memory usage and never modifies, suspends or terminates any process.

## Features (current)

- Live dashboard (egui), refreshed every 2 seconds
- System physical memory, **commit charge**, cache and kernel pool usage,
  with a warning when commit charge approaches its limit
- 10-minute usage history chart
- Per-application memory (private bytes / working set), grouped by executable
  so multi-process apps such as Chrome or Electron show as one line, with filter
- UI languages: 繁體中文, English, 日本語, 한국어

## Roadmap

1. Idle-process detection and classification (browsers, Electron/WebView2, VMs)
2. Lowering memory priority of idle processes (`ProcessMemoryPriority`)
3. Optional working-set trimming for long-idle background apps
4. Browser companion extension for tab-level analysis and tab discarding

## Build

```powershell
cargo build --release
.\target\release\memwarden.exe
```

Requires Windows 10 or later. Run as administrator to read all processes.

## Translations

UI text lives in [`locales/`](locales), one YAML file per language
(`en.yml`, `zh-TW.yml`, `ja.yml`, `ko.yml`). Missing keys fall back to English.

To add a language:

1. Copy `locales/en.yml` to `locales/<code>.yml` and translate the values.
   Keep `%{name}` placeholders unchanged.
2. Add the language to `Lang` in [`src/i18n.rs`](src/i18n.rs).
3. If the script needs a font egui does not ship, add a Windows system font
   for it in `set_fonts` in [`src/theme.rs`](src/theme.rs).

Preview a language without changing your saved choice:

```powershell
$env:MEMWARDEN_LANG = "ja"; cargo run
```

On first launch the language follows the Windows display language; a choice
made in the language picker is remembered.

## Privacy

- memwarden makes no network connections.
- It stores only UI settings (language, window layout) under
  `%APPDATA%\memwarden`.
- Its display can include process names of the machine it runs on. Review
  screenshots before attaching them to public issues. Generated reports
  (`reports/`, `snapshots/`, `*.dmp`) are excluded by `.gitignore`.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option.
