# memwarden 接手文件

最後更新：2026-11-01 · 版本 0.1.0

開發進度與下一步請見 [PLAN.md](PLAN.md)。

**目前狀態（2026-11-01）**：M0–M5 完成；M6（瀏覽器擴充功能）、M7（關閉建議、分頁檔資訊）與
M8（記憶體成長追蹤）程式完成，部分實機驗證待做（PLAN.md 7.10、8.9、9.8）。
M8 第一次實機觀察（14 小時，夜間為主）已完成，依結果加入「仍在上升」條件（PLAN.md 9.3、9.8）。
**目前進行 M9 發佈**（PLAN.md 第 10 節）：CI、圖示、打包、README 已完成，
repo 已公開於 https://github.com/yuuchilyann/memwarden ，CI 通過。

**下一步**（依序）：

1. memwarden 自身占用（介紹網頁前要處理，目標 160 MB，目前穩定在約 182 MB）：白天的持續增加已查明是外部
   UI Automation 讀取（第 8 節）。螢幕鎖定時一次性約 +30 MB 疑為驅動程式造成，延後到發佈前量測時再檢查（PLAN.md 第 13 節）。
   M8 已完成夜間與白天兩次觀察，門檻數值不需調整。
2. 第一次 Release（做法見 3.1）：推送 `v0.1.0` 標籤，檢查草稿後由擁有者發佈。
3. 其餘實機驗證：M6 Edge／多設定檔／memwarden 中途關閉（PLAN.md 7.10），
   M7 有視窗程式的關閉結果、記事本未存檔、以系統管理員執行的程式（PLAN.md 8.9）。

## 1. 專案概要

Windows 記憶體儀表板（Rust + egui）。定位是「看懂記憶體被誰吃掉、哪些做法真的有用」，不是一鍵清理：
分清楚實體記憶體與認可記憶體（Commit），找出閒置的應用程式，並只在使用者觸發時：

- 降低記憶體優先權（M4）、修剪工作集（M5）：都可還原，但不會降低 Commit；
- 透過瀏覽器擴充功能丟棄閒置分頁（M6）：會降低 Commit；
- 關閉閒置很久的大型程式（M7）：等同按視窗的 X，逐一確認，這是唯一無法還原的操作；
- 持續成長的程式（M8）：提示重新開啟約可降低多少，沿用 M7 的關閉流程，不自動重新開啟。

不會暫停或強制結束任何程序。分頁檔設定只讀取，調整交給 Windows 的「效能選項」。

## 2. 環境

| 項目 | 版本 / 說明 |
|---|---|
| 作業系統 | Windows 10 以上（開發機：Windows 11 Pro） |
| Rust | 1.99（edition 2024） |
| 主要套件 | `eframe`/`egui` 0.37、`egui_plot` 0.38、`egui_extras` 0.37、`windows` 0.62、`rust-i18n` 4、`serde_json` 1 |
| Git 作者 | `見未亞由 <yuuchilyann@hotmail.com>`（已由專案擁有者同意公開） |
| GitHub | https://github.com/yuuchilyann/memwarden（公開，`master` 分支；`gh` 已登入 `yuuchilyann`） |
| 授權 | `MIT OR Apache-2.0`（使用者二選一）。兩者都不授予商標權，名稱與圖示需另行保護 |

**Git 注意**：`E:\memwarden\.git` 的擁有者是 Administrators，一般使用者執行 git 會出現 `dubious ownership`。
不要改全域設定，用 `git -c safe.directory=E:/memwarden ...`；`gh` 會在內部呼叫 git，改用環境變數：

```bash
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=E:/memwarden
```

## 3. 建置與執行

Cargo workspace：根目錄是 memwarden，`host/` 是 Native Messaging host（`memwarden-host.exe`）。
`default-members` 包含兩者，所以 `cargo build`／`cargo test` 會一起建置與測試。

```powershell
cargo build                     # 偵錯版，會顯示主控台
cargo build --release           # 發佈版，無主控台視窗（LTO，約 4–7 分鐘）
.\target\release\memwarden.exe
.\scripts\package.ps1           # 建置發佈版並打包 dist\memwarden-<版本>-windows-x64.zip（含 .sha256）
$env:MEMWARDEN_LANG = "ja"; cargo run   # 強制指定語言（zh-TW / en / ja / ko）
```

以系統管理員身分執行可讀取全部程序；否則受保護程序（例如防毒軟體）會被計入「無法讀取」。

### 3.1 發佈流程

