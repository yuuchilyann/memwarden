# memwarden 接手文件

最後更新：2026-10-31 · 版本 0.1.0

開發進度與下一步請見 [PLAN.md](PLAN.md)。

## 1. 專案概要

Windows 記憶體儀表板（Rust + egui）。可以找出閒置的應用程式，並在使用者觸發時降低它們的記憶體優先權，
讓 Windows 在記憶體不足時優先回收；不會暫停或結束任何程序，所有變更都可還原。

## 2. 環境

| 項目 | 版本 / 說明 |
|---|---|
| 作業系統 | Windows 10 以上（開發機：Windows 11 Pro） |
| Rust | 1.99（edition 2024） |
| 主要套件 | `eframe`/`egui` 0.37、`egui_plot` 0.38、`egui_extras` 0.37、`windows` 0.62、`rust-i18n` 4、`serde_json` 1 |
| Git 作者 | `見未亞由 <yuuchilyann@hotmail.com>`（已由專案擁有者同意公開） |

## 3. 建置與執行

Cargo workspace：根目錄是 memwarden，`host/` 是 Native Messaging host（`memwarden-host.exe`）。
`default-members` 包含兩者，所以 `cargo build`／`cargo test` 會一起建置與測試。

```powershell
cargo build                     # 偵錯版，會顯示主控台
cargo build --release           # 發佈版，無主控台視窗（LTO，約 4–7 分鐘）
.\target\release\memwarden.exe
$env:MEMWARDEN_LANG = "ja"; cargo run   # 強制指定語言（zh-TW / en / ja / ko）
```

以系統管理員身分執行可讀取全部程序；否則受保護程序（例如防毒軟體）會被計入「無法讀取」。

瀏覽器擴充功能（M6）的安裝：

1. 在 memwarden 標題列勾選「瀏覽器整合」（寫入 HKCU 登錄與 `%LOCALAPPDATA%\memwarden\com.memwarden.host.json`，
   取消勾選即全部刪除）。`memwarden-host.exe` 必須和 memwarden.exe 在同一個資料夾，`cargo build` 會一起產生。
2. 到 `chrome://extensions`（或 `edge://extensions`）開啟開發人員模式，「載入未封裝項目」選 `extension\`，
   確認 ID 是 `jmngjfeoiaihgaabjklldhfmnaejnkga`。

**注意**：Claude desktop app 是 MSIX 套件，從它（包含它的終端機面板）啟動的程式寫入 `HKCU\Software` 與
`%LOCALAPPDATA%` 會被虛擬化，瀏覽器看不到，擴充功能會一直顯示「請在 memwarden 開啟瀏覽器整合」
（錯誤訊息 `Specified native messaging host not found`）。要測試瀏覽器整合時，memwarden 請從一般終端機啟動。

## 4. 架構

```
sampler 執行緒（每 2 秒）                       UI 執行緒（eframe）
  system::query()                ─┐              App::ui()
  Collector::list()  → procs      │                drain_samples() → history
  classify::group(procs) → apps   ├ Snapshot ─mpsc─▶ header / kpi_row / history_card
  Tracker::update(procs, apps,    │                top_card / apps_card（篩選、類別、狀態）
                  foreground)    ─┘
```

```
瀏覽器擴充功能 ──stdio── memwarden-host.exe ──named pipe── bridge 執行緒（每條連線一個）
                         （每個瀏覽器設定檔一個）              Bridge::browsers() → apps_card 的分頁列
