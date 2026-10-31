# memwarden 計畫書

最後更新：2026-10-31

## 1. 目標

在 Windows 上找出「占著記憶體卻沒在用」的應用程式，並以**不關閉程式**的方式
讓系統優先回收它們的記憶體；同時讓使用者看清楚真正的瓶頸（實體記憶體 vs. 認可記憶體 Commit）。

## 2. 設計原則

| 原則 | 說明 |
|---|---|
| 先觀察、後動手 | 分析功能永遠唯讀。任何會改變其他程序狀態的功能（M4 以後）都必須由使用者明確觸發，且可還原 |
| 調度而非關閉 | 優先使用系統配合型機制（記憶體優先權），強制修剪工作集只作為輔助 |
| 誠實呈現 | 修剪工作集**不會**降低 Commit；介面與文件不得暗示「釋放」了 Commit |
| 隱私 | 不連網；程序命令列只在記憶體中解析、不顯示、不儲存；報表與傾印檔一律被 `.gitignore` 排除 |
| 低占用 | 記憶體工具本身要輕。目前目標：私有記憶體 < 160 MB |
| 國際化 | 所有介面文字走 `locales/*.yml`，不得在程式碼中寫死使用者可見字串 |

## 3. 里程碑與開發進度

| 階段 | 內容 | 狀態 | 完成日 |
|---|---|---|---|
| M0 | 專案建立、Git、授權（MIT OR Apache-2.0）、開源用 `.gitignore` | ✅ 完成 | 2026-10-31 |
| M1 | 唯讀分析核心：系統記憶體、Commit、各應用程式私有記憶體／工作集 | ✅ 完成 | 2026-10-31 |
| M2 | 儀表板 UI（egui）：KPI 卡片、Commit 警示、10 分鐘歷史圖、排行、可篩選表格 | ✅ 完成 | 2026-10-31 |
| M2.1 | 多語言：繁中／英／日／韓，依語言切換系統字型，記住選擇 | ✅ 完成 | 2026-10-31 |
| M2.2 | 改用 glow 渲染器，自身記憶體 420 MB → 約 90–150 MB | ✅ 完成 | 2026-10-31 |
| M3 | 閒置程序偵測與分類：類別、WebView2 歸屬、30 秒平均活動判定、篩選與閒置摘要 | ✅ 完成 | 2026-10-31 |
| M4 | 降低閒置程序記憶體優先權（`ProcessMemoryPriority`），可還原 | ✅ 完成 | 2026-10-31 |
| M5 | 音訊播放偵測；選擇性修剪長時間閒置程式的工作集（`EmptyWorkingSet`） | ✅ 完成 | 2026-10-31 |
| **M6** | **瀏覽器配套擴充功能：分頁閒置概況與分頁丟棄（Native Messaging）** | 🚧 規格完成（見第 7 節） | |
| M7 | 發佈：GitHub repo、CI（build + clippy）、Release 執行檔、應用程式圖示 | ⏳ 未開始 | |

## 4. M3 閒置程序偵測與分類（詳細規格）

### 4.1 分類

每個應用程式群組歸入一個類別：

| 類別 | 判斷方式 | 可列為閒置候選 |
|---|---|---|
| 系統 | 已知系統程序清單（`System`、`svchost.exe`、`dwm.exe`…） | 否 |
| 安全防護 | 已知清單（Defender、卡巴斯基、AdGuard…） | 否 |
| 虛擬機 | `vmmem`、`vmwp.exe`、VirtualBox、VMware… | 否（另行處理） |
| 背景服務 | 執行於 Session 0 | 否 |
| 瀏覽器 | 已知瀏覽器執行檔 | 是 |
| Web 技術應用 | 子程序帶有 Chromium `--type=` 參數（Electron／CEF），或擁有 WebView2 子程序 | 是 |
| 一般應用程式 | 其餘 | 是 |

**WebView2 歸屬**：`msedgewebview2.exe` 沿父程序往上找到第一個非 WebView2 程序，
其記憶體計入該擁有者（例如 Teams、LINE），不再單獨列為 `msedgewebview2.exe`。

### 4.2 閒置判定

