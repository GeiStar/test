# Windows Monitor Manager Auto - 使用說明書

> **語言**: Rust  
> **適用平台**: Windows 10/11

## 🌍 語言切換

- [English](README.md)
- [简体中文](README_zh-CN.md)
- [繁體中文](README_zh-TW.md) (目前)

* * *

## 📋 目錄

1.  [專案簡介](#1-%E5%B0%88%E6%A1%88%E7%B0%A1%E4%BB%8B)
2.  [命令列參數說明](#2-%E5%91%BD%E4%BB%A4%E5%88%97%E5%8F%83%E6%95%B8%E8%AA%AA%E6%98%8E)
3.  [預設指令行為](#3-%E9%A0%90%E8%A8%AD%E6%8C%87%E4%BB%A4%E8%A1%8C%E7%82%BA)
4.  [執行模式](#4-%E5%9F%B7%E8%A1%8C%E6%A8%A1%E5%BC%8F)
5.  [典型使用場景](#5-%E5%85%B8%E5%9E%8B%E4%BD%BF%E7%94%A8%E5%A0%B4%E6%99%AF)
6.  [快速鍵操作](#6-%E5%BF%AB%E9%80%9F%E9%8D%B5%E6%93%8D%E4%BD%9C)
7.  [系統匣選單說明](#7-%E7%B3%BB%E7%B5%B1%E5%8C%A3%E9%81%B8%E5%96%AE%E8%AA%AA%E6%98%8E)
8.  [API 介面說明](#8-api-%E4%BB%8B%E9%9D%A2%E8%AA%AA%E6%98%8E)
9.  [服務安裝指南](#9-%E6%9C%8D%E5%8B%99%E5%AE%89%E8%A3%9D%E6%8C%87%E5%8D%97)
10. [日誌與除錯](#10-%E6%97%A5%E8%AA%8C%E8%88%87%E9%99%A4%E9%8C%AF)

* * *

## 1\. 專案簡介

### 1.1 誕生背景

在學校等公共教學場景中，學生經常透過 `Win+P` 胡亂調整顯示設定（Display Topology），導致：

- 畫面只在教室電視顯示，教師電腦黑屏無法操作
- 登入畫面因顯示模式錯誤而無法看到輸入框
- 影響正常教學工作的開展

**Windows Monitor Manager** 應運而生——用於**強制鎖定顯示模式**（通常是同步顯示模式 Clone），確保即使被惡意或誤操作修改，系統也能**自我修復**，保障多螢幕顯示始終可用。

### 1.2 核心能力

| 能力 | 說明 |
| --- | --- |
| 🔄 **自動修復** | 即時監控顯示模式，被竄改後自動回彈到目標模式 |
| 🔒 **強制鎖定** | 服務模式在後台兜底，系統匣模式在前台便捷控制 |
| ⌨️ **全域快速鍵** | `Alt+Shift+W/E` 等快速鍵一鍵切換，高效無憂 |
| 👤 **使用者感知** | 支援 Session 0 隔離，使用者桌面內不干擾正常操作 |
| 🖥️ **多場景適配** | 從個人家用到學校機房，四種模式全覆蓋 |

* * *

## 2\. 命令列參數說明

### 基本語法

```powershell
monitor-service.exe [OPTIONS] [COMMAND]
```

### 可用參數列表

| 參數 | 簡寫 | 預設值 | 說明 |
| --- | --- | --- | --- |
| `--service` | \- | 無 | 以 Windows 服務模式執行 |
| `--addr 0.0.0.0:3000` | \- | 無 | 監聽位址和連接埠（不填不啟動 HTTP） |
| `--mode <MODE>` | `-m` | 系統當前模式 | 初始顯示模式 |
| `--interval <SECONDS>` | `-i` | `2` | 監控間隔（秒） |
| `--log-path <PATH>` | `-l` | 無 | 日誌檔案路徑 |
| `--no-enforce` | \- | 否 | 停用執行時監控（預設啟用強制執行） |
| `--enforce-session0-only` | \- | 否 | 僅在「無使用者」或「使用者鎖屏」時強制執行；使用者已登入且未鎖屏時跳過（服務模式專用） |
| `--show-console` | \- | 否 | 顯示主控台視窗 |
| `--hide-exit` | \- | 否 | 隱藏系統匣選單中的退出按鈕 |
| `--relaunched` | \- | \- | 內部參數（使用者無需使用） |
| `--help` | `-h` | \- | 顯示說明資訊 |

### 子命令

| 子命令 | 說明 |
| --- | --- |
| `agent` | 作為代理執行（內部使用，用於工作階段注入） |
| `agent list` | 列出所有顯示器 |
| `agent set <mode>` | 設定顯示設定模式 |

### 模式參數 `<MODE>` 可選值

| 模式值 | 說明 |
| --- | --- |
| `clone` | 同步顯示模式（畫面同步顯示於所有螢幕） |
| `extend` | 延伸顯示模式（桌面橫跨多個螢幕） |
| `internal` | 僅電腦螢幕（僅內建顯示器） |
| `external` | 僅外接螢幕（僅第二螢幕/外接顯示器） |
| `single:<id>` | 僅顯示於指定顯示器（ID格式：`low:high:id`） |

* * *

## 3\. 預設指令行為

### 3.1 啟動預設行為

當不帶任何參數啟動時：

```powershell
monitor-service.exe
```

**預設行為**：

- ✗ 不以服務模式執行
- ✓ 主控台視窗隱藏（預設隱藏）
- ✗ 不啟動 HTTP 伺服器（未指定 `--addr`）
- ✓ 監控強制執行已啟用（預設啟用）
- ✓ 自動脫離父主控台
- ✓ 程序設定初始化完成
- ✓ 日誌系統初始化完成

## 4\. 執行模式

### 快速啟動

| 你要什麼 | 一條命令 |
|---------|---------|
| 🖥️ 系統匣模式（無 Web） | `monitor-service.exe` |
| 🌐 系統匣模式 + Web 管理頁 | `monitor-service.exe --addr 0.0.0.0:3000` |
| ⚙️ 後台服務模式 | `sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone" start= auto` |

各模式的詳細說明及變體見下。

### 4.1 主控台模式（系統匣模式）

```powershell
# 顯示主控台視窗（除錯用）
monitor-service.exe --show-console --addr 0.0.0.0:3000

# 指定初始模式為延伸顯示
monitor-service.exe --mode extend --addr 0.0.0.0:3000
```

### 4.2 服務模式

```powershell
# 建立並啟動服務，在登入畫面強制顯示模式為同步顯示
sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only" start= auto
sc.exe start MonitorService

# 停止並刪除服務
sc.exe stop MonitorService
sc.exe delete MonitorService
```

### 4.3 Agent 代理模式（內部使用）

```powershell
# 列出顯示器（由主程序自動呼叫）
monitor-service.exe agent list

# 設定顯示模式（由主程序自動呼叫）
monitor-service.exe agent set clone
monitor-service.exe agent set extend
```

* * *

## 5\. 典型使用場景

> 以下四種場景按複雜度遞增排列，使用者可依需求選擇最適合的部署方案。

* * *

### 🏠 場景一：個人使用者純系統匣模式

**適用對象**：普通家庭使用者、個人辦公電腦

**核心需求**：

- 雙擊啟動，無感執行
- 右下角系統匣一鍵切換顯示設定
- 防止顯示器更換後的「顯示漂移」問題
- 可鎖定常用顯示模式

**部署方式**：

```powershell
# 方式A：直接雙擊 monitor-service.exe
# 方式B：建立捷徑，新增到啟動資料夾
shell:startup
```

**操作說明**：

1.  啟動後程序自動最小化到系統匣
2.  右鍵系統匣圖示 → 選擇需要的顯示模式
3.  開啟「Runtime Monitoring」後，即使按 `Win+P` 修改也會被自動回彈
4.  更換顯示器或顯示卡驅動後，模式不會「亂跑」

* * *

### 🏫 場景二：公共場所純服務模式（Windows 登入畫面兜底）

**適用對象**：學校教室電腦、圖書館公共電腦、機房終端

**核心需求**：

- **無人值守**：僅在鎖屏/登入畫面強制同步顯示模式
- **不干擾教學**：使用者登入後完全自主，教師可自由切換
- **防範學生亂改**：防止學生在課前/課後亂按 `Win+P` 導致下一位老師無法登入

**部署方式**：

```powershell
# 以系統管理員身分執行 PowerShell

# 建立服務：僅在 Session 0 / 鎖屏狀態強制執行同步顯示模式
sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only" start= auto displayName= "Monitor Manager Service"

sc.exe start MonitorService

# 如需停止/刪除服務：
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**工作流程**：

| 狀態 | 顯示模式 | 說明 |
| --- | --- | --- |
| 開機啟動 → Windows 登入畫面 | **強制同步顯示** | 確保登入畫面同時顯示於電腦和電視 |
| 學生亂按 Win+P | **自動回彈同步顯示** | 服務模式在後台守護 |
| 教師登入進入桌面 | **完全自主** | 服務模式退讓，教師可依需求切換 |
| 教師登出/鎖屏 | **恢復強制同步顯示** | 回到 Windows 登入畫面，再次兜底 |

**效果**：教師在教學過程中擁有完全控制權，學生無法在課前課後破壞顯示設定影響後續使用。

* * *

### 👨‍🏫 場景三：服務模式 + 個人化系統匣（依使用者記憶模式）

**適用對象**：多位教師共用一台電腦，每人有自己偏好的顯示模式

**核心需求**：

- Windows 登入畫面統一同步顯示（確保任何人都能登入）
- **不同教師登入後自動切換**到各自的偏好模式
- 教師 A 喜歡延伸顯示，教師 B 喜歡同步顯示，互不干擾

**部署方式**：

**步驟 1：安裝服務模式（兜底）**

```powershell
$exePath = "C:\monitor-service.exe"

sc.exe create MonitorService binPath= "$exePath --service --mode clone" start= auto

sc.exe start MonitorService

# 如需停止/刪除服務：
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**步驟 2：為每位教師建立個人化捷徑**

1.  右鍵 `monitor-service.exe` → 建立捷徑
2.  右鍵捷徑 → 內容 → **目標** 欄末尾新增參數：

```
# 教師 A（喜歡延伸顯示模式）
"C:\monitor-service.exe" --mode extend

# 教師 B（喜歡同步顯示模式）
"C:\monitor-service.exe" --mode clone
```

3.  將捷徑放入該教師的啟動資料夾（`shell:startup`）

**工作流程**：

```
開機 → Windows 登入畫面 → [服務模式] 強制同步顯示
        ↓
教師 A 登入 → [系統匣模式] 自動切換延伸顯示 → A 使用延伸顯示教學
        ↓
教師 A 登出 → 回到 Windows 登入畫面 → [服務模式] 恢復同步顯示
        ↓
教師 B 登入 → [系統匣模式] 自動切換同步顯示 → B 使用同步顯示教學
```

* * *

### 🛡️ 場景四：服務模式 + 統一系統匣（全域預設配置）

**適用對象**：學校機房，為所有教師提供**統一的預設顯示模式**，但允許依需求自行調整

**核心需求**：

- Windows 登入畫面同步顯示兜底（確保登入畫面可見）
- 所有教師登入後**預設**使用同一種顯示模式（如延伸顯示）
- **非強制鎖定**：教師可透過系統匣選單自由切換，也可關閉強制執行
- 提供一致性體驗，減少「每次登入都要調」的麻煩

**與場景三的區別**：

- 場景三：每位教師**不同**預設模式（A老師用延伸顯示，B老師用同步顯示）
- 場景四：所有教師**相同**預設模式（統一從延伸顯示開始，想改自己改）

**部署方式**：

**步驟 1：安裝服務模式（Windows 登入畫面兜底）**

```powershell
$exePath = "C:\monitor-service.exe"

sc.exe create MonitorService binPath= "$exePath --service --mode clone" start= auto

sc.exe start MonitorService

# 如需停止/刪除服務：
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**步驟 2：為所有使用者配置統一預設系統匣**

方法 A：群組原則（推薦）

- 透過 GPO 將捷徑部署到所有使用者的 `shell:startup`
- 捷徑目標：`"$exePath" --mode extend --hide-exit`

方法 B：排程工作（使用者層級）

```powershell
# 以系統管理員身分執行 PowerShell

# 建立排程工作：任何使用者登入時啟動系統匣模式（預設延伸顯示，非強制）
$exePath = "C:\monitor-service.exe"
$action = New-ScheduledTaskAction -Execute "$exePath" -Argument "--mode extend --hide-exit"
$trigger = New-ScheduledTaskTrigger -AtLogOn
$principal = New-ScheduledTaskPrincipal -GroupId "INTERACTIVE" -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan) -MultipleInstances Parallel
Register-ScheduledTask -TaskName "MonitorTray_AutoStart" -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force
```

**工作流程**：

| 狀態 | 顯示模式 | 說明 |
| --- | --- | --- |
| 開機啟動 → Windows 登入畫面 | **強制同步顯示** | 服務模式兜底，確保登入畫面可見 |
| 教師登入 → 進入桌面 | **預設延伸顯示** | 系統匣自動啟動，設為預設延伸顯示 |
| 教師需要同步顯示 | **自行切換** | 右鍵系統匣 → 切換到同步顯示，自由決定 |
| 教師登出/鎖屏 | **恢復強制同步顯示** | 回到 Windows 登入畫面，服務接管 |

**特點**：

- ✅ 提供統一的**初始體驗**（大家都從延伸顯示開始）
- ✅ **不強制**：教師可隨時透過系統匣切換到自己需要的模式
- ✅ 避免每次登入都要重新調整的麻煩
- ✅ Windows 登入畫面由服務統一保障（同步顯示）

&nbsp;

* * *

## 6\. 快速鍵操作

全域快速鍵在所有模式下都可用：

| 快速鍵組合 | 功能 |
| --- | --- |
| `Alt + Shift + Q` | 切換到「僅電腦螢幕」模式 |
| `Alt + Shift + W` | 切換到「同步顯示」模式 (Clone) |
| `Alt + Shift + E` | 切換到「延伸顯示」模式 (Extend) |
| `Alt + Shift + R` | 切換到「僅外接螢幕」模式 |
| `Alt + Shift + T` | 切換「執行時監控」開關 |

**快速鍵特點**：

- 全域註冊，任何視窗都有效
- 切換失敗時不會回滾目標狀態
- 強制執行循環會持續重試直到成功
- 切換後系統匣圖示會自動重新整理

* * *

## 7\. 系統匣選單說明

在主控台模式下，系統匣會顯示圖示，右鍵選單包含：

| 選單項目 | 功能 |
| --- | --- |
| 僅電腦螢幕 | 僅使用內建顯示器 |
| 同步顯示 | 切換到同步顯示（所有螢幕顯示相同畫面） |
| 延伸顯示 | 切換到延伸顯示（桌面橫跨多螢幕） |
| 僅外接螢幕 | 僅使用外接顯示器 |
| ──── | 分隔線 |
| 執行時監控 | 開關執行時模式保護 |
| ──── | 分隔線 |
| 開啟 Web 主控台 | 在瀏覽器中開啟管理頁（需 `--addr`） |
| ──── | 分隔線 |
| 關於 | 顯示版本、作者等資訊 |
| 退出 | 退出程序（可透過 `--hide-exit` 隱藏） |

**系統匣特點**：

- 選單會顯示目前啟用模式（✓ 標記）
- 「執行時監控」顯示目前狀態（✓ 表示啟用）
- 每次切換後自動重新整理圖示狀態
- 系統匣圖示從系統 DLL 提取（與系統顯示設定圖示一致）

* * *

## 8\. API 介面說明

當指定 `--addr` 啟動時，可透過 HTTP 存取：

### 8.1 取得顯示器列表

```http
GET /api/monitors

Response:
[
  {
    "id": "0:0:12345",
    "name": "DELL U2720Q",
    "width": 3840,
    "height": 2160,
    "is_primary": true
  }
]
```

### 8.2 取得目前顯示設定

```http
GET /api/topology

Response:
{
  "mode": "Extend"
}
```

### 8.3 設定顯示設定

```http
POST /api/topology
Content-Type: application/json

{
  "mode": "Clone" | "Extend" | "Internal" | "External"
}
```

### 8.4 取得服務狀態

```http
GET /api/status

Response:
{
  "mode": "Extend",
  "monitoring": true,
  "version": "V0.2.0"
}
```

### 8.5 Web 管理介面

```http
GET /
# 返回嵌入式 Web UI 頁面
```

* * *

## 9\. 服務安裝指南

### 9.1 快速安裝（場景二推薦）

```powershell
# 以系統管理員身分執行 PowerShell

$exePath = "C:\monitor-service.exe"

# 建立並啟動服務（Session 0 兜底模式）
sc.exe create MonitorService binPath= "$exePath --service --mode clone --enforce-session0-only" start= auto displayName= "Monitor Manager Service"

sc.exe start MonitorService

# 停止並刪除服務
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**注意**：`binPath=` 等號後必須有一個空格。

### 9.2 僅無使用者桌面時強制執行

```powershell
# 服務模式：僅在 Windows 登入畫面 / 預登入狀態時執行 enforcement
# 當使用者已登入桌面後自動暫停，避免與使用者操作衝突
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --mode clone --enforce-session0-only" start= auto
sc.exe start MonitorService

# 停止並刪除服務
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**運作原理**：透過 `WTSQuerySessionInformationW(WTSUserName + WTSSessionInfoEx)` 組合判斷：

| 狀態 | enforcement |
| --- | --- |
| 有真實使用者名稱 + 螢幕未鎖 | ❌ 跳過（使用者正在使用桌面） |
| 有真實使用者名稱 + 螢幕鎖定（Win+L） | ✅ 執行（鎖屏期間服務接管） |
| 空/SYSTEM（Windows 登入畫面/預登入） | ✅ 執行 |
| API 呼叫失敗 | ✅ 執行（保守策略） |

### 9.3 帶日誌的服務

```powershell
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --mode clone --log-path C:\logs\monitor.log --interval 2" start= auto
sc.exe start MonitorService

# 停止並刪除服務
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

### 9.4 解除安裝服務

```powershell
sc.exe stop MonitorService
sc.exe delete MonitorService
```

* * *

## 10\. 日誌與除錯

### 10.1 啟用日誌

```powershell
# 主控台模式
monitor-service.exe --log-path "C:\logs\monitor.log"

# 服務模式
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --log-path C:\logs\monitor.log"
```

### 10.2 日誌格式

```
[2026-05-01 17:55:12] ============================================================
[2026-05-01 17:55:12] Monitor Service V0.2.0 Started
[2026-05-01 17:55:12] CLI Options: Some(Clone)
[2026-05-01 17:55:12] ============================================================
[2026-05-01 17:55:12] Running in console mode
[2026-05-01 17:55:12] Enforcement loop started (interval: 2s)
[2026-05-01 17:55:14] Topology set via hotkey: Clone
```

### 10.3 日誌事件類型

| 事件 | 說明 |
| --- | --- |
| `Header/Footer` | 服務啟動/停止標記 |
| `Enforcement loop started` | 強制執行循環啟動 |
| `Mismatch detected` | 偵測到顯示模式不匹配 |
| `Successfully corrected` | 成功修正顯示模式 |
| `Heartbeat: GC hint` | 記憶體回收心跳（每12週期） |
| `Topology set via hotkey` | 透過快速鍵切換模式 |

* * *

## 📌 快速參考卡

### 場景速查表

| 場景 | 安裝命令 | 關鍵參數 |
| --- | --- | --- |
| **個人系統匣** | 雙擊執行 | 無 |
| **Windows 登入畫面兜底** | `sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only"` | `--enforce-session0-only` |
| **個人化系統匣** | 服務 + 使用者層級捷徑 | `--mode` 個人化 |
| **統一管控** | 服務 + 排程工作/GPO | `--mode` 統一 |

### 故障排查

```powershell
# 檢視服務狀態
sc.exe query MonitorService

# 檢視即時日誌
Get-Content "C:\logs\monitor.log" -Tail 20 -Wait

# 手動測試系統匣模式
monitor-service.exe --show-console --mode clone
```

* * *

*文件版本: V0.2.1 | 更新日期: 2026-05-03*

* * *

**致謝**

本專案 Fork 自 [github.com/Seryta/windows-monitor-manager](https://github.com/Seryta/windows-monitor-manager)，感謝原作者的開源貢獻。