```

`Collector` 會快取每個程序實例 `(pid, 啟動時間)` 的 Chromium 旗標，命令列只讀一次。
`Tracker` 以程序實例計算 CPU／I/O 增量；新出現的程序第一次沒有基準值，不算活動。

| 檔案 | 職責 |
|---|---|
| `src/main.rs` | 進入點、視窗設定、`rust_i18n::i18n!` 巨集 |
| `src/app.rs` | 儀表板 UI 與狀態（歷史資料、篩選、語言） |
| `src/sampler.rs` | 背景取樣執行緒，避免列舉程序時卡住 UI |
| `src/system.rs` | 系統層級：`GlobalMemoryStatusEx`、`GetPerformanceInfo` |
| `src/process.rs` | 程序層級原始資料：Toolhelp 列舉、記憶體、CPU 時間、I/O、Session、Chromium `--type=` 偵測、前景程序 |
| `src/classify.rs` | 依執行檔名分組成「應用程式」、WebView2 歸屬到擁有者、分類（系統／安全／虛擬機／服務／瀏覽器／Web 技術／一般） |
| `src/activity.rs` | 跨取樣追蹤 CPU、I/O、前景，判定使用中／不活躍／閒置 |
| `src/priority.rs` | `Governor`：降低／還原記憶體優先權、自動還原、當機還原紀錄檔（M4） |
| `src/trim.rs` | 修剪長時間閒置程式的工作集，在背景執行緒進行並回報實際移出量（M5） |
| `src/audio.rs` | Core Audio：找出有作用中播放／錄音工作階段的程序（M5） |
| `src/integration.rs` | 「瀏覽器整合」開關：寫入／刪除 host 資訊清單與 HKCU 的 Native Messaging 機碼（M6） |
| `src/bridge.rs` | 瀏覽器擴充功能的 pipe 伺服器：依 client PID 判定瀏覽器、加總各設定檔的分頁數、轉送使用者觸發的丟棄請求（M6） |
| `host/src/main.rs` | `memwarden-host.exe`：在擴充功能的 stdio 與 memwarden 的 pipe 之間轉送訊框，不解析內容 |
| `host/src/frame.rs` | 訊框格式（4 位元組長度 + JSON，上限 64 KB），host 與 bridge 共用 |
| `host/src/pipe.rs` | named pipe 兩端（overlapped I/O）、只限目前使用者的 DACL |
| `host/src/browser.rs` | 從 PID 沿父程序往上找出 Chrome／Edge 瀏覽器程序 |
| `extension/manifest.json` | Chrome MV3 擴充功能；`key` 讓未封裝載入時 ID 固定為 `jmngjfeoiaihgaabjklldhfmnaejnkga` |
| `extension/tabs.js` | 可丟棄候選判定、分頁計數、丟棄（service worker 與彈出視窗共用） |
| `extension/background.js` | service worker：維持 Native Messaging 連線、回報計數、執行 memwarden 送來的丟棄請求 |
| `extension/popup.*` | 彈出視窗：連線狀態、閒置門檻、分頁清單、逐一丟棄與「丟棄閒置分頁」（需確認） |
| `extension/_locales/*/messages.json` | 擴充功能的四語系文字 |
| `extension/tests/` | node 測試（打包時排除） |
| `src/theme.rs` | 深色主題配色、依語言載入 Windows 系統字型 |
| `src/i18n.rs` | 語言列舉、偵測 Windows 顯示語言 |
| `locales/*.yml` | 介面文字（每語言一檔，缺字回退英文） |
| `scripts/capture-window.ps1` | 擷取 memwarden 視窗截圖（驗證 UI 用） |

## 5. 開發慣例

- **寫入界線**：會改變其他程序的操作只能放在 `priority.rs`（優先權）與 `trim.rs`（修剪），只能由使用者觸發，
  且不得留下無法還原的狀態。開啟程序一律經 `process::open_instance`（檢查 PID + 啟動時間）。其他模組開啟程序只能用 `PROCESS_QUERY_LIMITED_INFORMATION`。
  `Governor` 由 UI 與取樣執行緒共用（`Arc<Mutex<_>>`）；UI 的操作先收集成 `Action`，畫面繪製完再套用。
  `bridge.rs` 不直接改變任何程序，只在使用者按下按鈕時請擴充功能丟棄閒置分頁，實際丟棄由瀏覽器執行。
- **i18n**：使用者看得到的字串一律加入四個 `locales/*.yml`，用 `t!("key", name = value)`。
  鍵名用「區塊.項目」（例如 `kpi.commit.title`）。
  翻譯在編譯期內嵌，**只改 `.yml` 不會觸發重新編譯**，介面會顯示鍵名；改完先 `touch src/main.rs`
  （PowerShell：`(Get-Item src\main.rs).LastWriteTime = Get-Date`）再建置。
  瀏覽器擴充功能（M6）使用 Chrome 規定的 `extension/_locales/*/messages.json`，同樣四種語言、鍵必須一致。
- **隱私**：
  - 程序命令列只在記憶體中解析必要旗標（例如 Chromium 的 `--type=`），不得顯示、記錄或寫入檔案。
  - 截圖會含本機程序名稱，不要提交到 repo（`/screenshots/` 已被忽略）。
  - 程式不得連網。擴充功能 ↔ host ↔ memwarden 只走本機 stdio 與 named pipe
    （pipe 只允許目前使用者、拒絕遠端連線）。
  - 分頁的 URL、標題只留在擴充功能內；memwarden 只收到分頁計數。擴充功能不載入 favicon（會連網）。
  - 登錄只由 `integration.rs` 在使用者勾選「瀏覽器整合」時寫入 HKCU 的兩個 Native Messaging 機碼，取消勾選即刪除；
    其他模組與 host 都不寫登錄。
  - 本機只寫入三個檔案：UI 設定（`%APPDATA%\memwarden`，由 eframe 管理）、啟用瀏覽器整合時的
    host 資訊清單（`%LOCALAPPDATA%\memwarden\com.memwarden.host.json`），與還原紀錄檔
    （`%LOCALAPPDATA%\memwarden\lowered-priorities.txt`，只有數字，沒有降低任何程式時會刪除）。
- **平台程式碼**：Windows API 呼叫集中在 `system.rs`、`process.rs`（及 M3 新增模組）與 `host/src/`，UI 不直接呼叫 Win32。
- **字型**：不打包字型檔，執行時讀取 `C:\Windows\Fonts`。
- **自身占用**：新增功能後用發佈版量測 memwarden 的私有記憶體，目標 < 160 MB。

## 6. egui 0.37 API 注意事項

網路上多數範例是舊版 API，以下是本專案已踩過的差異：

| 舊版 | 0.37 |
|---|---|
| `fn update(&mut self, ctx, frame)` | `fn ui(&mut self, ui: &mut Ui, frame)` |
| `TopBottomPanel::top(..).show(ctx, ..)` | `Panel::top(..).show(ui, ..)` |
| `CentralPanel::default().show(ctx, ..)` | `CentralPanel::default().show(ui, ..)` |
| `ctx.style_mut(..)` | `ctx.style_mut_of(Theme::Dark, ..)` |
| `Frame::none()`、`Rounding` | `Frame::new()`、`CornerRadius`（整數） |
| `Line::new(points)` | `Line::new(name, points)` |

eframe 必須維持 `default-features = false` 並啟用 `glow`；預設的 `wgpu` 會讓自身記憶體升到約 420 MB。

## 7. 驗證方式

1. `cargo build` 和 `cargo test` 都必須沒有錯誤與警告。單元測試涵蓋不需呼叫 Win32 的純邏輯
   （`classify.rs` 分類與 WebView2 歸屬、`activity.rs` 狀態轉換）。
2. 執行 `scripts/capture-window.ps1` 擷取四種語言的截圖並目視檢查：
   ```powershell
   .\scripts\capture-window.ps1 -Exe .\target\debug\memwarden.exe -OutDir .\screenshots -Langs zh-TW,en,ja,ko
   ```
   這個腳本用 `PrintWindow` 只擷取 memwarden 視窗，不會截到其他視窗。
   閒置狀態需要 `-WaitSeconds 370`（5 分鐘以上）；修剪按鈕需要 `-WaitSeconds 970`（15 分鐘以上）。
3. 音訊偵測可對真實系統執行診斷：`cargo test audio -- --ignored --nocapture`（印出正在播放／錄音的 PID）。
4. 擴充功能：`node --test "extension/tests/*.test.mjs"`（需要 Node.js；只測不需瀏覽器的邏輯與四語系一致性）。
   彈出視窗的版面可以在暫存目錄複製一份 `extension/`，在 `popup.js` 前加一個假的 `chrome` 物件
   （`i18n`、`tabs`、`storage`、`runtime`），用本機 HTTP 伺服器開啟 `popup.html` 檢查。
5. 不經真實瀏覽器驗證 bridge 與分頁列：把 `powershell.exe` 複製成暫存目錄裡的 `chrome.exe`，
   由它啟動 `memwarden-host.exe` 並在收到 `connected:true` 時送出 `tabs` 訊息（格式見 PLAN.md 7.5）。
   memwarden 會把它當成瀏覽器，在應用程式卡片顯示分頁列。測試完記得結束這些程序。
6. 發佈版量測自身記憶體（工作管理員或 `Get-Process memwarden`）。

## 8. 已知問題與限制

- 閒置判定只能從 memwarden 啟動後開始累計。
- UWP 應用程式的前景視窗屬於 `ApplicationFrameHost.exe`，前景偵測無法對應到實際的 UWP 程序；
  它們仍可由 CPU／I/O 判定活動。
- 分類依賴 `classify.rs` 中的已知清單；不在清單內的安全軟體會被歸為一般應用程式。
- CJK 字型占自身記憶體約 30–60 MB（正黑體約 20 MB、Malgun Gothic 約 13 MB）。
- 日文模式為了基線一致，Latin 文字也用 Yu Gothic 繪製。
- 歷史圖例的排序由 egui_plot 依名稱決定，各語言順序不同。
- UI 沒有自動化測試，靠截圖目視驗證。
- 閒置門檻（30 秒平均、單核心 5%、512 KB/s、5 分鐘）是依開發機實測訂的經驗值，常數在 `activity.rs` 開頭。
- 同時開兩個 memwarden 時，只有第一個能建立 pipe，第二個不顯示分頁資訊。
- 以系統管理員身分執行 memwarden 時，一般權限的 host 能否連上尚未實測。
