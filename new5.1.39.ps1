# ==========================================
# Kiosk Display Manager (V7.9-Smart-Complete)
# ==========================================
# Complete logic: Check monitor count, then decide
=========================
# 1. Process Priority
# ==========================================
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public class ShutdownHelper {
    [DllImport("kernel32.dll")]
    public static extern IntPtr GetConsoleWindow();

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetProcessShutdownParameters(uint dwLevel, uint dwFlags);
}

public class HotkeyHelper {
    [DllImport("user32.dll")]
    public static extern short GetAsyncKeyState(int vKey);
    
    public const int VK_MENU = 0x12;
    public const int VK_SHIFT = 0x10;
    public const int VK_Q = 0x51;
    public const int VK_W = 0x57;
    public const int VK_E = 0x45;
    public const int VK_R = 0x52;
    public const int VK_T = 0x54;
}
"@

[ShutdownHelper]::SetProcessShutdownParameters(0x3FF, 0)

try {
    $process = Get-Process -Id $PID
    $process.PriorityClass = [System.Diagnostics.ProcessPriorityClass]::High
} catch {}

$SW_HIDE = 0
$consolePtr = [ShutdownHelper]::GetConsoleWindow()
if ($consolePtr -ne [IntPtr]::Zero) {
    [ShutdownHelper]::ShowWindow($consolePtr, $SW_HIDE)
}

# ==========================================
# 2. Logging Helper
# ==========================================
function Write-Log {
    param([string]$Message, [switch]$EventDriven)
    
    if (-not $LogPath) { return }
    
    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss.fff"
    $cleanMsg = if ($EventDriven) { $Message -replace "[`r`n]+", " " } else { $Message }
    
    try {
        "[$timestamp] $cleanMsg" | Out-File -FilePath $LogPath -Append -Encoding utf8 -Force
    } catch {}
}

# ==========================================
# 3. Display Config API
# ==========================================
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public class DisplayConfigAPI
{
    [Flags]
    public enum QueryDisplayFlags : uint
    {
        QDC_ALL_PATHS = 0x00000001,
        QDC_ONLY_ACTIVE_PATHS = 0x00000002,
        QDC_DATABASE_CURRENT = 0x00000004
    }

    public enum DisplayConfigTopologyId : uint
    {
        DISPLAYCONFIG_TOPOLOGY_INTERNAL = 0x00000001,
        DISPLAYCONFIG_TOPOLOGY_CLONE = 0x00000002,
        DISPLAYCONFIG_TOPOLOGY_EXTEND = 0x00000004,
        DISPLAYCONFIG_TOPOLOGY_EXTERNAL = 0x00000008
    }

    [Flags]
    public enum SetDisplayFlags : uint
    {
        SDC_TOPOLOGY_INTERNAL = 0x00000001,
        SDC_TOPOLOGY_CLONE = 0x00000002,
        SDC_TOPOLOGY_EXTEND = 0x00000004,
        SDC_TOPOLOGY_EXTERNAL = 0x00000008,
        SDC_APPLY = 0x00000080
    }

    public const int ERROR_SUCCESS = 0;

    [DllImport("user32.dll")]
    public static extern int GetDisplayConfigBufferSizes(uint flags, out uint numPathArrayElements, out uint numModeInfoArrayElements);

    [DllImport("user32.dll")]
    public static extern int QueryDisplayConfig(uint flags, ref uint numPathArrayElements, IntPtr pathArray, ref uint numModeInfoArrayElements, IntPtr modeInfoArray, out DisplayConfigTopologyId currentTopologyId);

    [DllImport("user32.dll")]
    public static extern int SetDisplayConfig(uint numPathArrayElements, IntPtr pathArray, uint numModeInfoArrayElements, IntPtr modeInfoArray, uint flags);
}
"@

# ==========================================
# 4. Display Logic Functions
# ==========================================

$Global:InitialMode = $Mode
$Script:CurrentTargetMode = $Mode
$Script:HeartbeatCounter = 0
$Script:EnforceEnabled = -not $DisableEnforce
$Script:MenuItems = @{}

$Global:InitialModeFlag = switch ($Global:InitialMode) {
    "Clone"    { [uint32]0x00000082 }
    "Extend"   { [uint32]0x00000084 }
    "Internal" { [uint32]0x00000081 }
    "External" { [uint32]0x00000088 }
}

