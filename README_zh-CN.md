# Windows Monitor Manager Auto - 使用说明书

> **语言**: Rust  
> **适用平台**: Windows 10/11

## 🌍 语言切换

- [English](README.md)
- [简体中文](README_zh-CN.md) (当前)
- [繁體中文](README_zh-TW.md)

* * *

## 📋 目录

1.  [项目简介](#1-%E9%A1%B9%E7%9B%AE%E7%AE%80%E4%BB%8B)
2.  [命令行参数说明](#2-%E5%91%BD%E4%BB%A4%E8%A1%8C%E5%8F%82%E6%95%B0%E8%AF%B4%E6%98%8E)
3.  [默认指令行为](#3-%E9%BB%98%E8%AE%A4%E6%8C%87%E4%BB%A4%E8%A1%8C%E4%B8%BA)
4.  [运行模式](#4-%E8%BF%90%E8%A1%8C%E6%A8%A1%E5%BC%8F)
5.  [典型使用场景](#5-%E5%85%B8%E5%9E%8B%E4%BD%BF%E7%94%A8%E5%9C%BA%E6%99%AF)
6.  [热键操作](#6-%E7%83%AD%E9%94%AE%E6%93%8D%E4%BD%9C)
7.  [托盘菜单说明](#7-%E6%89%98%E7%9B%98%E8%8F%9C%E5%8D%95%E8%AF%B4%E6%98%8E)
8.  [API 接口说明](#8-api-%E6%8E%A5%E5%8F%A3%E8%AF%B4%E6%98%8E)
9.  [服务安装指南](#9-%E6%9C%8D%E5%8A%A1%E5%AE%89%E8%A3%85%E6%8C%87%E5%8D%97)
10. [日志与调试](#10-%E6%97%A5%E5%BF%97%E4%B8%8E%E8%B0%83%E8%AF%95)

* * *

## 1\. 项目简介

### 1.1 诞生背景

在学校等公共教学场景中，学生经常通过 `Win+P` 胡乱调整显示拓扑（Display Topology），导致：

- 画面只在教室电视显示，教师电脑黑屏无法操作
- 登录界面因显示模式错误而无法看到输入框
- 影响正常教学工作的开展

**Windows Monitor Manager** 应运而生——用于**强制锁定显示模式**（通常是复制模式 Clone），确保即使被恶意或误操作修改，系统也能**自我修复**，保障多屏显示始终可用。

### 1.2 核心能力

| 能力  | 说明  |
| --- | --- |
| 🔄 **自动修复** | 实时监控显示模式，被篡改后自动回弹到目标模式 |
| 🔒 **强制锁定** | 服务模式在后台兜底，托盘模式在前台便捷控制 |
| ⌨️ **全局热键** | `Alt+Shift+W/E` 等快捷键一键切换，高效无忧 |
| 👤 **用户感知** | 支持 Session0 隔离，用户桌面内不干扰正常操作 |
| 🖥️ **多场景适配** | 从个人家用到学校机房，四种模式全覆盖 |

* * *

## 2\. 命令行参数说明

### 基本语法

```powershell
monitor-service.exe [OPTIONS] [COMMAND]
```

### 可用参数列表

| 参数  | 简写  | 默认值 | 说明  |
| --- | --- | --- | --- |
| `--service` | \-  | 无   | 以 Windows 服务模式运行 |
| `--addr 0.0.0.0:3000` | \-  | 无   | 监听地址和端口（不填不启动 HTTP） |
| `--mode <MODE>` | `-m` | 系统当前模式 | 初始显示模式 |
| `--interval <SECONDS>` | `-i` | `2` | 监控间隔（秒） |
| `--log-path <PATH>` | `-l` | 无   | 日志文件路径 |
| `--no-enforce` | \-  | 否   | 禁用运行时监控（默认启用强制执行） |
| `--enforce-session0-only` | \-  | 否   | 仅在「无用户」或「用户锁屏」时强制执行；用户已登录且未锁屏时跳过（服务模式专用） |
| `--show-console` | \-  | 否   | 显示控制台窗口 |
| `--hide-exit` | \-  | 否   | 隐藏托盘菜单中的退出按钮 |
| `--relaunched` | \-  | \-  | 内部参数（用户无需使用） |
| `--help` | `-h` | \-  | 显示帮助信息 |

### 子命令

| 子命令 | 说明  |
| --- | --- |
| `agent` | 作为代理运行（内部使用，用于会话注入） |
| `agent list` | 列出所有显示器 |
| `agent set <mode>` | 设置显示拓扑模式 |

### 模式参数 `<MODE>` 可选值

| 模式值 | 说明  |
| --- | --- |
| `clone` | 复制模式（Duplicate） |
| `extend` | 扩展模式 |
| `internal` | 仅电脑屏幕 |
| `external` | 仅第二屏幕 |
| `single:<id>` | 仅显示在指定显示器（ID格式：`low:high:id`） |

* * *

## 3\. 默认指令行为

### 3.1 启动默认行为

当不带任何参数启动时：

```powershell
monitor-service.exe
```

**默认行为**：

- ✗ 不以服务模式运行
- ✓ 控制台窗口隐藏（默认隐藏）
- ✗ 不启动 HTTP 服务器（未指定 `--addr`）
- ✓ 监控强制执行已启用（默认启用）
- ✓ 自动脱离父控制台
- ✓ 进程设置初始化完成
- ✓ 日志系统初始化完成

## 4\. 运行模式

### 快速启动

| 你要什么 | 一条命令 |
|---------|---------|
| 🖥️ 托盘模式（无 Web） | `monitor-service.exe` |
| 🌐 托盘模式 + Web 管理页 | `monitor-service.exe --addr 0.0.0.0:3000` |
| ⚙️ 后台服务模式 | `sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone" start= auto` |

各模式的详细说明及变体见下。

### 4.1 控制台模式（托盘模式）

```powershell
# 显示控制台窗口（调试用）
monitor-service.exe --show-console --addr 0.0.0.0:3000

# 指定初始模式为扩展
monitor-service.exe --mode extend --addr 0.0.0.0:3000
```

### 4.2 服务模式

```powershell
# 创建并启动服务,在锁屏界面强制显示模式为clone
sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only" start= auto
sc.exe start MonitorService

# 停止并删除服务
sc.exe stop MonitorService
sc.exe delete MonitorService
```

### 4.3 Agent 代理模式（内部使用）

```powershell
# 列出显示器（由主进程自动调用）
monitor-service.exe agent list

# 设置显示模式（由主进程自动调用）
monitor-service.exe agent set clone
monitor-service.exe agent set extend
```

* * *

## 5\. 典型使用场景

> 以下四种场景按复杂度递增排列，用户可按需选择最适合的部署方案。

* * *

### 🏠 场景一：个人用户纯托盘模式

**适用对象**：普通家庭用户、个人办公电脑

**核心需求**：

- 双击启动，无感运行
- 右下角托盘一键切换显示拓扑
- 防止显示器更换后的"显示漂移"问题
- 可锁定常用显示模式

**部署方式**：

```powershell
# 方式A：直接双击 monitor-service.exe
# 方式B：创建快捷方式，添加到启动文件夹
shell:startup
```

**操作说明**：

1.  启动后程序自动最小化到系统托盘
2.  右键托盘图标 → 选择需要的显示模式
3.  开启「Runtime Monitoring」后，即使按 `Win+P` 修改也会被自动回弹
4.  更换显示器或显卡驱动后，模式不会"乱跑"

* * *

### 🏫 场景二：公共场所纯服务模式（Winlogon 兜底）

**适用对象**：学校教室电脑、图书馆公共电脑、机房终端

**核心需求**：

- **无人值守**：仅在锁屏/登录界面强制 Clone 模式
- **不干扰教学**：用户登录后完全自主，教师可自由切换
- **防范学生乱改**：防止学生在课前/课后乱按 `Win+P` 导致下一位老师无法登录

**部署方式**：

```powershell
# 以管理员身份运行 PowerShell

# 创建服务：仅在 Session0/锁屏状态强制执行 Clone 模式
sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only" start= auto displayName= "Monitor Manager Service"

sc.exe start MonitorService

# 如需停止/删除服务：
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**工作流程**：

| 状态  | 显示模式 | 说明  |
| --- | --- | --- |
| 开机启动 → Winlogon 登录界面 | **强制 Clone** | 确保登录界面同时显示在电脑和电视 |
| 学生乱按 Win+P | **自动回弹 Clone** | 服务模式在后台守护 |
| 教师登录进入桌面 | **完全自主** | 服务模式退让，教师可按需切换 |
| 教师注销/锁屏 | **恢复强制 Clone** | 回到 Winlogon 界面，再次兜底 |

**效果**：教师在教学过程中拥有完全控制权，学生无法在课前课后破坏显示设置影响后续使用。

* * *

### 👨‍🏫 场景三：服务模式 + 个性化托盘（按用户记忆模式）

**适用对象**：多位教师共用一台电脑，每人有自己偏好的显示模式

**核心需求**：

- Winlogon 界面统一 Clone（确保任何人都能登录）
- **不同教师登录后自动切换**到各自的偏好模式
- 教师 A 喜欢 Extend，教师 B 喜欢 Clone，互不干扰

**部署方式**：

**步骤 1：安装服务模式（兜底）**

```powershell
$exePath = "C:\monitor-service.exe"

sc.exe create MonitorService binPath= "$exePath --service --mode clone" start= auto

sc.exe start MonitorService

# 如需停止/删除服务：
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**步骤 2：为每位教师创建个性化快捷方式**

1.  右键 `monitor-service.exe` → 创建快捷方式
2.  右键快捷方式 → 属性 → **目标** 栏末尾添加参数：

```
# 教师 A（喜欢扩展模式）
"C:\monitor-service.exe" --mode extend

# 教师 B（喜欢复制模式）
"C:\monitor-service.exe" --mode clone
```

3.  将快捷方式放入该教师的启动文件夹（`shell:startup`）

**工作流程**：

```
开机 → Winlogon 界面 → [服务模式] 强制 Clone
        ↓
教师 A 登录 → [托盘模式] 自动切换 Extend → A 使用 Extend 教学
        ↓
教师 A 注销 → 回到 Winlogon → [服务模式] 恢复 Clone
        ↓
教师 B 登录 → [托盘模式] 自动切换 Clone → B 使用 Clone 教学
```

* * *

### 🛡️ 场景四：服务模式 + 统一托盘（全局默认配置）

**适用对象**：学校机房，为所有教师提供**统一的默认显示模式**，但允许按需自行调整

**核心需求**：

- Winlogon 界面 Clone 兜底（确保登录界面可见）
- 所有教师登录后**默认**使用同一种显示模式（如 Extend）
- **非强制锁定**：教师可通过托盘菜单自由切换，也可关闭强制执行
- 提供一致性体验，减少"每次登录都要调"的麻烦

**与场景三的区别**：

- 场景三：每位教师**不同**默认模式（A老师用Extend，B老师用Clone）
- 场景四：所有教师**相同**默认模式（统一从Extend开始，想改自己改）

**部署方式**：

**步骤 1：安装服务模式（Winlogon兜底）**

```powershell
$exePath = "C:\monitor-service.exe"

sc.exe create MonitorService binPath= "$exePath --service --mode clone" start= auto

sc.exe start MonitorService

# 如需停止/删除服务：
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**步骤 2：为所有用户配置统一默认托盘**

方法 A：组策略（推荐）

- 通过 GPO 将快捷方式部署到所有用户的 `shell:startup`
- 快捷方式目标：`"$exePath" --mode extend --hide-exit`

方法 B：计划任务（用户级）

```powershell
# 以管理员身份运行 PowerShell

# 创建计划任务：任何用户登录时启动托盘模式（默认Extend，非强制）
$exePath = "C:\monitor-service.exe"
$action = New-ScheduledTaskAction -Execute "$exePath" -Argument "--mode extend --hide-exit"
$trigger = New-ScheduledTaskTrigger -AtLogOn
$principal = New-ScheduledTaskPrincipal -GroupId "INTERACTIVE" -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan) -MultipleInstances Parallel
Register-ScheduledTask -TaskName "MonitorTray_AutoStart" -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force
```

**工作流程**：

| 状态  | 显示模式 | 说明  |
| --- | --- | --- |
| 开机启动 → Winlogon | **强制 Clone** | 服务模式兜底，确保登录界面可见 |
| 教师登录 → 进入桌面 | **默认 Extend** | 托盘自动启动，设为默认Extend |
| 教师需要 Clone | **自行切换** | 右键托盘 → 切换到Clone，自由决定 |
| 教师注销/锁屏 | **恢复强制 Clone** | 回到Winlogon，服务接管 |

**特点**：

- ✅ 提供统一的**初始体验**（大家都从Extend开始）
- ✅ **不强制**：教师可随时通过托盘切换到自己需要的模式
- ✅ 避免每次登录都要重新调整的麻烦
- ✅ Winlogon界面由服务统一保障（Clone）

&nbsp;

* * *

## 6\. 热键操作

全局热键在所有模式下都可用：

| 热键组合 | 功能  |
| --- | --- |
| `Alt + Shift + Q` | 切换到「仅电脑屏幕」模式 |
| `Alt + Shift + W` | 切换到「复制」模式 (Clone) |
| `Alt + Shift + E` | 切换到「扩展」模式 (Extend) |
| `Alt + Shift + R` | 切换到「仅第二屏幕」模式 |
| `Alt + Shift + T` | 切换「运行时监控」开关 |

**热键特点**：

- 全局注册，任何窗口都有效
- 切换失败时不会回滚目标状态
- 强制执行循环会持续重试直到成功
- 切换后托盘图标会自动刷新

* * *

## 7\. 托盘菜单说明

在控制台模式下，系统托盘会显示图标，右键菜单包含：

| 菜单项 | 功能  |
| --- | --- |
| Internal | 仅使用内建显示器 |
| Clone | 切换到复制显示 |
| Extend | 切换到扩展显示 |
| External | 仅使用外接显示器 |
| ──── | 分隔线 |
| Runtime Monitoring | 开关运行时模式保护 |
| ──── | 分隔线 |
| Open Web Console | 在浏览器中打开管理页（需 `--addr`） |
| ──── | 分隔线 |
| About | 显示版本、作者等信息 |
| Exit | 退出程序（可通过 `--hide-exit` 隐藏） |

**托盘特点**：

- 菜单会显示当前激活模式（✓ 标记）
- 「Runtime Monitoring」显示当前状态（✓ 表示启用）
- 每次切换后自动刷新图标状态
- 托盘图标从系统 DLL 提取（与系统显示设置图标一致）

* * *

## 8\. API 接口说明

当指定 `--addr` 启动时，可通过 HTTP 访问：

### 8.1 获取显示器列表

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

### 8.2 获取当前拓扑

```http
GET /api/topology

Response:
{
  "mode": "Extend"
}
```

### 8.3 设置显示拓扑

```http
POST /api/topology
Content-Type: application/json

{
  "mode": "Clone" | "Extend" | "Internal" | "External"
}
```

### 8.4 获取服务状态

```http
GET /api/status

Response:
{
  "mode": "Extend",
  "monitoring": true,
  "version": "V0.2.0"
}
```

### 8.5 Web 管理界面

```http
GET /
# 返回嵌入式 Web UI 页面
```

* * *

## 9\. 服务安装指南

### 9.1 快速安装（场景二推荐）

```powershell
# 以管理员身份运行 PowerShell

$exePath = "C:\monitor-service.exe"

# 创建并启动服务（Session0 兜底模式）
sc.exe create MonitorService binPath= "$exePath --service --mode clone --enforce-session0-only" start= auto displayName= "Monitor Manager Service"

sc.exe start MonitorService

# 停止并删除服务
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**注意**：`binPath=` 等号后必须有一个空格。

### 9.2 仅无用户桌面时强制执行

```powershell
# 服务模式：仅在 Winlogon 界面/预登录状态时执行 enforcement
# 当用户已登录桌面后自动暂停，避免与用户操作冲突
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --mode clone --enforce-session0-only" start= auto
sc.exe start MonitorService

# 停止并删除服务
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

**工作原理**：通过 `WTSQuerySessionInformationW(WTSUserName + WTSSessionInfoEx)` 组合判断：

| 状态  | enforcement |
| --- | --- |
| 有真实用户名 + 屏幕未锁 | ❌ 跳过（用户正在使用桌面） |
| 有真实用户名 + 屏幕锁定（Win+L） | ✅ 执行（锁屏期间服务接管） |
| 空/SYSTEM（Winlogon/预登录） | ✅ 执行 |
| API 调用失败 | ✅ 执行（保守策略） |

### 9.3 带日志的服务

```powershell
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --mode clone --log-path C:\logs\monitor.log --interval 2" start= auto
sc.exe start MonitorService

# 停止并删除服务
# sc.exe stop MonitorService
# sc.exe delete MonitorService
```

### 9.4 卸载服务

```powershell
sc.exe stop MonitorService
sc.exe delete MonitorService
```

* * *

## 10\. 日志与调试

### 10.1 启用日志

```powershell
# 控制台模式
monitor-service.exe --log-path "C:\logs\monitor.log"

# 服务模式
$exePath = "C:\monitor-service.exe"
sc.exe create MonitorService binPath= "$exePath --service --log-path C:\logs\monitor.log"
```

### 10.2 日志格式

```
[2026-05-01 17:55:12] ============================================================
[2026-05-01 17:55:12] Monitor Service V0.2.0 Started
[2026-05-01 17:55:12] CLI Options: Some(Clone)
[2026-05-01 17:55:12] ============================================================
[2026-05-01 17:55:12] Running in console mode
[2026-05-01 17:55:12] Enforcement loop started (interval: 2s)
[2026-05-01 17:55:14] Topology set via hotkey: Clone
```

### 10.3 日志事件类型

| 事件  | 说明  |
| --- | --- |
| `Header/Footer` | 服务启动/停止标记 |
| `Enforcement loop started` | 强制执行循环启动 |
| `Mismatch detected` | 检测到显示模式不匹配 |
| `Successfully corrected` | 成功纠正显示模式 |
| `Heartbeat: GC hint` | 内存回收心跳（每12周期） |
| `Topology set via hotkey` | 通过热键切换模式 |

* * *

## 📌 快速参考卡

### 场景速查表

| 场景  | 安装命令 | 关键参数 |
| --- | --- | --- |
| **个人托盘** | 双击运行 | 无   |
| **Winlogon 兜底** | `sc.exe create MonitorService binPath= "C:\monitor-service.exe --service --mode clone --enforce-session0-only"` | `--enforce-session0-only` |
| **个性化托盘** | 服务 + 用户级快捷方式 | `--mode` 个性化 |
| **统一管控** | 服务 + 计划任务/GPO | `--mode` 统一 |

### 故障排查

```powershell
# 查看服务状态
sc.exe query MonitorService

# 查看实时日志
Get-Content "C:\logs\monitor.log" -Tail 20 -Wait

# 手动测试托盘模式
monitor-service.exe --show-console --mode clone
```

* * *

*文档版本: V0.2.1 | 更新日期: 2026-05-03*

* * *

**致谢**

本项目 Fork 自 [github.com/Seryta/windows-monitor-manager](https://github.com/Seryta/windows-monitor-manager)，感谢原作者的开源贡献。