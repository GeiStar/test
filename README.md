# Windows Monitor Manager Auto - User Manual

> **Language**: Rust  
> **Supported Platforms**: Windows 10/11

## 🌍 Language Switcher

- [English](README.md) (Current)
- [简体中文](README_zh-CN.md) 
- [繁體中文](README_zh-TW.md)

* * *

## 📋 Table of Contents

1.  [Project Introduction](#1-project-introduction)
2.  [Command Line Parameters](#2-command-line-parameters)
3.  [Default Command Behavior](#3-default-command-behavior)
4.  [Operation Modes](#4-operation-modes)
5.  [Typical Usage Scenarios](#5-typical-usage-scenarios)
6.  [Hotkey Operations](#6-hotkey-operations)
7.  [System Tray Menu](#7-system-tray-menu)
8.  [API Interface](#8-api-interface)
9.  [Service Installation Guide](#9-service-installation-guide)
10. [Logging and Debugging](#10-logging-and-debugging)

* * *

## 1\. Project Introduction

### 1.1 Background

In public educational environments such as schools, students frequently use `Win+P` to randomly change display topology, causing:

- Images displayed only on classroom TVs while the teacher's computer screen goes black
- Login screens becoming invisible due to incorrect display mode settings
- Disruption of normal teaching activities

**Windows Monitor Manager** was developed to address these issues — by **forcefully locking display modes** (typically Clone/Duplicate mode), ensuring the system can **self-heal** even after malicious or accidental modifications, guaranteeing multi-monitor availability at all times.

### 1.2 Core Capabilities

| Capability | Description |
| --- | --- |
| 🔄 **Auto-Recovery** | Real-time monitoring of display mode; automatically reverts to target mode when tampered with |
| 🔒 **Force Lock** | Service mode provides background protection; tray mode provides convenient foreground control |
| ⌨️ **Global Hotkeys** | One-key switching with `Alt+Shift+W/E` shortcuts for efficient operation |
| 👤 **User-Aware** | Supports Session 0 isolation; does not interfere with normal user desktop operations |
| 🖥️ **Multi-Scenario Support** | Covers everything from personal home use to school computer labs with four operation modes |

* * *

## 2\. Command Line Parameters

### Basic Syntax

```powershell
monitor-service.exe [OPTIONS] [COMMAND]
```

### Available Parameters

| Parameter | Short | Default | Description |
| --- | --- | --- | --- |
| `--service` | \- | None | Run as Windows service |
| `--addr 0.0.0.0:3000` | \- | None | Listen address and port (HTTP not started if omitted) |
| `--mode <MODE>` | `-m` | Current system mode | Initial display mode |
| `--interval <SECONDS>` | `-i` | `2` | Monitoring interval (seconds) |
| `--log-path <PATH>` | `-l` | None | Log file path |
| `--no-enforce` | \- | No | Disable runtime monitoring (enforcement enabled by default) |
| `--enforce-session0-only` | \- | No | Only enforce when "no user" or "screen locked"; skip when user is logged in and screen unlocked (service mode only) |
| `--show-console` | \- | No | Show console window |
| `--hide-exit` | \- | No | Hide the Exit button in the tray menu |
| `--relaunched` | \- | \- | Internal parameter (users do not need to use) |
| `--help` | `-h` | \- | Show help information |

### Subcommands

| Subcommand | Description |
| --- | --- |
| `agent` | Run as agent (internal use, for session injection) |
| `agent list` | List all monitors |
| `agent set <mode>` | Set display topology mode |

### Mode Parameter `<MODE>` Values

| Mode Value | Description |
| --- | --- |
| `clone` | Duplicate mode (same image on all displays) |
| `extend` | Extend mode (desktop spans across multiple displays) |
| `internal` | Internal display only |
| `external` | External display only |
| `single:<id>` | Display on specified monitor only (ID format: `low:high:id`) |

* * *

## 3\. Default Command Behavior

### 3.1 Default Startup Behavior

When started without any parameters:

```powershell
monitor-service.exe
```

**Default behavior**:

- ✗ Does not run as a service
- ✓ Console window hidden (hidden by default)
- ✗ HTTP server not started (no `--addr` specified)
- ✓ Runtime monitoring enforcement enabled (enabled by default)
- ✓ Automatically detaches from parent console
- ✓ Process initialization complete
- ✓ Log system initialization complete

## 4\. Operation Modes

### Quick Start

| What you want | One-liner command |
|---------|---------|
| 🖥️ Tray mode (no Web) | `monitor-service.exe` |
| 🌐 Tray mode + Web management UI | `monitor-service.exe --addr 0.0.0.0:3000` |
| ⚙️ Background service mode | `sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone" start= auto` |

See below for detailed descriptions and variants of each mode.

### 4.1 Console Mode (Tray Mode)

```powershell
# Show console window (debugging)
monitor-service.exe --show-console --addr 0.0.0.0:3000

# Specify initial mode as Extend
monitor-service.exe --mode extend --addr 0.0.0.0:3000
```

### 4.2 Service Mode

```powershell
# Create and start service (enforces Clone mode on lock screen)
sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only" start= auto
sc.exe start MonitorService

# Stop and delete service
sc.exe stop MonitorService
sc.exe delete MonitorService
```

### 4.3 Agent Mode (Internal Use)

```powershell
# List monitors (called automatically by main process)
monitor-service.exe agent list

# Set display mode (called automatically by main process)
monitor-service.exe agent set clone
monitor-service.exe agent set extend
```

* * *

## 5\. Typical Usage Scenarios

> The following four scenarios are arranged in increasing complexity. Choose the deployment that best fits your needs.

* * *

### 🏠 Scenario 1: Personal Tray-Only Mode

**Target users**: Home users, personal office computers

**Core requirements**:

- Double-click to run, operates silently in background
- One-click display topology switch via system tray
- Prevent "display drift" after monitor changes
- Lock preferred display mode

**Deployment**:

```powershell
# Option A: Double-click monitor-service.exe
# Option B: Create shortcut, add to startup folder
shell:startup
```

**Usage**:

1.  Program auto-minimizes to system tray on startup
2.  Right-click tray icon → select desired display mode
3.  With "Runtime Monitoring" enabled, even `Win+P` changes will auto-revert
4.  After changing monitors or GPU drivers, modes won't "drift"

* * *

### 🏫 Scenario 2: Public Service-Only Mode (Winlogon Fallback)

**Target users**: School classroom computers, library public PCs, lab terminals

**Core requirements**:

- **Unattended**: Enforce Clone mode only on lock screen / login screen
- **Non-intrusive**: Full autonomy after user login; teachers can switch freely
- **Tamper-proof**: Prevent students from breaking display settings before/after class

**Deployment**:

```powershell
# Run PowerShell as Administrator

# Create service: enforce Clone only in Session 0 / lock screen state
sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only" start= auto displayName= "Monitor Manager Service"

sc.exe start MonitorService

# To stop/delete service:
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**Workflow**:

| State | Display Mode | Description |
| --- | --- | --- |
| Boot → Winlogon login screen | **Force Clone** | Ensures login screen visible on both PC and TV |
| Student presses Win+P | **Auto-revert to Clone** | Service mode guards in the background |
| Teacher logs into desktop | **Full autonomy** | Service steps back; teacher can switch freely |
| Teacher logs off / locks screen | **Restore Force Clone** | Returns to Winlogon, service takes over |

* * *

### 👨‍🏫 Scenario 3: Service + Personalized Tray (Per-User Mode Memory)

**Target users**: Multiple teachers sharing one computer, each with their own preferred display mode

**Core requirements**:

- Unified Clone at Winlogon (ensures everyone can log in)
- **Auto-switch** to each teacher's preferred mode upon login
- Teacher A prefers Extend, Teacher B prefers Clone — no interference

**Deployment**:

**Step 1: Install service mode (fallback)**

```powershell
$exePath = "C:\monitor-service.exe"

sc.exe create MonitorService binPath= "$exePath --service --mode clone" start= auto

sc.exe start MonitorService

# To stop/delete service:
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**Step 2: Create personalized shortcuts for each teacher**

1.  Right-click `monitor-service.exe` → Create shortcut
2.  Right-click shortcut → Properties → append parameters to the **Target** field:

```
# Teacher A (prefers Extend mode)
"C:\monitor-service.exe" --mode extend

# Teacher B (prefers Clone mode)
"C:\monitor-service.exe" --mode clone
```

3.  Place shortcut in the respective teacher's startup folder (`shell:startup`)

**Workflow**:

```
Boot → Winlogon → [Service] Force Clone
        ↓
Teacher A logs in → [Tray] Auto-switch to Extend → A teaches with Extend
        ↓
Teacher A logs off → back to Winlogon → [Service] Restore Clone
        ↓
Teacher B logs in → [Tray] Auto-switch to Clone → B teaches with Clone
```

* * *

### 🛡️ Scenario 4: Service + Unified Tray (Global Default)

**Target users**: School computer labs, providing **a unified default display mode** for all teachers while allowing individual adjustment

**Core requirements**:

- Clone fallback at Winlogon (ensures login screen is visible)
- All teachers start with the **same default** display mode (e.g., Extend)
- **Non-enforced**: teachers can switch freely via tray menu or disable enforcement
- Consistent experience, reducing the need to re-adjust settings on every login

**Key difference from Scenario 3**:

- Scenario 3: Each teacher has a **different** default mode (Teacher A uses Extend, Teacher B uses Clone)
- Scenario 4: All teachers share the **same** default mode (everyone starts from Extend, can change as needed)

**Deployment**:

**Step 1: Install service mode (Winlogon fallback)**

```powershell
$exePath = "C:\monitor-service.exe"

sc.exe create MonitorService binPath= "$exePath --service --mode clone" start= auto

sc.exe start MonitorService

# To stop/delete service:
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**Step 2: Configure unified default tray for all users**

Option A: Group Policy (recommended)

- Deploy shortcut to all users' `shell:startup` via GPO
- Shortcut target: `"$exePath" --mode extend --hide-exit`

Option B: Scheduled Task (per-user)

```powershell
# Run PowerShell as Administrator

# Create scheduled task: starts tray mode on any user login (default Extend, non-enforced)
$exePath = "C:\monitor-service.exe"
$action = New-ScheduledTaskAction -Execute "$exePath" -Argument "--mode extend --hide-exit"
$trigger = New-ScheduledTaskTrigger -AtLogOn
$principal = New-ScheduledTaskPrincipal -GroupId "INTERACTIVE" -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan) -MultipleInstances Parallel
Register-ScheduledTask -TaskName "MonitorTray_AutoStart" -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force
```

**Workflow**:

| State | Display Mode | Description |
| --- | --- | --- |
| Boot → Winlogon | **Force Clone** | Service mode fallback, ensures login screen is visible |
| Teacher logs in → Desktop | **Default Extend** | Tray auto-starts, set to default Extend |
| Teacher needs Clone | **Switch freely** | Right-click tray → switch to Clone |
| Teacher logs off / locks screen | **Restore Force Clone** | Back to Winlogon, service takes over |

**Features**:

- ✅ Unified **starting experience** (everyone starts from Extend)
- ✅ **Non-enforced**: teachers can switch to their preferred mode at any time
- ✅ No need to readjust settings on every login
- ✅ Winlogon screen guaranteed by service (Clone)

&nbsp;

* * *

## 6\. Hotkey Operations

Global hotkeys are available in all modes:

| Hotkey Combination | Function |
| --- | --- |
| `Alt + Shift + Q` | Switch to "Internal display only" |
| `Alt + Shift + W` | Switch to "Clone" mode (Duplicate) |
| `Alt + Shift + E` | Switch to "Extend" mode |
| `Alt + Shift + R` | Switch to "External display only" |
| `Alt + Shift + T` | Toggle "Runtime Monitoring" |

**Hotkey features**:

- Globally registered, effective in any window
- Does not roll back target state on failure
- Enforcement loop will retry until success
- Tray icon auto-refreshes after switching

* * *

## 7\. System Tray Menu

In console mode, the system tray displays an icon. Right-click menu:

| Menu Item | Function |
| --- | --- |
| Internal | Use built-in display only |
| Clone | Switch to duplicate display |
| Extend | Switch to extended display |
| External | Use external display only |
| ──── | Separator |
| Runtime Monitoring | Toggle runtime mode protection |
| ──── | Separator |
| Open Web Console | Open management page in browser (requires `--addr`) |
| ──── | Separator |
| About | Show version, author and other information |
| Exit | Exit program (can be hidden via `--hide-exit`) |

**Tray features**:

- Menu shows currently active mode (✓ mark)
- "Runtime Monitoring" shows current state (✓ means enabled)
- Tray icon auto-refreshes after each switch
- Tray icon extracted from system DLL (same as system display settings icon)

* * *

## 8\. API Interface

When started with `--addr`, HTTP access is available:

### 8.1 Get Monitor List

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

### 8.2 Get Current Topology

```http
GET /api/topology

Response:
{
  "mode": "Extend"
}
```

### 8.3 Set Display Topology

```http
POST /api/topology
Content-Type: application/json

{
  "mode": "Clone" | "Extend" | "Internal" | "External"
}
```

### 8.4 Get Service Status

```http
GET /api/status

Response:
{
  "mode": "Extend",
  "monitoring": true,
  "version": "V0.2.0"
}
```

### 8.5 Web Management UI

```http
GET /
# Returns embedded Web UI page
```

* * *

## 9\. Service Installation Guide

### 9.1 Quick Install (Recommended for Scenario 2)

```powershell
# Run PowerShell as Administrator

$exePath = "C:\monitor-service.exe"

# Create and start service (Session 0 fallback mode)
sc.exe create MonitorService binPath= "$exePath --service --mode clone --enforce-session0-only" start= auto displayName= "Monitor Manager Service"

sc.exe start MonitorService

# To stop/delete service:
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**Note**: There must be a space after the `=` in `binPath=`.

### 9.2 No-User Desktop Enforcement Only

```powershell
# Service mode: enforce only in Winlogon / pre-login state
# Automatically pauses when user is logged into desktop to avoid conflicts
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --mode clone --enforce-session0-only" start= auto
sc.exe start MonitorService

# To stop/delete service:
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**How it works**: Uses `WTSQuerySessionInformationW(WTSUserName + WTSSessionInfoEx)` to determine state:

| State | Enforcement |
| --- | --- |
| Real username present + screen unlocked | ❌ Skip (user actively using desktop) |
| Real username present + screen locked (Win+L) | ✅ Enforce (service takes over during lock screen) |
| Empty/SYSTEM (Winlogon/pre-login) | ✅ Enforce |
| API call failure | ✅ Enforce (conservative fallback) |

### 9.3 Service with Logging

```powershell
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --mode clone --log-path C:\logs\monitor.log --interval 2" start= auto
sc.exe start MonitorService

# To stop/delete service:
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

### 9.4 Uninstall Service

```powershell
sc.exe stop MonitorService
sc.exe delete MonitorService
```

* * *

## 10\. Logging and Debugging

### 10.1 Enable Logging

```powershell
# Console mode
monitor-service.exe --log-path "C:\logs\monitor.log"

# Service mode
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --log-path C:\logs\monitor.log"
```

### 10.2 Log Format

```
[2026-05-01 17:55:12] ============================================================
[2026-05-01 17:55:12] Monitor Service V0.2.0 Started
[2026-05-01 17:55:12] CLI Options: Some(Clone)
[2026-05-01 17:55:12] ============================================================
[2026-05-01 17:55:12] Running in console mode
[2026-05-01 17:55:12] Enforcement loop started (interval: 2s)
[2026-05-01 17:55:14] Topology set via hotkey: Clone
```

### 10.3 Log Event Types

| Event | Description |
| --- | --- |
| `Header/Footer` | Service start/stop markers |
| `Enforcement loop started` | Enforcement loop has started |
| `Mismatch detected` | Display mode mismatch detected |
| `Successfully corrected` | Display mode successfully corrected |
| `Heartbeat: GC hint` | Memory GC heartbeat (every 12 cycles) |
| `Topology set via hotkey` | Mode switched via hotkey |

* * *

## 📌 Quick Reference Card

### Scenario Quick-Reference Table

| Scenario | Installation Command | Key Parameter |
| --- | --- | --- |
| **Personal Tray** | Double-click to run | None |
| **Winlogon Fallback** | `sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only"` | `--enforce-session0-only` |
| **Personalized Tray** | Service + per-user shortcuts | `--mode` personalization |
| **Unified Management** | Service + Scheduled Task/GPO | `--mode` unified |

### Troubleshooting

```powershell
# Check service status
sc.exe query MonitorService

# View real-time logs
Get-Content "C:\logs\monitor.log" -Tail 20 -Wait

# Manually test tray mode
monitor-service.exe --show-console --mode clone
```

* * *

*Document Version: V0.2.1 | Updated: 2026-05-03*

* * *

**Acknowledgments**

This project is forked from [github.com/Seryta/windows-manager-monitor](https://github.com/Seryta/windows-manager-monitor). Thanks to the original author for their open-source contribution.