$Script:HotkeyState = @{
    "Q" = $false; "W" = $false; "E" = $false; "R" = $false; "T" = $false
}

function Get-DisplayTopology {
    [uint32]$flags = [DisplayConfigAPI+QueryDisplayFlags]::QDC_DATABASE_CURRENT
    [uint32]$numPath = 0
    [uint32]$numMode = 0
    $ptrPath = [IntPtr]::Zero
    $ptrMode = [IntPtr]::Zero

    $r1 = [DisplayConfigAPI]::GetDisplayConfigBufferSizes($flags, [ref]$numPath, [ref]$numMode)
    if ($r1 -ne [DisplayConfigAPI]::ERROR_SUCCESS) { return "Error" }

    try {
        $sizePath = 72; $sizeMode = 64
        if ($numPath -gt 0) { $ptrPath = [System.Runtime.InteropServices.Marshal]::AllocHGlobal($sizePath * $numPath) }
        if ($numMode -gt 0) { $ptrMode = [System.Runtime.InteropServices.Marshal]::AllocHGlobal($sizeMode * $numMode) }

        $topology = [DisplayConfigAPI+DisplayConfigTopologyId]::DISPLAYCONFIG_TOPOLOGY_INTERNAL
        $r2 = [DisplayConfigAPI]::QueryDisplayConfig($flags, [ref]$numPath, $ptrPath, [ref]$numMode, $ptrMode, [ref]$topology)
    
        if ($r2 -ne [DisplayConfigAPI]::ERROR_SUCCESS) { return "Error" }

        switch ($topology) {
            ([DisplayConfigAPI+DisplayConfigTopologyId]::DISPLAYCONFIG_TOPOLOGY_INTERNAL) { return "Internal" }
            ([DisplayConfigAPI+DisplayConfigTopologyId]::DISPLAYCONFIG_TOPOLOGY_EXTERNAL) { return "External" }
            ([DisplayConfigAPI+DisplayConfigTopologyId]::DISPLAYCONFIG_TOPOLOGY_CLONE)    { return "Clone" }
            ([DisplayConfigAPI+DisplayConfigTopologyId]::DISPLAYCONFIG_TOPOLOGY_EXTEND)   { return "Extend" }
            default { return "Unknown" }
        }
    }
    finally {
        if ($ptrPath -ne [IntPtr]::Zero) { [System.Runtime.InteropServices.Marshal]::FreeHGlobal($ptrPath) }
        if ($ptrMode -ne [IntPtr]::Zero) { [System.Runtime.InteropServices.Marshal]::FreeHGlobal($ptrMode) }
    }
}

function Set-DisplayTopology {
    param([string]$TargetMode)

    [uint32]$flagVal = 0
    switch ($TargetMode) {
        "Internal" { $flagVal = [uint32][DisplayConfigAPI+SetDisplayFlags]::SDC_TOPOLOGY_INTERNAL }
        "External" { $flagVal = [uint32][DisplayConfigAPI+SetDisplayFlags]::SDC_TOPOLOGY_EXTERNAL }
        "Clone"    { $flagVal = [uint32][DisplayConfigAPI+SetDisplayFlags]::SDC_TOPOLOGY_CLONE }
        "Extend"   { $flagVal = [uint32][DisplayConfigAPI+SetDisplayFlags]::SDC_TOPOLOGY_EXTEND }
    }

    $flagVal = $flagVal -bor [uint32][DisplayConfigAPI+SetDisplayFlags]::SDC_APPLY
    $r = [DisplayConfigAPI]::SetDisplayConfig(0, [IntPtr]::Zero, 0, [IntPtr]::Zero, $flagVal)

    if ($r -eq [DisplayConfigAPI]::ERROR_SUCCESS) {
        Write-Log "Success: Display mode set to $TargetMode"
    } else {
        Write-Log "Error: SetDisplayConfig failed ($r) for $TargetMode"
    }
}

function Get-PhysicalMonitorCount {
    [uint32]$flags = [DisplayConfigAPI+QueryDisplayFlags]::QDC_ALL_PATHS
    [uint32]$numPath = 0
    [uint32]$numMode = 0
    
    $r = [DisplayConfigAPI]::GetDisplayConfigBufferSizes($flags, [ref]$numPath, [ref]$numMode)
    
    if ($r -ne [DisplayConfigAPI]::ERROR_SUCCESS) { return 1 }
    return $numPath
}