memwarden 每 2 秒取樣一次，對每個應用程式群組計算：

- **CPU**：所有程序 CPU 時間增量 ÷ 經過時間（以單一核心 = 100% 表示）
- **I/O**：讀寫與其他 I/O 位元組增量
- **前景**：前景視窗所屬程序是否在群組內

任一條件成立即視為「有活動」，更新最後活動時間：
前景、**最近 30 秒平均** CPU ≥ 單核心 5%、**最近 30 秒平均** I/O ≥ 512 KB/s。

使用 30 秒平均，是因為背景程式會週期性短暫喚醒（計時器、同步）。如果只看單次 2 秒取樣，
Electron 這類大型應用幾乎永遠不會被判定為閒置（2026-10-31 實測發現後修正）。
門檻 5% 依實測訂定：未使用的背景程式 30 秒平均最高約 4%（ChatGPT 3.3%、Everything 4.0%）。
表格的 CPU 欄位則顯示最近一次取樣、占全部核心的比例，與工作管理員一致。

| 狀態 | 條件 |
|---|---|
| 使用中 | 最後活動 < 1 分鐘 |
| 不活躍 | 1–5 分鐘 |
| 閒置 | ≥ 5 分鐘（顯示已閒置多久） |

注意：閒置時間從 memwarden 開始觀察時起算，剛啟動時所有程式都還不會被判定為閒置。

### 4.3 介面

- 表格新增「類別」、「CPU」、「狀態」欄位
- 依類別篩選、「只顯示閒置」切換
- 應用程式卡片標題顯示閒置摘要：「閒置 N 個 · X GB」

### 4.4 工作項目

- [x] 規格與文件
- [x] 程序資料擴充：父程序、Session、CPU 時間、I/O、Chromium 程序類型
- [x] 分類模組 `classify.rs`（含 WebView2 歸屬）
- [x] 活動追蹤模組 `activity.rs`（CPU／I/O／前景 → 狀態）
- [x] 取樣器整合
- [x] 介面：欄位、篩選、閒置摘要
- [x] 四語系翻譯
- [x] 截圖驗證（閒置狀態需觀察 5 分鐘以上）與量測自身占用（發佈版約 102 MB）
- [x] 單元測試：分類規則、WebView2 歸屬、狀態轉換（10 項，`cargo test`）

## 5. M4 降低記憶體優先權（規格）

- **作用**：以 `SetProcessInformation(ProcessMemoryPriority)` 把閒置程式所有程序的記憶體優先權
  從 Normal (5) 降到 Low (2)。實體記憶體不足時，Windows 會先回收這些程式的分頁。程式持續執行，Commit 不變。
  優先權只影響之後被存取或回收的分頁，所以效果是逐步出現的，不是立即釋放記憶體。
- **觸發**：只由使用者觸發。可以逐列點「降低優先權」（不是使用中的候選程式），或用「降低所有閒置程式」（需要再按一次確認）。
- **對象**：只限閒置候選類別（一般應用程式、Web 技術應用、瀏覽器）。系統、安全防護、虛擬機、服務都不會出現按鈕。
- **還原**（每個程序都先記錄原始值）：
  1. 該程式恢復使用（狀態回到「使用中」，例如切到前景）時自動還原
  2. 手動「還原」／「全部還原」
  3. 關閉 memwarden 時全部還原（`on_exit`）
  4. 當機保護：每次變更後立即寫入 `%LOCALAPPDATA%\memwarden\lowered-priorities.txt`，
     內容只有「PID 啟動時間 原始值」三個數字；下次啟動時先依此還原再刪除
- **新程序**：已降低的程式之後新開的程序（例如 Chrome 新分頁）會在下一次取樣時一併降低。
- **安全**：以「PID + 啟動時間」確認是同一個程序實例，避免 PID 被重複使用時誤改到其他程序。
  受保護或權限較高的程序無法變更，介面會顯示失敗數量。
- **權限**：只對這些操作使用 `PROCESS_SET_INFORMATION`；其他讀取仍只用 `PROCESS_QUERY_LIMITED_INFORMATION`。
- **測試**：`priority.rs` 內含 4 項實際呼叫 Win32 的測試，對測試自己啟動的 `cmd.exe` 子程序操作。

