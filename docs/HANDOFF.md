# memwarden 接手文件

最後更新：2026-10-31 · 版本 0.1.0

開發進度與下一步請見 [PLAN.md](PLAN.md)。

## 1. 專案概要

Windows 記憶體儀表板（Rust + egui）。目前為**唯讀**：只讀取記憶體資訊，不修改、暫停或結束任何程序。
長期目標是在不關閉程式的前提下，讓系統優先回收閒置程式的記憶體。

## 2. 環境

| 項目 | 版本 / 說明 |
|---|---|
| 作業系統 | Windows 10 以上（開發機：Windows 11 Pro） |
| Rust | 1.99（edition 2024） |
| 主要套件 | `eframe`/`egui` 0.37、`egui_plot` 0.38、`egui_extras` 0.37、`windows` 0.62、`rust-i18n` 4 |
| Git 作者 | `見未亞由 <yuuchilyann@hotmail.com>`（已由專案擁有者同意公開） |

## 3. 建置與執行

```powershell
cargo build                     # 偵錯版，會顯示主控台
cargo build --release           # 發佈版，無主控台視窗（LTO，約 4–7 分鐘）
.\target\release\memwarden.exe
$env:MEMWARDEN_LANG = "ja"; cargo run   # 強制指定語言（zh-TW / en / ja / ko）
```

以系統管理員身分執行可讀取全部程序；否則受保護程序（例如防毒軟體）會被計入「無法讀取」。

## 4. 架構

```
sampler 執行緒（每 2 秒）                       UI 執行緒（eframe）
  system::query()                ─┐              App::ui()
  Collector::list()  → procs      │                drain_samples() → history
  classify::group(procs) → apps   ├ Snapshot ─mpsc─▶ header / kpi_row / history_card
  Tracker::update(procs, apps,    │                top_card / apps_card（篩選、類別、狀態）
                  foreground)    ─┘
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
| `src/theme.rs` | 深色主題配色、依語言載入 Windows 系統字型 |
| `src/i18n.rs` | 語言列舉、偵測 Windows 顯示語言 |
| `locales/*.yml` | 介面文字（每語言一檔，缺字回退英文） |
| `scripts/capture-window.ps1` | 擷取 memwarden 視窗截圖（驗證 UI 用） |

## 5. 開發慣例

- **唯讀界線**：M4 之前，開啟程序控制代碼只能用 `PROCESS_QUERY_LIMITED_INFORMATION`。
- **i18n**：使用者看得到的字串一律加入四個 `locales/*.yml`，用 `t!("key", name = value)`。
  鍵名用「區塊.項目」（例如 `kpi.commit.title`）。
- **隱私**：
  - 程序命令列只在記憶體中解析必要旗標（例如 Chromium 的 `--type=`），不得顯示、記錄或寫入檔案。
  - 截圖會含本機程序名稱，不要提交到 repo（`/screenshots/` 已被忽略）。
  - 程式不得連網。
- **平台程式碼**：Windows API 呼叫集中在 `system.rs`、`process.rs`（及 M3 新增模組），UI 不直接呼叫 Win32。
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
3. 發佈版量測自身記憶體（工作管理員或 `Get-Process memwarden`）。

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