function Enforce-DisplayMode {
    $pathCount = Get-PhysicalMonitorCount
    
    if ($pathCount -le 1) { return }

    $currentTopology = Get-DisplayTopology
    
    if ($currentTopology -ne $Script:CurrentTargetMode) {
        Write-Log "Mismatch: Current=$currentTopology != Target=$Script:CurrentTargetMode. Enforcing..."
        Set-DisplayTopology -TargetMode $Script:CurrentTargetMode
    }
}

function Toggle-EnforceMonitoring {
    $Script:EnforceEnabled = -not $Script:EnforceEnabled
    
    if ($Script:EnforceEnabled) {
        Write-Log "Runtime monitoring ENABLED by user"
        if ($enforceTimer) { $enforceTimer.Start() }
        $menuEnforce.Checked = $true
    } else {
        Write-Log "Runtime monitoring DISABLED by user"
        if ($enforceTimer) { $enforceTimer.Stop() }
        $menuEnforce.Checked = $false
    }
}

function Check-Hotkeys {
    $altPressed = ([HotkeyHelper]::GetAsyncKeyState([HotkeyHelper]::VK_MENU) -band 0x8000) -ne 0
    $shiftPressed = ([HotkeyHelper]::GetAsyncKeyState([HotkeyHelper]::VK_SHIFT) -band 0x8000) -ne 0
    
    if (-not ($altPressed -and $shiftPressed)) {
        $Script:HotkeyState["Q"] = $false; $Script:HotkeyState["W"] = $false
        $Script:HotkeyState["E"] = $false; $Script:HotkeyState["R"] = $false
        $Script:HotkeyState["T"] = $false
        return
    }
    
    $keyQ = ([HotkeyHelper]::GetAsyncKeyState([HotkeyHelper]::VK_Q) -band 0x8000) -ne 0
    if ($keyQ -and -not $Script:HotkeyState["Q"]) {
        Write-Log "Hotkey: Alt+Shift+Q -> Internal"
        $Script:CurrentTargetMode = "Internal"
        $notifyIcon.Text = "Display Manager: Internal (Initial: $Global:InitialMode)"
        Enforce-DisplayMode
    }
    $Script:HotkeyState["Q"] = $keyQ
    
    $keyW = ([HotkeyHelper]::GetAsyncKeyState([HotkeyHelper]::VK_W) -band 0x8000) -ne 0
    if ($keyW -and -not $Script:HotkeyState["W"]) {
        Write-Log "Hotkey: Alt+Shift+W -> Clone"
        $Script:CurrentTargetMode = "Clone"
        $notifyIcon.Text = "Display Manager: Clone (Initial: $Global:InitialMode)"
        Enforce-DisplayMode
    }
    $Script:HotkeyState["W"] = $keyW
    
    $keyE = ([HotkeyHelper]::GetAsyncKeyState([HotkeyHelper]::VK_E) -band 0x8000) -ne 0
    if ($keyE -and -not $Script:HotkeyState["E"]) {
        Write-Log "Hotkey: Alt+Shift+E -> Extend"
        $Script:CurrentTargetMode = "Extend"
        $notifyIcon.Text = "Display Manager: Extend (Initial: $Global:InitialMode)"
        Enforce-DisplayMode
    }
    $Script:HotkeyState["E"] = $keyE
    
    $keyR = ([HotkeyHelper]::GetAsyncKeyState([HotkeyHelper]::VK_R) -band 0x8000) -ne 0
    if ($keyR -and -not $Script:HotkeyState["R"]) {
        Write-Log "Hotkey: Alt+Shift+R -> External"
        $Script:CurrentTargetMode = "External"
        $notifyIcon.Text = "Display Manager: External (Initial: $Global:InitialMode)"
        Enforce-DisplayMode
    }
    $Script:HotkeyState["R"] = $keyR
    
    $keyT = ([HotkeyHelper]::GetAsyncKeyState([HotkeyHelper]::VK_T) -band 0x8000) -ne 0
    if ($keyT -and -not $Script:HotkeyState["T"]) {
        Write-Log "Hotkey: Alt+Shift+T -> Toggle Runtime Monitoring"
        Toggle-EnforceMonitoring
    }
    $Script:HotkeyState["T"] = $keyT
}

# ==========================================
# 7. COMPLETE Smart Form - Full Enforce Logic
# ==========================================

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public class IconHelper {
    [DllImport("shell32.dll", CharSet = CharSet.Auto)]
    public static extern IntPtr ExtractIcon(IntPtr hInst, string lpszExeFileName, int nIconIndex);
}
"@