## 6. M5 音訊偵測與修剪工作集（規格）

### 6.1 音訊播放偵測（先決條件）

- 每次取樣以 Core Audio（`IMMDeviceEnumerator` → `IAudioSessionManager2`）列舉**所有播放與錄音裝置**上
  狀態為 Active 的音訊工作階段，取得其 PID。
- 有作用中音訊工作階段的程式一律視為「使用中」，因此不會被列為閒置、降低優先權或修剪。
  涵蓋背景音樂、影片、通話（含收音）。暫停播放時工作階段不再是 Active，屬於正常的閒置。
- 介面狀態欄以「♪」標示。實測：Chrome 播放 YouTube 時其音訊服務程序、LINE 的 `LineCall.exe` 都被正確偵測。
- 取樣執行緒以 MTA 初始化 COM；Core Audio 不可用時只是停用偵測，不影響其他功能。

### 6.2 修剪工作集

- **作用**：對程式的每個程序呼叫 `EmptyWorkingSet`，立即把分頁移出實體記憶體：未修改的進入備用清單，
  已修改的寫入分頁檔或記憶體壓縮區。程式持續執行；之後再存取時才讀回，因此第一次使用會稍慢。
- **對象**：閒置候選類別中**閒置 ≥ 15 分鐘**（`trim::TRIM_AFTER`）的程式。音訊中的程式不會是閒置，自然排除。
- **觸發**：只由使用者觸發。逐列「修剪」，或「修剪長時間閒置」（需確認，確認文字列出程式數與占用的實體記憶體）。
- **執行**：在背景執行緒進行，期間其他修剪按鈕停用；完成後顯示「已修剪 N 個 · 移出實體記憶體 X」，
  數字是修剪前後實際量測的工作集差值，不是估計值。
- **可還原性**：修剪不留下任何持久狀態（分頁會在需要時自動讀回），所以不需要還原機制。
- **與 M4 的關係**：互相獨立，可以同時使用。M4 影響之後的回收順序，M5 是立即移出。
- **限制**：不會降低 Commit。對很快又會被使用的程式反覆修剪會增加磁碟 I/O，因此只提供手動觸發，不做自動修剪。
- **權限**：`PROCESS_SET_QUOTA`（只在 `trim.rs` 使用），同樣以「PID + 啟動時間」確認程序實例。
- **測試**：實際修剪測試自己啟動的 `cmd.exe` 子程序，驗證工作集確實下降，以及不同啟動時間時不會動作。

## 7. M6 瀏覽器配套擴充功能（規格）

### 7.1 目標與範圍

瀏覽器是最大的記憶體使用者，但 memwarden 從外部只看得到一堆 `chrome.exe` 程序，
看不出是哪些分頁、也無法安全地只釋放某些分頁。M6 加入一個配套擴充功能，提供：

1. **分頁閒置概況**：分頁總數、已丟棄數、可丟棄數（閒置超過門檻的候選分頁）。
2. **丟棄分頁**（`chrome.tabs.discard`）：瀏覽器卸載分頁內容，分頁仍留在分頁列，點一下就重新載入。
   當一個 renderer 程序的分頁全部被丟棄，該程序會結束，因此和修剪工作集不同，**丟棄分頁確實會降低 Commit**。

支援 Chrome 與 Edge（同一份擴充功能，Chromium 121 以上；需要 `Tab.lastAccessed`）。
Firefox 等非 Chromium 瀏覽器不在本階段範圍。

**做不到的事**：穩定版 Chrome 不提供「分頁 ↔ 程序 PID」與單一分頁記憶體的 API
（`chrome.processes` 只在 Dev 頻道）。因此 memwarden 不顯示每個分頁的記憶體，
只顯示整個瀏覽器在丟棄前後實際量測到的差值。

### 7.2 元件與目錄

```
extension/   Chrome MV3 擴充功能（純 JS，無建置步驟）
host/        Native Messaging host：memwarden-host.exe（workspace 成員）
src/bridge.rs  memwarden 端的 named pipe 伺服器
scripts/register-host.ps1  註冊／解除註冊 host
```