1. 把 `Cargo.toml` 與 `extension/manifest.json` 的版本改成新版本並提交（`package.ps1` 只檢查 `Cargo.toml`）。
2. 本機先跑 `.\scripts\package.ps1` 確認 zip 內容。
3. `git tag v<版本>`、`git push origin v<版本>`：`release.yml` 建置、打包並建立**草稿** Release。
4. 由擁有者在 GitHub 檢查草稿（檔案、說明）後按發佈。發佈是對外動作，不要自行發佈。

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
                  foreground)     │              update_close()：每次新取樣後
  growth::Tracker::update(apps)   │
  pagefile::query()              ─┘                close::blocker / Closing::poll（在 UI 執行緒）
```

```
瀏覽器擴充功能 ──stdio── memwarden-host.exe ──named pipe── bridge 執行緒（每條連線一個）
                         （每個瀏覽器設定檔一個）              Bridge::browsers() → apps_card 的分頁列
```

`Collector` 會快取每個程序實例 `(pid, 啟動時間)` 的 Chromium 旗標，命令列只讀一次。
`Tracker` 以程序實例計算 CPU／I/O 增量；新出現的程序第一次沒有基準值，不算活動。
`growth::Tracker` 記錄每個程式每分鐘的私有記憶體最低值（保留 6 小時），只在記憶體中，不寫檔。

| 檔案 | 職責 |
|---|---|
| `src/main.rs` | 進入點、視窗設定、`rust_i18n::i18n!` 巨集 |
| `src/app.rs` | 儀表板 UI 與狀態（歷史資料、篩選、語言、各種操作的確認與結果） |
| `src/sampler.rs` | 背景取樣執行緒，避免列舉程序時卡住 UI |
| `src/system.rs` | 系統層級：`GlobalMemoryStatusEx`、`GetPerformanceInfo` |
| `src/process.rs` | 程序層級原始資料：Toolhelp 列舉、記憶體、CPU 時間、I/O、Session、Chromium `--type=` 偵測、前景程序 |
| `src/classify.rs` | 依執行檔名分組成「應用程式」、WebView2 歸屬到擁有者、分類（系統／安全／虛擬機／服務／瀏覽器／Web 技術／一般） |
| `src/activity.rs` | 跨取樣追蹤 CPU、I/O、前景，判定使用中／不活躍／閒置 |
| `src/priority.rs` | `Governor`：降低／還原記憶體優先權、自動還原、當機還原紀錄檔（M4） |
| `src/trim.rs` | 修剪長時間閒置程式的工作集，在背景執行緒進行並回報實際移出量（M5） |
| `src/close.rs` | 關閉建議：判定建議關閉的程式、對其最上層視窗送 `WM_CLOSE`、追蹤結果（M7） |
| `src/growth.rs` | 記憶體成長追蹤：每分鐘私有記憶體底線、重新啟動判定、持續成長判定（M8，唯讀） |
| `src/history.rs` | 歷史圖資料：最近 10 分鐘每次取樣、6 小時內每分鐘的最高值；圖表範圍（10 分／1 小時／6 小時） |
| `src/pagefile.rs` | 分頁檔設定與大小（唯讀）、系統管理時的最大值、開啟 Windows「效能選項」（M7） |
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
| `extension/icons/` | 擴充功能圖示（由 `scripts/make-icon.py` 產生） |
| `extension/popup.*` | 彈出視窗：連線狀態、閒置門檻、分頁清單、逐一丟棄與「丟棄閒置分頁」（需確認） |
| `extension/_locales/*/messages.json` | 擴充功能的四語系文字 |
| `extension/tests/` | node 測試（打包時排除） |
| `src/theme.rs` | 深色主題配色、依語言載入 Windows 系統字型 |
| `src/i18n.rs` | 語言列舉、偵測 Windows 顯示語言 |
| `locales/*.yml` | 介面文字（每語言一檔，缺字回退英文） |
| `scripts/capture-window.ps1` | 擷取 memwarden 視窗截圖（驗證 UI 用） |
| `scripts/package.ps1` | 建置發佈版並打包 zip（CI 的 release workflow 也用它）；標籤版本須等於 `Cargo.toml` |
| `scripts/make-icon.py` | 繪製圖示（Python + Pillow），只在修改圖示時執行，產物已提交 |
| `scripts/growth-logger.ps1` | M8 觀察用：每 15 秒讀 `Win32_Process`（不讀命令列），每分鐘把各程式私有記憶體最低值寫入 `reports\growth-log.csv` |
| `scripts/replay-growth.py` | 以 `growth.rs` 的規則重播上面的記錄，比較多組門檻（只用 Python 標準函式庫；常數與排除清單需和 `growth.rs`、`classify.rs` 同步） |
| `assets/` | `memwarden.ico`（執行檔）、`icon-256.png`（視窗） |
| `build.rs` | 把圖示與版本資訊嵌入 memwarden.exe（`embed-resource`） |
| `.github/workflows/` | `ci.yml`：build、clippy（`-D warnings`）、test、擴充功能測試；`release.yml`：推送 `v*` 標籤時建立草稿 Release |

## 5. 開發慣例

- **寫入界線**：會改變其他程序的操作只能放在 `priority.rs`（優先權）與 `trim.rs`（修剪），只能由使用者觸發，
  且不得留下無法還原的狀態。開啟程序一律經 `process::open_instance`（檢查 PID + 啟動時間）。其他模組開啟程序只能用 `PROCESS_QUERY_LIMITED_INFORMATION`。
  `Governor` 由 UI 與取樣執行緒共用（`Arc<Mutex<_>>`）；UI 的操作先收集成 `Action`，畫面繪製完再套用。
  `bridge.rs` 不直接改變任何程序，只在使用者按下按鈕時請擴充功能丟棄閒置分頁，實際丟棄由瀏覽器執行。
  `close.rs` 只在使用者確認關閉某個程式後，對它可見的最上層視窗送 `WM_CLOSE`（等同按 X），
  不結束程序、不開啟具寫入權限的程序控制代碼。這是唯一無法還原的操作，所以只能逐一確認，不做批次或自動關閉。
  `pagefile.rs` 只讀分頁檔設定，不修改；「調整分頁檔…」只開啟 Windows 的「效能選項」，由使用者自己在那裡設定。
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
| `ui.id()` | `ui.scope_id()`（`id()` 已棄用）。**並排的子 Ui（例如 `ui.columns` 的各欄）共用同一個 scope**，
  自訂互動 ID 要再加上可區分的鹽值（例如卡片標題），否則 ID 重複、只有最後一個有效 |

eframe 必須維持 `default-features = false` 並啟用 `glow`；預設的 `wgpu` 會讓自身記憶體升到約 420 MB。

## 7. 驗證方式

1. `cargo build` 和 `cargo test` 都必須沒有錯誤與警告（目前 memwarden 51 項 + host 10 項），
   `cargo clippy --all-targets -- -D warnings` 也要通過（CI 會檢查）。
   CI 上略過需要互動式桌面的 `closes_a_real_window`，所以 `close.rs` 的視窗測試只在本機執行。
   純邏輯：`classify.rs` 分類與 WebView2 歸屬、`activity.rs` 狀態轉換、`bridge.rs` 訊息處理、
   `close.rs` 建議條件與結果判定、`pagefile.rs` 設定解析與最大值、`growth.rs` 底線彙總與成長判定。
   會呼叫 Win32 的測試只操作測試自己啟動的子程序：`priority.rs`、`trim.rs`（`cmd.exe`），
   `close.rs`（以 PowerShell 開一個縮小的 WinForms 視窗再送 `WM_CLOSE`，測試期間工作列會短暫出現它），
   `integration.rs`（暫時的 `HKCU\Software\memwarden-test-*` 機碼）。
2. 執行 `scripts/capture-window.ps1` 擷取四種語言的截圖並目視檢查：
   ```powershell
   .\scripts\capture-window.ps1 -Exe .\target\debug\memwarden.exe -OutDir .\screenshots -Langs zh-TW,en,ja,ko
   ```
   這個腳本用 `PrintWindow` 只擷取 memwarden 視窗，不會截到其他視窗。
   閒置狀態需要 `-WaitSeconds 370`（5 分鐘以上）；修剪按鈕需要 `-WaitSeconds 970`（15 分鐘以上）；
   關閉建議需要 30 分鐘以上；持續成長需要 120 分鐘以上，且要有程式的底線成長 500 MB 以上、最近兩小時仍在上升。
   已經有另一個 memwarden 在執行時，新開的視窗不會顯示（截圖只有 16×16），先關掉原本的再截圖。
   懸停說明（tooltip）無法用這個腳本驗證，需要在實際視窗上用滑鼠確認。
3. 音訊偵測可對真實系統執行診斷：`cargo test audio -- --ignored --nocapture`（印出正在播放／錄音的 PID）。
4. 擴充功能：`node --test "extension/tests/*.test.mjs"`（需要 Node.js；只測不需瀏覽器的邏輯與四語系一致性）。
   彈出視窗的版面可以在暫存目錄複製一份 `extension/`，在 `popup.js` 前加一個假的 `chrome` 物件
   （`i18n`、`tabs`、`storage`、`runtime`），用本機 HTTP 伺服器開啟 `popup.html` 檢查。
5. 不經真實瀏覽器驗證 bridge 與分頁列：把 `powershell.exe` 複製成暫存目錄裡的 `chrome.exe`，
   由它啟動 `memwarden-host.exe` 並在收到 `connected:true` 時送出 `tabs` 訊息（格式見 PLAN.md 7.5）。
   memwarden 會把它當成瀏覽器，在應用程式卡片顯示分頁列。測試完記得結束這些程序。
6. 發佈版量測自身記憶體（工作管理員或 `Get-Process memwarden`）。
7. M8 實機觀察：把 `dist\memwarden-<版本>-windows-x64.zip` 解壓縮到 repo 以外的資料夾，從檔案總管或一般終端機
   啟動（不要從 Claude desktop app 啟動，視窗不會顯示；也不要直接執行 `target\release` 的檔案，開著時會鎖住、
   之後無法重新建置）。開著數小時，記錄被標示「持續成長」的程式、懸停說明的數字與折線，以及 memwarden 自身占用。
   介面只顯示已達門檻的程式，所以同時在背景執行外部記錄器，結束後重播：
   ```powershell
   Start-Process pwsh -ArgumentList '-NoProfile', '-File', 'scripts\growth-logger.ps1' -WindowStyle Hidden -PassThru
   # 結束時 Stop-Process -Id <上面的 PID>，再：
   python -I scripts\replay-growth.py reports\growth-log.csv
   ```
   記錄器本身約 50 MB、每次取樣約 0.5 秒的 WMI 工作；在 memwarden 中顯示為 `pwsh.exe`。
   結束前可用 `PrintWindow` 擷取正在執行的 memwarden 視窗（`capture-window.ps1` 的作法，對既有 PID 的 `MainWindowHandle`），
   截圖放 `screenshots\`。

## 8. 已知問題與限制

- 閒置判定與記憶體成長都只能從 memwarden 啟動後開始累計；成長的門檻（`growth.rs` 開頭）是初值，待實測調整。
- UWP 應用程式的前景視窗屬於 `ApplicationFrameHost.exe`，前景偵測無法對應到實際的 UWP 程序；
  它們仍可由 CPU／I/O 判定活動。
- 分類依賴 `classify.rs` 中的已知清單；不在清單內的安全軟體會被歸為一般應用程式。
- 有程式透過 UI Automation 讀取 memwarden 的介面時（eframe 的 `accesskit`；例如輔助工具、會讀取其他視窗的 AI
  或翻譯工具），memwarden 的私有記憶體會隨讀取次數增加（實測每 2 秒讀一次約 +3 MB／分），對方結束或釋放後歸還。
  量測自身占用時要確認沒有這類程式（`Get-Process -Id <pid> -Module` 出現 `uiautomationcore.dll` 表示曾被讀取）。
- 螢幕鎖定或顯示變化時，自身占用可能一次增加約 30 MB 且不歸還（疑為 OpenGL 驅動程式），延後檢查（PLAN.md 第 13 節）。
- CJK 字型占自身記憶體約 30–60 MB（正黑體約 20 MB、Malgun Gothic 約 13 MB）。
- 日文模式為了基線一致，Latin 文字也用 Yu Gothic 繪製。
- 歷史圖例的排序由 egui_plot 依名稱決定，各語言順序不同。
- UI 沒有自動化測試，靠截圖目視驗證。
- 閒置門檻（30 秒平均、單核心 5%、512 KB/s、5 分鐘）是依開發機實測訂的經驗值，常數在 `activity.rs` 開頭。
- 同時開兩個 memwarden 時，只有第一個能建立 pipe，第二個不顯示分頁資訊。
- 以系統管理員身分執行 memwarden 時，一般權限的 host 能否連上尚未實測。
- 關閉建議：只剩系統匣圖示的程式（LINE 等）無法由 memwarden 關閉，只能提示；檔案同步程式
  （`close::KEEP_RUNNING`）不列入建議。按 X 只是縮到系統匣的程式，關閉後會顯示「仍在背景執行」。
- 執行檔沒有程式碼簽章，第一次執行會出現 SmartScreen 警告（README 有說明）。
- CI 的 Windows runner 會讓子程序繼承 Low 記憶體優先權；測試不能假設新程序是 Normal（見 `priority.rs` 測試）。
- 分頁檔由系統管理時的最大值依 Microsoft 文件計算（RAM × 3 或 4 GB 取大、不超過磁碟 1/8、受剩餘空間限制），
  只在開發機（自動管理、單一 C:）實測過。