$formCode = @"
using System;
using System.Windows.Forms;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

public class CompleteSmartForm : Form {
    private const int WM_QUERYENDSESSION = 0x0011;
    private const uint ENDSESSION_LOGOFF = 0x80000000;
    
    private const uint EWX_LOGOFF = 0;
    private const uint EWX_SHUTDOWN = 0x00000001;
    private const uint EWX_POWEROFF = 0x00000008;
    
    [DllImport("user32.dll", SetLastError = true)]
    static extern bool ExitWindowsEx(uint uFlags, uint dwReason);
    
    [DllImport("user32.dll")]
    public static extern int SetDisplayConfig(uint numPathArrayElements, IntPtr pathArray, uint numModeInfoArrayElements, IntPtr modeInfoArray, uint flags);
    
    [DllImport("user32.dll")]
    public static extern int GetDisplayConfigBufferSizes(uint flags, out uint numPathArrayElements, out uint numModeInfoArrayElements);
    
    [DllImport("user32.dll")]
    public static extern int QueryDisplayConfig(uint flags, ref uint numPathArrayElements, IntPtr pathArray, ref uint numModeInfoArrayElements, IntPtr modeInfoArray, out uint currentTopologyId);
    
    public uint InitialModeFlag { get; set; }
    public string InitialMode { get; set; }
    public string LogPath { get; set; }
    private bool isScriptInitiated = false;
    private bool taskRunning = false;
    
    protected override void WndProc(ref Message m) {
        if (m.Msg == WM_QUERYENDSESSION) {
            if (isScriptInitiated) {
                LogMessage(">>> [COMPLETE] Script-initiated - ALLOWING");
                m.Result = (IntPtr)1;
                return;
            }
            
            if (taskRunning) {
                LogMessage(">>> [COMPLETE] Task running - ALLOWING to prevent loop");
                m.Result = (IntPtr)1;
                return;
            }
            
            int lParam = (int)m.LParam;
            bool isLogoff = (lParam & ENDSESSION_LOGOFF) != 0;
            uint exitType = isLogoff ? EWX_LOGOFF : (EWX_SHUTDOWN | EWX_POWEROFF);
            string exitTypeName = isLogoff ? "LOGOFF" : "SHUTDOWN";
            
            LogMessage(string.Format(">>> [COMPLETE] {0} detected - Starting COMPLETE logic...", exitTypeName));
            
            // STEP 1: Check monitor count
            LogMessage(">>> [COMPLETE] STEP 1: Checking monitor count...");
            int monitorCount = GetMonitorCount();
            LogMessage(string.Format(">>> [COMPLETE] Monitor count: {0}", monitorCount));
            
            if (monitorCount <= 1) {
                LogMessage(">>> [COMPLETE] Single monitor - No switching needed - ALLOWING immediately");
                m.Result = (IntPtr)1;
                return;
            }
            
            // STEP 2: Detect current mode
            LogMessage(">>> [COMPLETE] STEP 2: Detecting current mode...");
            string currentMode = DetectCurrentMode();
            
            if (currentMode == null) {
                // CASE 3: In Secure Desktop
                LogMessage(">>> [COMPLETE] CASE 3: In Secure Desktop - CANCELLING");
                m.Result = (IntPtr)0;
                taskRunning = true;
                
                uint capturedExitType = exitType;
                
                Task.Run(() => {
                    try {
                        LogMessage(">>> [TASK] Waiting for user desktop...");
                        
                        int attempt = 0;
                        string detectedMode = null;
                        int detectedCount = 0;
                        
                        while (attempt < 50) {
                            attempt++;
                            Thread.Sleep(100);
                            
                            detectedCount = GetMonitorCount();
                            detectedMode = DetectCurrentMode();
                            
                            if (detectedMode != null) {
                                LogMessage(string.Format(">>> [TASK] Back to desktop! Monitors={0}, Mode={1}", detectedCount, detectedMode));
                                break;
                            }
                        }
                        
                        if (detectedMode == null) {
                            LogMessage(">>> [TASK] TIMEOUT");
                            taskRunning = false;
                            return;
                        }
                        
                        // Check monitor count again
                        if (detectedCount <= 1) {
                            LogMessage(string.Format(">>> [TASK] Single monitor ({0}) - No need to switch", detectedCount));
                        } else if (detectedMode == InitialMode) {
                            LogMessage(string.Format(">>> [TASK] Mode correct ({0})", InitialMode));
                        } else {
                            LogMessage(string.Format(">>> [TASK] Fixing: {0} -> {1}", detectedMode, InitialMode));
                            int fixResult = SetDisplayConfig(0, IntPtr.Zero, 0, IntPtr.Zero, InitialModeFlag);
                            LogMessage(string.Format(">>> [TASK] Fix result: {0}", fixResult));
                            Thread.Sleep(200);
                        }
                        
                        LogMessage(string.Format(">>> [TASK] Re-initiating {0}...", capturedExitType == EWX_LOGOFF ? "LOGOFF" : "SHUTDOWN"));
                        Thread.Sleep(100);
                        isScriptInitiated = true;
                        bool result = ExitWindowsEx(capturedExitType, 0);
                        LogMessage(string.Format(">>> [TASK] Result: {0}", result));
                        
                    } catch (Exception ex) {
                        LogMessage(string.Format(">>> [TASK] EXCEPTION: {0}", ex.Message));
                        taskRunning = false;
                    }
                });
                
                return;
            }
            
            // We're in user desktop
            LogMessage(string.Format(">>> [COMPLETE] Current mode: {0}, Initial: {1}", currentMode, InitialMode));
            
            if (currentMode == InitialMode) {
                // CASE 1: Already correct
                LogMessage(">>> [COMPLETE] CASE 1: Mode CORRECT - ALLOWING (0 flash, 0 delay)");
                m.Result = (IntPtr)1;
                return;
            } else {
                // CASE 2: Need to fix
                LogMessage(string.Format(">>> [COMPLETE] CASE 2: Fixing {0} -> {1}...", currentMode, InitialMode));
                
                int fixResult = SetDisplayConfig(0, IntPtr.Zero, 0, IntPtr.Zero, InitialModeFlag);
                LogMessage(string.Format(">>> [COMPLETE] CASE 2: Fix result: {0}", fixResult));
                
                if (fixResult == 0) {
                    LogMessage(">>> [COMPLETE] CASE 2: Fixed - ALLOWING (~200ms)");
                    Thread.Sleep(50);
                } else {
                    LogMessage(">>> [COMPLETE] CASE 2: Fix failed - ALLOWING anyway");
                }
                
                m.Result = (IntPtr)1;
                return;
            }
        }
        
        base.WndProc(ref m);
    }
    