```
擴充功能 ──stdio（Native Messaging）── memwarden-host.exe ──named pipe── memwarden.exe
（service worker）                     （每個瀏覽器設定檔一個）          （bridge.rs）
```

- **為什麼需要獨立的 host**：每次 `connectNative` 時 Chrome 都會啟動一個新的 host 程序。
  host 只負責轉送訊息，不連結 egui，維持很小的占用。
- **Cargo workspace**：根目錄的 `Cargo.toml` 保留 `[package] memwarden`，另加 `[workspace] members = ["host"]`，
  `src/` 不搬動。
- **共用程式碼**：`host` 套件同時是函式庫（`memwarden_host`）：訊框讀寫（`frame`）與 pipe 名稱（`pipe_name`），
  `bridge.rs` 直接引用，避免兩端格式不一致。
- **pipe 使用 overlapped I/O**：同步 handle 上 Windows 會把讀寫排隊，等待 memwarden 的讀取會卡住擴充功能送來的寫入。
- **host 不解析 JSON**：host 只轉送長度前綴的訊框，唯一自己產生的訊息是連線時的 `hello`
  （以 `format!` 組出，只含數字），因此 host 不依賴 serde。memwarden 端用 `serde_json` 解析。

### 7.3 擴充功能

- **權限**：只要 `tabs`、`nativeMessaging`、`storage`。沒有 `host_permissions`、沒有 content script、
  不載入遠端程式碼、不連網。`tabs` 用來在彈出視窗顯示分頁標題，會讓 Chrome 顯示「讀取瀏覽記錄」的權限警告，
  README 需說明原因。
- **背景（service worker）**：啟動時 `connectNative`。連線中的 native port 會讓 service worker 保持存活。
  監聽 `tabs.onCreated / onRemoved / onUpdated / onActivated`，變動時（最多每 5 秒一次）把概況送給 host。
- **彈出視窗**：
  - 狀態列：「已連線 memwarden」／「memwarden 未執行」（擴充功能本身在沒有 memwarden 時也能使用）。
  - 依最後存取時間排序的分頁清單（標題、閒置多久、是否已丟棄），每列有「丟棄」按鈕。
  - 「丟棄閒置分頁」：需再按一次確認，確認文字列出分頁數與「未儲存的表單內容可能遺失」警告。
  - 閒置門檻設定（預設 15 分鐘，可選 5／15／30／60／120 分鐘），存在 `chrome.storage.local`。
- **可丟棄候選**：符合**全部**條件：
  - 不是任一視窗的作用中分頁
  - 沒有在播放聲音（`audible`）
  - 沒有釘選
  - 尚未丟棄
  - `autoDiscardable` 不是 `false`
  - 已載入完成
  - `lastAccessed` 距今 ≥ 門檻
  
  `tabs.discard` 失敗（瀏覽器拒絕）時計為「略過」。
- **i18n**：Chrome 擴充功能只能用自己的 `_locales/{zh_TW,en,ja,ko}/messages.json`（`default_locale: en`），
  語言跟隨瀏覽器介面語言。四個檔案的鍵必須一致。
- **擴充功能 ID 固定**：`manifest.json` 放公開金鑰（`key`），讓未封裝載入時 ID 固定，host 資訊清單的
  `allowed_origins` 才能寫死。私鑰 `.pem` 不進 repo（已被 `.gitignore` 排除）。上架商店屬於 M7。

### 7.4 隱私界線

- 分頁的 URL、標題、favicon **只留在擴充功能內**（彈出視窗顯示用），不送給 host 或 memwarden，也不寫入儲存空間。
- 送給 memwarden 的只有數字與布林值：分頁 ID 與視窗 ID（僅用於回報結果）、各狀態的計數、閒置秒數。
- host 與 bridge 不寫檔案、不寫記錄、不連網。

### 7.5 通訊協定（版本 1）

訊框：Native Messaging 規定的 4 位元組長度（原生位元組順序）＋ UTF-8 JSON。named pipe 上使用相同格式。
單一訊息上限 64 KB（Chrome 的 host → 瀏覽器上限為 1 MB），超過即斷線。所有訊息帶 `"v": 1`。

