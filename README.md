# memwarden

Windows memory dashboard written in Rust: see what is using your memory, and
do only the things that actually help.

> Status: early release. memwarden never suspends or force-quits processes.
> Every change it can make happens only when you ask, and everything except
> closing an app can be undone.

## Why

Task Manager shows memory right now; memory cleaners trim everything and
call it "freed". memwarden separates the two numbers that matter:

- **Physical memory** (RAM in use): when it runs out, Windows gets slow.
- **Commit charge** (memory programs have reserved): when it hits its limit,
  programs fail to allocate memory and may crash. Only programs releasing
  memory themselves lower it, so trimming does not help here, while closing
  an idle app or discarding browser tabs does.

## Features

- Live dashboard, refreshed every 2 seconds: physical memory, commit charge,
  cache and kernel pools, a 10-minute history, and per-application memory
  grouped by executable (multi-process apps such as Chrome or Electron show as
  one line; WebView2 helpers are counted under the app that hosts them).
- **Idle detection**: apps unused for 5 minutes, classified as browsers,
  web-based apps, services, VMs, system or security software. Apps playing or
  recording audio always count as in use.
- **Lower memory priority** of idle apps: Windows reclaims their memory first
  when RAM runs low. Restored when the app is used again, on request, when
  memwarden exits, or after a crash on the next start.
- **Trim working sets** of apps idle for 15+ minutes, with the amount actually
  moved out of RAM.
- **Browser tabs** (Chrome and Edge, with the companion extension): tab counts
  per browser and discarding idle tabs, with the browser's memory measured
  before and after. Discarded tabs stay in the tab strip and reload on click.
- **Close suggestions** for large apps idle for 30+ minutes, with the commit
  charge closing them would free. Closing is the same as clicking the window's
  X, confirmed one app at a time.
- **Memory growth tracking**: apps whose lowest private memory per minute keeps
  rising over hours (leaks, caches that never shrink), with how much
  reopening them would give back. memwarden needs to stay open for at least an
  hour to see this.
- **Page file info**: whether Windows can grow the page file, and how far the
  commit limit can go. A button opens the Windows settings to change it.
- UI languages: 繁體中文, English, 日本語, 한국어.

## Install

1. Download `memwarden-<version>-windows-x64.zip` from
   [Releases](../../releases) and extract it anywhere. Keep
   `memwarden-host.exe` in the same folder as `memwarden.exe`.
2. Run `memwarden.exe`. The executable is not code-signed yet, so Windows
   SmartScreen may warn on first start ("More info" → "Run anyway").
   Run it as administrator to read every process; otherwise protected
   processes (such as antivirus) are counted as unreadable.

Requires Windows 10 or later. Nothing is installed: delete the folder to remove
it (turn off browser integration first, see below).

### Browser extension (optional)

1. In memwarden, tick **Browser integration** in the header. This registers
   `memwarden-host.exe` with Chrome and Edge for the current user; unticking
   removes it again.
2. Open `chrome://extensions` (or `edge://extensions`), turn on Developer mode,
   choose **Load unpacked** and select the `extension` folder. Its ID is
   `jmngjfeoiaihgaabjklldhfmnaejnkga`.

Chrome warns that the extension can "read your browsing history". That comes
from the `tabs` permission, which the popup needs to list tab titles. Titles
and URLs never leave the extension: memwarden only receives tab counts. The
extension makes no network connections and loads no favicons.

## Build

```powershell
cargo build --release
.\target\release\memwarden.exe
.\scripts\package.ps1    # builds and writes dist\memwarden-<version>-windows-x64.zip
```

`cargo test` runs the unit tests of memwarden and the host;
`node --test "extension/tests/*.test.mjs"` tests the extension.
Developer notes are in [docs/HANDOFF.md](docs/HANDOFF.md) and
[docs/PLAN.md](docs/PLAN.md) (Traditional Chinese).

## Translations

UI text lives in [`locales/`](locales), one YAML file per language
(`en.yml`, `zh-TW.yml`, `ja.yml`, `ko.yml`). Missing keys fall back to English.
The extension's text is in [`extension/_locales/`](extension/_locales).

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

- memwarden, its host and the extension make no network connections. The
  extension talks to memwarden only through a local named pipe restricted to
  the current user.
- Process command lines are never displayed, logged or stored.
- memwarden writes only:
  - UI settings (language, window layout) under `%APPDATA%\memwarden`;
  - while priorities are lowered, a recovery file of numbers only (PID, start
    time, original priority) under `%LOCALAPPDATA%\memwarden`;
  - with browser integration on, the host manifest
    `%LOCALAPPDATA%\memwarden\com.memwarden.host.json` and two registry keys
    under `HKCU\Software\{Google\Chrome,Microsoft\Edge}\NativeMessagingHosts`.
- Its display can include process names of the machine it runs on. Review
  screenshots before attaching them to public issues.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option.