    private int GetMonitorCount() {
        try {
            const uint QDC_ALL_PATHS = 1;
            uint numPath = 0;
            uint numMode = 0;
            
            int r = GetDisplayConfigBufferSizes(QDC_ALL_PATHS, out numPath, out numMode);
            if (r != 0) return 1;
            
            return (int)numPath;
        } catch {
            return 1;
        }
    }
    
    private string DetectCurrentMode() {
        try {
            const uint QDC_DATABASE_CURRENT = 4;
            uint numPath = 0;
            uint numMode = 0;
            
            int r1 = GetDisplayConfigBufferSizes(QDC_DATABASE_CURRENT, out numPath, out numMode);
            if (r1 != 0) return null;
            if (numPath == 0) return null;
            
            int sizePath = 72;
            int sizeMode = 64;
            IntPtr ptrPath = Marshal.AllocHGlobal((int)(sizePath * numPath));
            IntPtr ptrMode = Marshal.AllocHGlobal((int)(sizeMode * numMode));
            
            try {
                uint topology = 0;
                int r2 = QueryDisplayConfig(QDC_DATABASE_CURRENT, ref numPath, ptrPath, ref numMode, ptrMode, out topology);
                if (r2 != 0) return null;
                
                switch (topology) {
                    case 1: return "Internal";
                    case 2: return "Clone";
                    case 4: return "Extend";
                    case 8: return "External";
                    default: return null;
                }
            } finally {
                Marshal.FreeHGlobal(ptrPath);
                Marshal.FreeHGlobal(ptrMode);
            }
        } catch {
            return null;
        }
    }
    
    private void LogMessage(string msg) {
        if (!string.IsNullOrEmpty(LogPath)) {
            try {
                string timestamp = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff");
                string logMsg = string.Format("[{0}] {1}\r\n", timestamp, msg);
                System.IO.File.AppendAllText(LogPath, logMsg);
            } catch { }
        }
    }
}
"@