| 方向 | 訊息 | 內容 |
|---|---|---|
| host → memwarden | `hello` | host 的父瀏覽器程序 PID 與啟動時間（host 沿父程序往上找到第一個已知瀏覽器；Chrome 在 Windows 可能經由 `cmd.exe` 啟動 host） |
| 擴充功能 → memwarden | `tabs` | `total`、`discarded`、`candidates`、`audible`、門檻分鐘數、最舊候選的閒置秒數 |
| memwarden → 擴充功能 | `discard_idle` | 請求丟棄目前所有候選分頁（只由 memwarden 的使用者按鈕觸發） |
| 擴充功能 → memwarden | `discard_result` | `requested`、`discarded`、`skipped` |

不認得的訊息類型一律忽略；版本不符則斷線，並在 memwarden 介面顯示「擴充功能版本不相容」。

### 7.6 host（`memwarden-host.exe`）

- stdin／stdout 使用二進位模式；stdin 關閉（瀏覽器結束或擴充功能斷線）時結束。
- 以 client 身分連線 `\\.\pipe\memwarden-<使用者 SID>`；memwarden 未執行時每 5 秒重試，
  並送 `{"type":"status","connected":false}` 給擴充功能，讓彈出視窗顯示狀態。
- 多個瀏覽器設定檔會各自啟動 host，memwarden 端同時接受多條連線。

### 7.7 memwarden 端（`bridge.rs`）

- 背景執行緒建立 named pipe 伺服器：DACL 只允許目前使用者、`PIPE_REJECT_REMOTE_CLIENTS`。
- 依 `hello` 的 PID + 啟動時間把連線對應到應用程式群組（例如 `chrome.exe`）；同一瀏覽器的多條連線加總顯示。
- **介面**：已連線的瀏覽器在應用程式表格中顯示「分頁 N · 已丟棄 D · 可丟棄 C」，並有「丟棄閒置分頁」按鈕
  （需確認）。完成後顯示「已丟棄 N 個分頁 · 瀏覽器私有記憶體 −X」，X 是送出前與收到結果 5 秒後
  實際量測的群組私有記憶體差值，可能包含其他變動，介面標示為「前後差值」。
  沒有安裝擴充功能時不顯示任何提示。
- **寫入界線**：`bridge.rs` 不直接改變任何程序，只轉送使用者觸發的丟棄請求，實際丟棄由瀏覽器執行。
  HANDOFF 的寫入界線規則需補上這一條。

### 7.8 註冊

`scripts/register-host.ps1` 由使用者手動執行：

- 在 `memwarden-host.exe` 旁產生 host 資訊清單（`com.memwarden.host.json`，`allowed_origins` 只有本擴充功能 ID）。
- 寫入 `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.memwarden.host` 與
  `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.memwarden.host`（預設值為資訊清單路徑）。
- `-Unregister` 刪除上述機碼與資訊清單。memwarden 本身不寫登錄。

### 7.9 可還原性

被丟棄的分頁點一下就會重新載入，分頁列、分頁歷史和捲動位置由瀏覽器保留。
**未儲存的表單內容可能遺失**，而擴充功能為了隱私不注入 content script，無法偵測表單內容。
緩解方式：只丟棄閒置超過門檻的分頁、一律由使用者觸發、確認文字明確警告。不提供自動丟棄，
也不提供「全部重新載入」（會一次讀回大量記憶體）。

### 7.10 工作項目

- [x] 規格
- [x] Cargo workspace 與 `host/` 骨架；訊框讀寫與父瀏覽器查找（10 項單元測試；
      另以暫時的 PowerShell pipe 伺服器手動驗證雙向轉送、memwarden 未執行／中途關閉時的狀態通知、stdin 關閉時結束）
- [ ] `bridge.rs`：named pipe 伺服器、DACL、訊息解析（含單元測試）
- [ ] 擴充功能：manifest、service worker、候選判定、彈出視窗、四語系 `_locales`
- [ ] memwarden 介面：瀏覽器列的分頁概況、丟棄按鈕與結果、四語系 `locales/*.yml`
- [ ] `register-host.ps1`（含 `-Unregister`）
- [ ] 實機驗證：Chrome 與 Edge、多設定檔、memwarden 未執行／中途結束、Commit 前後量測
- [ ] 量測 host 與 memwarden 自身占用
- [ ] 更新 HANDOFF：架構、檔案表、寫入界線、隱私（登錄機碼、分頁資料）、i18n 規則、驗證方式