Add-Type -TypeDefinition $formCode -ReferencedAssemblies @("System.Windows.Forms") -ErrorAction SilentlyContinue

$form = New-Object CompleteSmartForm
$form.InitialModeFlag = $Global:InitialModeFlag
$form.InitialMode = $Global:InitialMode
$form.LogPath = $LogPath
$form.WindowState = "Minimized"
$form.ShowInTaskbar = $false
$form.Opacity = 0
$form.FormBorderStyle = "None"

$notifyIcon = New-Object System.Windows.Forms.NotifyIcon

try {
    $iconPath = "$env:SystemRoot\system32\compstui.dll"
    $iconIndex = 16 
    $hIcon = [IconHelper]::ExtractIcon([IntPtr]::Zero, $iconPath, $iconIndex)
    
    if ($hIcon -ne [IntPtr]::Zero) {
        $notifyIcon.Icon = [System.Drawing.Icon]::FromHandle($hIcon)
    } else {
        $icon = [System.Drawing.Icon]::ExtractAssociatedIcon((Get-Process -Id $pid).Path)
        $notifyIcon.Icon = $icon
    }
} catch {
    $notifyIcon.Icon = [System.Drawing.SystemIcons]::Application
}

$notifyIcon.Text = "Display Manager: $Script:CurrentTargetMode (Initial: $Global:InitialMode)"
$notifyIcon.Visible = $true

# ==========================================
# 8. Context Menu
# ==========================================

$contextMenu = New-Object System.Windows.Forms.ContextMenuStrip

function Add-ModeMenuItem($name) {
    $item = $contextMenu.Items.Add($name)
    $item.Tag = $name
    $Script:MenuItems[$name] = $item
    $item.Add_Click({
        param($sender, $e)
        $selectedMode = $sender.Tag
        Write-Log "User menu: $selectedMode"
        $Script:CurrentTargetMode = $selectedMode
        $notifyIcon.Text = "Display Manager: $selectedMode (Initial: $Global:InitialMode)"
        Enforce-DisplayMode
    })
}

Add-ModeMenuItem "Extend"
Add-ModeMenuItem "Clone"
Add-ModeMenuItem "Internal"
Add-ModeMenuItem "External"
$contextMenu.Items.Add("-")

$menuEnforce = $contextMenu.Items.Add("Runtime Monitoring (Alt+Shift+T)")
$menuEnforce.CheckOnClick = $true
$menuEnforce.Checked = $Script:EnforceEnabled
$menuEnforce.Add_Click({ Toggle-EnforceMonitoring })

$contextMenu.Items.Add("-")
$menuExit = $contextMenu.Items.Add("Exit")
$menuExit.Add_Click({ 
    Write-Log "User Exit"
    $form.Close() 
})

$contextMenu.Add_Opening({
    try {
        $currentMode = Get-DisplayTopology
        foreach ($modeName in $Script:MenuItems.Keys) {
            $Script:MenuItems[$modeName].Checked = ($modeName -eq $currentMode)
        }
    } catch {}
})

$notifyIcon.ContextMenuStrip = $contextMenu

# ==========================================
# 9. Dual Timer System
# ==========================================

$hotkeyTimer = New-Object System.Windows.Forms.Timer
$hotkeyTimer.Interval = 100
$hotkeyTimer.Add_Tick({ try { Check-Hotkeys } catch {} })
$hotkeyTimer.Start()

$enforceTimer = $null
if ($Script:EnforceEnabled) {
    $enforceTimer = New-Object System.Windows.Forms.Timer
    $enforceTimer.Interval = $IntervalSeconds * 1000
    $enforceTimer.Add_Tick({
        try {
            Enforce-DisplayMode
            $Script:HeartbeatCounter++
            if ($Script:HeartbeatCounter -ge 12) {
                $Script:HeartbeatCounter = 0
                [System.GC]::Collect()
            }
        } catch {}
    })
    $enforceTimer.Start()
}

# ==========================================
# 10. Startup Execution
# ==========================================
try {
    Write-Log "=========================================="
    Write-Log "Display Manager V7.9-Smart-Complete Started"
    Write-Log "Windows Version: $([System.Environment]::OSVersion.Version)"
    Write-Log "Initial Mode: $Global:InitialMode"
    Write-Log "=========================================="
    Write-Log "COMPLETE LOGIC (like Enforce):"
    Write-Log "  1. Check monitor count first"
    Write-Log "  2. Single monitor → Allow immediately"
    Write-Log "  3. Multi-monitor:"
    Write-Log "     CASE 1: Mode correct → Allow (0 flash, 0 delay)"
    Write-Log "     CASE 2: Mode wrong (user desktop) → Fix & Allow (~200ms)"
    Write-Log "     CASE 3: Secure Desktop → Cancel, wait, fix, re-initiate"
    Write-Log "=========================================="
    
    Enforce-DisplayMode
    Write-Log "Initial display mode enforced"
    Write-Log "Entering message loop..."
    
    [System.Windows.Forms.Application]::Run($form)
    
    Write-Log "Message loop exited"
} catch {
    Write-Log "FATAL: $_"
} finally {
    Write-Log "Cleanup..."
    
    if ($notifyIcon) { $notifyIcon.Visible = $false; $notifyIcon.Dispose() }
    if ($hotkeyTimer) { $hotkeyTimer.Stop(); $hotkeyTimer.Dispose() }
    if ($enforceTimer) { $enforceTimer.Stop(); $enforceTimer.Dispose() }
    if ($form) { $form.Dispose() }
    
    Write-Log "Terminated"
}
<# ```

---

## 完整逻辑流程
```
关机请求
    ↓
STEP 1: 检查屏幕数量
    ↓
    单屏幕 → 直接放行 ✅ (0ms)
    ↓
STEP 2: 检测当前mode
    ↓
    ├─ 检测失败（安全桌面）→ CASE 3
    │
    ├─ mode正确 → CASE 1: 直接放行 ✅ (0ms)
    │
    └─ mode错误 → CASE 2: 改后放行 ✅ (~200ms)
```

---

## 预期日志（完整版）

### 单屏幕（最快）
```
[时间] >>> [COMPLETE] SHUTDOWN detected - Starting COMPLETE logic...
[时间] >>> [COMPLETE] STEP 1: Checking monitor count...
[时间] >>> [COMPLETE] Monitor count: 1
[时间] >>> [COMPLETE] Single monitor - No switching needed - ALLOWING immediately
```
**结果：立即关机，0检测mode，0切换！** ⭐⭐⭐⭐⭐

---

### 多屏幕 + Mode正确
```
[时间] >>> [COMPLETE] LOGOFF detected - Starting COMPLETE logic...
[时间] >>> [COMPLETE] STEP 1: Checking monitor count...
[时间] >>> [COMPLETE] Monitor count: 2
[时间] >>> [COMPLETE] STEP 2: Detecting current mode...
[时间] >>> [COMPLETE] Current mode: Clone, Initial: Clone
[时间] >>> [COMPLETE] CASE 1: Mode CORRECT - ALLOWING (0 flash, 0 delay)
```
**结果：立即注销，0切换！** ⭐⭐⭐⭐⭐

---

### 多屏幕 + Mode错误
```
[时间] >>> [COMPLETE] SHUTDOWN detected - Starting COMPLETE logic...
[时间] >>> [COMPLETE] STEP 1: Checking monitor count...
[时间] >>> [COMPLETE] Monitor count: 2
[时间] >>> [COMPLETE] STEP 2: Detecting current mode...
[时间] >>> [COMPLETE] Current mode: Extend, Initial: Clone
[时间] >>> [COMPLETE] CASE 2: Fixing Extend -> Clone...
[时间] >>> [COMPLETE] CASE 2: Fix result: 0
[时间] >>> [COMPLETE] CASE 2: Fixed - ALLOWING (~200ms)
```
**结果：改完关机，1闪烁，~200ms！** ⭐⭐⭐⭐

---

### 安全桌面
```
[时间] >>> [COMPLETE] LOGOFF detected - Starting COMPLETE logic...
[时间] >>> [COMPLETE] STEP 1: Checking monitor count...
[时间] >>> [COMPLETE] Monitor count: 2
[时间] >>> [COMPLETE] STEP 2: Detecting current mode...
[时间] >>> [COMPLETE] CASE 3: In Secure Desktop - CANCELLING
[时间] >>> [TASK] Waiting for user desktop...
[时间] >>> [TASK] Back to desktop! Monitors=2, Mode=Extend
[时间] >>> [TASK] Fixing: Extend -> Clone
[时间] >>> [TASK] Fix result: 0
[时间] >>> [TASK] Re-initiating LOGOFF... #>