## 8. 決策紀錄

| 日期 | 決策 | 理由 |
|---|---|---|
| 2026-10-31 | 只支援 Windows | macOS 不提供修剪他程序記憶體的公開 API，分頁由系統全權管理；管理功能無法移植，分析功能「活動監視器」已足夠 |
| 2026-10-31 | UI 用 egui／eframe | 純 Rust、原生、輕量。Tauri 依賴 WebView2 太吃記憶體；Slint 授權（GPL）與 MIT 不相容 |
| 2026-10-31 | 渲染器用 glow（OpenGL） | 預設 wgpu 使自身記憶體達 420 MB |
| 2026-10-31 | i18n 用 rust-i18n，每語言一檔 | 翻譯編譯期內嵌；新增語言只需加檔案 |
| 2026-10-31 | 程序命令列不顯示、不儲存 | 命令列可能含 token、路徑等敏感資訊；只解析 `--type=` 等必要旗標 |
| 2026-10-31 | 記憶體優先權降到 Low (2) 而非 Very Low (1) | 保守起見；閒置程式恢復使用時會自動還原，先觀察實際效果再決定是否調整 |
| 2026-10-31 | 修剪只提供手動觸發、門檻 15 分鐘 | 修剪會讓程式下次使用時變慢，自動修剪可能造成反覆讀回；15 分鐘比閒置門檻（5 分鐘）更保守 |
| 2026-10-31 | 音訊中的程式視為使用中 | 背景播放時 CPU 很低，若無此規則會被誤判為閒置，修剪後播放斷續 |
| 2026-10-31 | 閒置判定：30 秒平均、單核心 5% | 實測未使用的 Electron 背景程式會週期性短暫喚醒，30 秒平均最高約 4%；單次取樣 1% 門檻會讓大型應用永遠無法被判定為閒置 |
| 2026-10-31 | M6 目錄：`extension/`、`host/` 另開，Rust 程式不搬動 | 根目錄 package 可兼 workspace root；不影響既有建置指令、文件路徑與 git 歷史 |
| 2026-10-31 | Native Messaging host 為獨立的小執行檔，經 named pipe 連到 memwarden | Chrome 每次連線都會啟動新的 host 程序；直接用 memwarden.exe 會多開 GUI 實例 |
| 2026-10-31 | 分頁 URL／標題不離開擴充功能 | 與命令列同等敏感；memwarden 只需要計數就能呈現概況與觸發丟棄 |
| 2026-10-31 | 丟棄分頁只由使用者觸發，預設門檻 15 分鐘 | 未儲存的表單內容可能遺失，且無法在不注入 content script 的情況下偵測 |
| 2026-10-31 | 不做單一分頁記憶體 | 穩定版 Chrome 沒有分頁 ↔ PID 的 API；改以瀏覽器整體的前後實測差值呈現 |

## 9. 非目標

- 結束或強制關閉程序
- 「一鍵清理記憶體」這類全面修剪（會造成卡頓且無法降低 Commit）
- 讀取其他程序的記憶體內容

## 10. 待後續階段處理的風險

- ~~M4／M5 須排除正在播放音訊的程式~~：已於 M5 以 Core Audio 偵測解決（見 6.1）。
- 合併多個程序的音訊工作階段（`GetProcessId` 失敗者）目前會被略過；若發現漏判的播放程式，需改以工作階段識別字串比對。
- M6：Chrome／Edge 在 Windows 啟動 host 的程序鏈（是否經由 `cmd.exe`）需實機確認，父瀏覽器查找要能容忍中間層。
- M6：丟棄分頁可能讓未儲存的表單內容遺失（見 7.9），實機驗證時確認 Chrome／Edge 目前的表單還原行為，必要時調整確認文字。
- M6：memwarden 新增 `serde_json` 相依，需量測對執行檔大小與自身記憶體的影響。