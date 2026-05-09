#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod hotkey;
mod logger;
mod process_win;
#[cfg(windows)]
mod session_mutex;
#[cfg(windows)]
mod shutdown;
#[cfg(windows)]
mod system_icon;
#[cfg(windows)]
mod tray;

use axum::{
    extract::{Json, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::{Parser, Subcommand, ValueEnum};
use monitor_lib::{display, MonitorInfo, TopologyMode};
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::trace::TraceLayer;

// 状态共享
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(windows)]
use hotkey::{handle_hotkey_event, HotkeyManager};

use logger::{log_footer, log_header, Logger};

#[derive(Parser, Clone)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Run as a Windows Service
    #[arg(long)]
    service: bool,

    /// IP address and port to listen on (e.g. 127.0.0.1:3000). Not started if not set.
    #[arg(long)]
    addr: Option<String>,

    /// Initial display mode: Clone, Extend, Internal, External. Uses system current if not set.
    #[arg(short, long, value_enum)]
    mode: Option<ModeArg>,

    /// Monitoring interval in seconds
    #[arg(short, long, default_value = "2")]
    interval: u64,

    /// Log file path (disabled if not set)
    #[arg(short = 'l', long)]
    log_path: Option<PathBuf>,

    /// Disable runtime monitoring (enforcement is enabled by default)
    #[arg(long)]
    no_enforce: bool,

    /// Show console window (hidden by default)
    #[arg(long)]
    show_console: bool,

    /// Hide the Exit button in tray menu (shown by default)
    #[arg(long)]
    hide_exit: bool,

    /// Internal flag: process was already re-launched as detached (prevents infinite loop)
    #[arg(long, hide = true)]
    relaunched: bool,

    /// Service mode: only enforce when no user is on the desktop (Winlogon/pre-login state).
    /// Detects user desktop via WTSQuerySessionInformationW(WTSUserName) on the active console session.
    /// Skips enforcement when a real user (non-empty, non-SYSTEM) is associated with the session.
    #[arg(long)]
    enforce_session0_only: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ModeArg {
    Clone,
    Extend,
    Internal,
    External,
}

impl From<ModeArg> for TopologyMode {
    fn from(mode: ModeArg) -> Self {
        match mode {
            ModeArg::Clone => TopologyMode::Clone,
            ModeArg::Extend => TopologyMode::Extend,
            ModeArg::Internal => TopologyMode::Internal,
            ModeArg::External => TopologyMode::External,
        }
    }
}

#[derive(Subcommand, Clone)]
enum Commands {
    /// Run as Agent (Display operations)
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },
}

#[derive(Subcommand, Clone)]
enum AgentCommands {
    /// List all monitors
    List,
    /// Get current topology mode (prints: clone/extend/internal/external/single:ID)
    Get,
    /// Set topology mode
    Set {
        /// Mode: internal, external, extend, clone, or single:N
        mode: String,
    },
    /// Query active input desktop name (service internal; prints "Default" or "Winlogon")
    #[command(name = "query-desktop")]
    QueryDesktop,
}

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[cfg(windows)]
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

/// 共享应用状态
#[derive(Clone)]
pub struct AppState {
    /// 当前目标显示模式
    pub current_mode: Arc<std::sync::Mutex<TopologyMode>>,
    /// 运行监控是否启用
    pub monitoring_enabled: Arc<AtomicBool>,
    /// 初始模式
    pub initial_mode: TopologyMode,
    /// 监控间隔（秒）
    pub interval_seconds: u64,
    /// 服务模式需要在当前 Session 存在托盘实例时退让；托盘实例自身不能退让给自己。
    pub yield_to_tray_instance: bool,
    /// 托盘 UI 刷新标志；Web/API、热键、enforcement 都通过它通知托盘同步状态。
    pub force_refresh: Arc<AtomicBool>,
    /// 服务模式可选：仅在无用户 Session（Winlogon界面）时执行 enforcement。
    /// 当 explorer.exe 在活跃控制台 Session 中运行时（用户已登入桌面），跳过 enforcement。
    pub enforce_session0_only: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 1. Agent 模式命令分发
    if let Some(Commands::Agent { command }) = cli.command {
        return run_agent(command).await;
    }

    // 1.5. 自动脱离父控制台（修复 bat 黑窗）
    // 仅对 tray/console 模式生效（非 --service，非 agent，非 --show-console，未重新启动过）
    // 原进程脱离后立即退出 → bat 的 CMD 窗口随之关闭
    // 真正的托盘由重新启动的子进程运行
    #[cfg(windows)]
    if !cli.relaunched && !cli.service && !cli.show_console && cli.command.is_none() {
        match process_win::relaunch_as_detached() {
            Ok(()) => return Ok(()),
            Err(_) => {} // 重新启动失败，继续正常运行（不阻断功能）
        }
    }

    // 2. 初始化日志系统
    Logger::init(cli.log_path.clone());
    log_header("V0.2.0", &format!("{:?}", cli.mode));

    // 3. 初始化进程设置（控制台默认隐藏，--show-console 时显示）
    #[cfg(windows)]
    process_win::init_process_settings(!cli.show_console);

    // 4. 确定初始模式：用户指定 > 系统当前 > Clone 兜底
    let initial_mode: TopologyMode = if let Some(m) = cli.mode {
        m.into()
    } else {
        display::get_topology().unwrap_or(TopologyMode::Clone)
    };
    let current_mode = Arc::new(std::sync::Mutex::new(initial_mode.clone()));
    let monitoring_enabled = Arc::new(AtomicBool::new(!cli.no_enforce));
    let force_refresh = Arc::new(AtomicBool::new(false));
    let app_state = AppState {
        current_mode: current_mode.clone(),
        monitoring_enabled: monitoring_enabled.clone(),
        initial_mode: initial_mode.clone(),
        interval_seconds: cli.interval,
        yield_to_tray_instance: cli.service,
        force_refresh: force_refresh.clone(),
        enforce_session0_only: cli.enforce_session0_only,
    };

    // 5. 服务模式 vs 控制台模式
    if cli.service {
        #[cfg(windows)]
        {
            log!("Running in service mode");
            run_service_with_state(app_state)?;
        }
        #[cfg(not(windows))]
        println!("Service mode not supported on non-Windows OS");
    } else {
        // 控制台模式 - 启动托盘和热键
        #[cfg(windows)]
        {
            log!("Running in console mode");

            // 创建Session Mutex（声明当前Session的控制权）
            // 服务模式检测到此Mutex存在时会退让
            // 若Mutex已被占用（前一个托盘正在关闭），最多等待5秒后接管；
            // 若5秒后仍无法获取，说明另一个托盘真的在运行，直接退出（单实例保障）
            let _session_mutex = match session_mutex::SessionMutex::create() {
                Ok(mutex) => {
                    log!("Session mutex created: Session {}", mutex.session_id());
                    mutex
                }
                Err(e) => {
                    log!("Error: Tray already running, exiting: {}", e);
                    return Ok(());
                }
            };

            // 启动关机拦截器（必须在托盘和热键之前启动）
            let _shutdown_interceptor =
                shutdown::start_shutdown_interceptor(initial_mode.clone(), current_mode.clone());

            // 创建关闭通道
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

            // 启动托盘（传递初始模式）
            tray::start_tray_with_state(
                initial_mode.clone(),
                shutdown_tx,
                current_mode.clone(),
                monitoring_enabled.clone(),
                force_refresh.clone(),
                cli.hide_exit,
                cli.addr.clone(),
            );

            // 启动热键监听（在独立任务中）
            let current_mode_for_hotkey = current_mode.clone();
            let monitoring_ref = monitoring_enabled.clone();
            let force_refresh_for_hotkey = force_refresh.clone();

            tokio::spawn(async move {
                if let Ok((_hots, mut hotkey_rx)) = HotkeyManager::new() {
                    log!("Global hotkeys registered: Alt+Shift+Q/W/E/R/T");

                    while let Some(event) = hotkey_rx.recv().await {
                        if let Some((new_mode, new_monitoring)) =
                            handle_hotkey_event(event, &current_mode_for_hotkey, &monitoring_ref)
                        {
                            // 如果有模式变更，执行切换
                            if let Some(mode) = new_mode {
                                if let Err(e) = display::set_topology(mode.clone()) {
                                    log!("Failed to set topology via hotkey: {}", e);
                                    // 注意：不回滚 current_mode（目标模式）
                                    // enforcement loop 会持续重试，直到成功
                                } else {
                                    log!("Topology set via hotkey: {:?}", mode);
                                }
                                // 无论成功失败，都通知托盘刷新UI
                                force_refresh_for_hotkey.store(true, Ordering::Relaxed);
                            }

                            // 处理监控状态变更
                            if let Some(enabled) = new_monitoring {
                                log!("Monitoring toggled via hotkey: {}", enabled);
                                // 通知托盘刷新UI（checkbox 和 tooltip 同步）
                                force_refresh_for_hotkey.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    std::mem::drop(_hots); // 保持热键管理器存活
                }
            });

            // 启动强制执行定时器（总是启动，内部根据 monitoring_enabled 状态决定是否执行）
            let enforce_state = AppState {
                current_mode: current_mode.clone(),
                monitoring_enabled: monitoring_enabled.clone(),
                initial_mode: initial_mode.clone(),
                interval_seconds: cli.interval,
                yield_to_tray_instance: false,
                force_refresh: force_refresh.clone(),
                enforce_session0_only: cli.enforce_session0_only,
            };

            let force_refresh_for_enforce = force_refresh.clone();

            tokio::spawn(async move {
                run_enforcement_loop(enforce_state, force_refresh_for_enforce).await;
            });

            // 启动 HTTP 服务器（如果指定了监听地址），否则阻塞等待托盘退出信号
            if let Some(addr_str) = cli.addr {
                run_server(
                    Some(shutdown_rx),
                    addr_str.parse()?,
                    ServerState {
                        app_state: Some(app_state.clone()),
                    },
                )
                .await?;
            } else {
                shutdown_rx.await.ok();
            }
        }

        #[cfg(not(windows))]
        {
            if let Some(addr_str) = cli.addr {
                run_server(
                    None,
                    addr_str.parse()?,
                    ServerState {
                        app_state: Some(app_state.clone()),
                    },
                )
                .await?;
            }
        }
    }

    log_footer();
    Ok(())
}

/// 强制执行循环 - 定期检查和恢复显示模式
/// 设计原则（来自 PS1 Enforce-DisplayMode）：
/// - 只有本项目才是合法更改 display topology 的来源
/// - 其他改了的（如 Win+P）都视为非法更改
/// - 自动检测当前 topology 与用户在本 app 设定的 topology 是否相同
/// - 不同则自动纠正回用户设定的 topology
async fn run_enforcement_loop(state: AppState, force_refresh: Arc<AtomicBool>) {
    use tokio::time::{interval, Duration};

    let mut tick = interval(Duration::from_secs(state.interval_seconds));
    let mut heartbeat_counter = 0u8;
    let mut was_enabled = state.monitoring_enabled.load(Ordering::Relaxed);
    let mut consecutive_mismatches = 0u32;
    let mut was_locked = false;
    let mut was_user_on_desktop = false;

    log!(
        "Enforcement loop started (interval: {}s)",
        state.interval_seconds
    );

    loop {
        tick.tick().await;

        // 检查当前Session是否有托盘在运行 (Session Mutex检测)
        // 托盘模式创建Mutex -> 服务模式检测到此Mutex存在就退让
        // 只有用户活跃在桌面（已登录且未锁屏）时才退让；Winlogon/锁屏/无用户时不退让
        #[cfg(windows)]
        if state.yield_to_tray_instance && session_mutex::is_tray_present_in_current_session() {
            // 旧逻辑: let screen_locked = process_win::is_active_console_session_locked();
            //         if !screen_locked { yield }
            // 新逻辑: is_user_actively_on_desktop() = is_user_on_desktop() && !is_active_console_session_locked()
            if process_win::is_user_actively_on_desktop() {
                // 用户活跃在桌面：退让给托盘实例
                if was_enabled {
                    log!(
                        "Yielding to tray instance: Session {}",
                        session_mutex::get_current_session_id()
                    );
                }
                if was_locked {
                    log!("Screen unlocked: yielding back to tray instance");
                    was_locked = false;
                }
                was_enabled = false;
                continue;
            }
            // 用户未活跃（锁屏/Winlogon/无用户）：不退让，由服务模式接管 enforcement
            if !was_locked {
                log!(
                    "Screen locked: service enforcement active (Session {})",
                    session_mutex::get_current_session_id()
                );
                was_locked = true;
            }
        }

        // 托盘/控制台模式：锁屏时暂停 enforcement，让服务模式接管
        #[cfg(windows)]
        if !state.yield_to_tray_instance {
            let screen_locked = process_win::is_active_console_session_locked();
            if screen_locked {
                if !was_locked {
                    log!("Screen locked: tray enforcement paused, deferring to service");
                    was_locked = true;
                }
                consecutive_mismatches = 0;
                continue;
            } else if was_locked {
                log!("Screen unlocked: tray enforcement resumed");
                was_locked = false;
            }
        }

        // --enforce-session0-only: 通过 WTSUserName 检测 session 是否关联真实用户
        // 有非空非 SYSTEM 用户名 且 屏幕未锁定 → 用户在桌面 → 跳过 enforcement
        // Win+L 锁屏时用户仍登录，但屏幕已锁 → 执行 enforcement
        // 空/SYSTEM/API失败 → Winlogon/预登录/其他 → 执行 enforcement
        #[cfg(windows)]
        if state.enforce_session0_only {
            let user_on_desktop = process_win::is_user_on_desktop();
            if user_on_desktop != was_user_on_desktop {
                was_user_on_desktop = user_on_desktop;
                if user_on_desktop {
                    log!("Session0-only: user logged in to desktop, enforcement suspended");
                } else {
                    log!("Session0-only: user left desktop (Winlogon), enforcement resumed");
                }
            }
            // 旧逻辑: if user_on_desktop { let screen_locked = ...; if !screen_locked { continue; } }
            // 新逻辑: 封装为 is_user_actively_on_desktop()，语义等同于 user_on_desktop && !screen_locked
            if process_win::is_user_actively_on_desktop() {
                consecutive_mismatches = 0;
                continue;
            }
        }

        // 检查监控是否启用
        let enabled = state.monitoring_enabled.load(Ordering::Relaxed);

        // 状态变更日志
        if enabled != was_enabled {
            if enabled {
                log!("Enforcement loop resumed (monitoring enabled)");
            } else {
                log!("Enforcement loop paused (monitoring disabled)");
            }
            was_enabled = enabled;
        }

        // 如果 monitoring 禁用，跳过本次执行（继续循环等待）
        if !enabled {
            consecutive_mismatches = 0;
            continue;
        }

        // STEP 1: 检查显示器数量，单显示器时跳过 enforcement
        // 服务模式(Session 0)：跳过此检查。
        // Session 0 没有 display context，GetDisplayConfigBufferSizes 返回 0 路径，
        // 会误判为单显示器并永远跳过 enforcement。
        // 控制台/托盘模式：正常检查，单显示器时跳过。
        if !state.yield_to_tray_instance {
            match display::get_monitor_count() {
                Ok(count) if count <= 1 => {
                    consecutive_mismatches = 0;
                    continue;
                }
                Ok(_count) => {}
                Err(e) => {
                    log!("Enforcement: Failed to get monitor count: {}", e);
                    continue;
                }
            }
        }

        // 获取当前目标模式（用户在本 app 设定的 topology）
        // 这个值只有用户通过菜单或热键主动切换时才会改变
        // 绝不会因为系统状态变化而自动改变
        let target_mode = {
            let mode = state.current_mode.lock().unwrap();
            mode.clone()
        };

        // 获取系统实际当前模式
        // 服务模式(yield_to_tray_instance=true)：必须通过 run_agent_in_active_session 操作
        // 因为 SetDisplayConfig / QueryDisplayConfig 在 Session 0 中只影响 Session 0，
        // 不会影响活跃用户 Session 的显示器配置
        let get_result: Result<TopologyMode, String> = if state.yield_to_tray_instance {
            match process_win::run_agent_in_active_session(&["get"]) {
                Ok(output) => {
                    let trimmed = output.trim();
                    match parse_topology_from_str(trimmed) {
                        Some(mode) => Ok(mode),
                        None => {
                            log!("Enforcement(svc): agent get returned unknown output: {:?}", trimmed);
                            Err(format!("Unknown topology output: {:?}", trimmed))
                        }
                    }
                }
                Err(e) => {
                    log!("Enforcement(svc): agent get failed: {}", e);
                    Err(e.to_string())
                }
            }
        } else {
            display::get_topology()
        };

        match get_result {
            Ok(current) => {
                if current != target_mode {
                    consecutive_mismatches += 1;
                    log!(
                        "Enforcement: Mismatch detected (#{})! Current={:?} != Target={:?}. Correcting...",
                        consecutive_mismatches, current, target_mode
                    );

                    // 纠正回用户设定的 topology
                    // 服务模式同样需要通过 agent 在活跃用户 Session 中执行
                    let set_result: Result<(), String> = if state.yield_to_tray_instance {
                        let mode_str = topology_to_mode_str(&target_mode);
                        match process_win::run_agent_in_active_session(&["set", &mode_str]) {
                            Ok(output) => {
                                let trimmed = output.trim();
                                log!("Enforcement(svc): agent set {:?} output: {:?}", mode_str, trimmed);
                                if trimmed == "Success" {
                                    Ok(())
                                } else {
                                    // Agent ran but SetDisplayConfig failed inside the agent process.
                                    // Return Err so the enforcement loop retries next cycle.
                                    Err(trimmed.to_string())
                                }
                            }
                            Err(e) => {
                                // Failed to even spawn the agent process.
                                log!("Enforcement(svc): agent set {:?} failed: {}", mode_str, e);
                                Err(e.to_string())
                            }
                        }
                    } else {
                        display::set_topology(target_mode.clone())
                    };

                    match set_result {
                        Ok(()) => {
                            log!("Enforcement: Successfully corrected to {:?}", target_mode);
                            // 通知托盘刷新 UI
                            force_refresh.store(true, Ordering::Relaxed);
                        }
                        Err(e) => {
                            log!("Enforcement: Failed to correct topology: {}. Will retry next cycle.", e);
                        }
                    }
                } else {
                    // 匹配：如果之前有过 mismatch，记录恢复
                    if consecutive_mismatches > 0 {
                        log!("Enforcement: Topology matches target {:?} (corrected after {} mismatches)", target_mode, consecutive_mismatches);
                        consecutive_mismatches = 0;
                    }
                }
            }
            Err(e) => {
                log!("Enforcement: Failed to get topology: {}", e);
            }
        }

        // GC 心跳（每12个周期）
        heartbeat_counter = heartbeat_counter.wrapping_add(1);
        if heartbeat_counter >= 12 {
            heartbeat_counter = 0;
            log!("Heartbeat: GC hint");
        }
    }
}

fn topology_to_mode_str(mode: &TopologyMode) -> String {
    match mode {
        TopologyMode::Clone => "clone".to_string(),
        TopologyMode::Extend => "extend".to_string(),
        TopologyMode::Internal => "internal".to_string(),
        TopologyMode::External => "external".to_string(),
        TopologyMode::Single(id) => format!("single:{}", id),
    }
}

fn parse_topology_from_str(s: &str) -> Option<TopologyMode> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "clone" => Some(TopologyMode::Clone),
        "extend" => Some(TopologyMode::Extend),
        "internal" => Some(TopologyMode::Internal),
        "external" => Some(TopologyMode::External),
        s if s.starts_with("single:") => Some(TopologyMode::Single(s[7..].to_string())),
        _ => None,
    }
}

async fn run_agent(cmd: AgentCommands) -> anyhow::Result<()> {
    match cmd {
        AgentCommands::List => {
            let monitors = display::list_monitors().map_err(|e| anyhow::anyhow!(e))?;
            println!("{}", serde_json::to_string_pretty(&monitors)?);
        }
        AgentCommands::Get => {
            let topo = display::get_topology().map_err(|e| anyhow::anyhow!(e))?;
            println!("{}", topology_to_mode_str(&topo));
        }
        AgentCommands::Set { mode } => {
            let topology_mode = if mode.starts_with("single:") {
                let id_str = mode.trim_start_matches("single:");
                // Don't parse as u32 anymore, pass the raw string "low:high:id"
                TopologyMode::Single(id_str.to_string())
            } else {
                match mode.to_lowercase().as_str() {
                    "internal" => TopologyMode::Internal,
                    "external" => TopologyMode::External,
                    "extend" => TopologyMode::Extend,
                    "clone" => TopologyMode::Clone,
                    _ => return Err(anyhow::anyhow!("Invalid mode")),
                }
            };

            display::set_topology(topology_mode).map_err(|e| anyhow::anyhow!(e))?;
            println!("Success");
        }
        AgentCommands::QueryDesktop => {
            // Runs inside the user's session (spawned by the service via CreateProcessAsUserW).
            // OpenInputDesktop() returns the active INPUT desktop for this process's window station
            // (WinSta0 of the active console session), giving the service a session-based way to
            // distinguish Win11 Stage-1 lockscreen (Default active) from Stage-2 password input
            // (Winlogon active) — without inspecting any process names.
            #[cfg(windows)]
            {
                use windows::Win32::Foundation::{BOOL, HANDLE};
                use windows::Win32::System::StationsAndDesktops::{
                    CloseDesktop, GetUserObjectInformationW, OpenInputDesktop,
                    DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, UOI_NAME,
                };
                let desktop_name = unsafe {
                    match OpenInputDesktop(
                        DESKTOP_CONTROL_FLAGS(0),
                        BOOL(0),
                        DESKTOP_ACCESS_FLAGS(0x0001), // DESKTOP_READOBJECTS
                    ) {
                        Ok(h_desk) => {
                            let mut buf = vec![0u16; 256];
                            let _ = GetUserObjectInformationW(
                                HANDLE(h_desk.0),
                                UOI_NAME,
                                Some(buf.as_mut_ptr() as *mut _),
                                (buf.len() * 2) as u32,
                                None,
                            );
                            let _ = CloseDesktop(h_desk);
                            let len =
                                buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                            String::from_utf16_lossy(&buf[..len])
                        }
                        // Can't open input desktop with user token → it's restricted (Winlogon active).
                        Err(_) => "Winlogon".to_string(),
                    }
                };
                print!("{}", desktop_name);
            }
            #[cfg(not(windows))]
            print!("Default");
        }
    }
    Ok(())
}

#[cfg(windows)]
define_windows_service!(ffi_service_main, my_service_main);

#[cfg(windows)]
static SERVICE_STATE: once_cell::sync::OnceCell<AppState> = once_cell::sync::OnceCell::new();

#[cfg(windows)]
fn run_service() -> windows_service::Result<()> {
    service_dispatcher::start("MonitorService", ffi_service_main)
}

#[cfg(windows)]
fn my_service_main(arguments: Vec<std::ffi::OsString>) {
    let result = if let Some(state) = SERVICE_STATE.get().cloned() {
        run_service_impl_with_state(state, arguments)
    } else {
        run_service_impl_no_state()
    };

    if let Err(_e) = result {
        // Log error
    }
}

#[cfg(windows)]
fn run_service_with_state(state: AppState) -> windows_service::Result<()> {
    let _ = SERVICE_STATE.set(state);
    service_dispatcher::start("MonitorService", ffi_service_main)
}

#[cfg(windows)]
fn run_service_impl_with_state(
    state: AppState,
    _arguments: Vec<std::ffi::OsString>,
) -> anyhow::Result<()> {
    use crate::log;

    let cli = Cli::parse();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let mut shutdown_tx = Some(shutdown_tx);

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                log!("Service: Stop requested");
                if let Some(tx) = shutdown_tx.take() {
                    let _ = tx.send(());
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register("MonitorService", event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    let rt = tokio::runtime::Runtime::new()?;

    // 如果启用了监控，启动强制执行循环
    if state.monitoring_enabled.load(Ordering::Relaxed) {
        let enforce_state = AppState {
            current_mode: state.current_mode.clone(),
            monitoring_enabled: state.monitoring_enabled.clone(),
            initial_mode: state.initial_mode.clone(),
            interval_seconds: state.interval_seconds,
            yield_to_tray_instance: state.yield_to_tray_instance,
            force_refresh: state.force_refresh.clone(),
            enforce_session0_only: state.enforce_session0_only,
        };

        // 服务模式没有托盘UI，使用dummy标志
        let _force_refresh = Arc::new(AtomicBool::new(false));

        rt.spawn(async move {
            run_enforcement_loop(enforce_state, _force_refresh).await;
        });
    }

    rt.block_on(async {
        if let Some(addr_str) = cli.addr {
            let addr: SocketAddr = match addr_str.parse() {
                Ok(a) => a,
                Err(e) => {
                    log!("Invalid address '{}': {}", addr_str, e);
                    return;
                }
            };
            if let Err(e) = run_server(
                Some(shutdown_rx),
                addr,
                ServerState {
                    app_state: Some(state.clone()),
                },
            )
            .await
            {
                log!("Server error: {}", e);
            }
        } else {
            shutdown_rx.await.ok();
        }
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

#[cfg(windows)]
fn run_service_impl_no_state() -> anyhow::Result<()> {
    use crate::log;

    let cli = Cli::parse();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let mut shutdown_tx = Some(shutdown_tx);

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                log!("Service: Stop requested (no state mode)");
                if let Some(tx) = shutdown_tx.take() {
                    let _ = tx.send(());
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register("MonitorService", event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async {
        if let Some(addr_str) = cli.addr {
            let addr: SocketAddr = match addr_str.parse() {
                Ok(a) => a,
                Err(e) => {
                    log!("Invalid address '{}': {}", addr_str, e);
                    return;
                }
            };
            if let Err(e) =
                run_server(Some(shutdown_rx), addr, ServerState { app_state: None }).await
            {
                log!("Server error: {}", e);
            }
        } else {
            shutdown_rx.await.ok();
        }
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

async fn run_server(
    shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    addr: SocketAddr,
    server_state: ServerState,
) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/monitors", get(list_monitors))
        .route(
            "/api/topology",
            get(get_topology_handler).post(set_topology_handler),
        )
        .route("/api/status", get(status_handler))
        .fallback(serve_assets)
        .layer(TraceLayer::new_for_http())
        .with_state(server_state);

    println!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    if let Some(rx) = shutdown_rx {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                rx.await.ok();
            })
            .await?;
    } else {
        // Run without graceful shutdown handler (e.g. console mode, Ctrl+C handled by OS)
        axum::serve(listener, app).await?;
    }

    Ok(())
}

async fn list_monitors() -> Json<Vec<MonitorInfo>> {
    // Attempt to run via Agent in Active Session first
    // This is required when running as a Service (Session 0)
    if let Ok(json_str) = process_win::run_agent_in_active_session(&["list"]) {
        if let Ok(monitors) = serde_json::from_str::<Vec<MonitorInfo>>(&json_str) {
            return Json(monitors);
        }
    }

    // Fallback to local call (Works in Console mode or if Service has access)
    match display::list_monitors() {
        Ok(monitors) => Json(monitors),
        Err(_) => Json(vec![]),
    }
}

#[derive(serde::Deserialize)]
struct TopologyRequest {
    mode: TopologyMode,
}

#[derive(Clone)]
struct ServerState {
    app_state: Option<AppState>,
}

#[derive(serde::Serialize)]
struct TopologyResponse {
    mode: TopologyMode,
}

#[derive(serde::Serialize)]
struct StatusResponse {
    mode: TopologyMode,
    monitoring: bool,
    version: &'static str,
}

async fn get_topology_handler(State(server_state): State<ServerState>) -> Json<TopologyResponse> {
    let mode = server_state
        .app_state
        .as_ref()
        .and_then(|state| state.current_mode.lock().ok().map(|mode| mode.clone()))
        .or_else(|| display::get_topology().ok())
        .unwrap_or(TopologyMode::Clone);

    Json(TopologyResponse { mode })
}

async fn status_handler(State(server_state): State<ServerState>) -> Json<StatusResponse> {
    let mode = server_state
        .app_state
        .as_ref()
        .and_then(|state| state.current_mode.lock().ok().map(|mode| mode.clone()))
        .or_else(|| display::get_topology().ok())
        .unwrap_or(TopologyMode::Clone);

    let monitoring = server_state
        .app_state
        .as_ref()
        .map(|state| state.monitoring_enabled.load(Ordering::Relaxed))
        .unwrap_or(false);

    Json(StatusResponse {
        mode,
        monitoring,
        version: "V0.2.0",
    })
}

async fn set_topology_handler(
    State(server_state): State<ServerState>,
    Json(payload): Json<TopologyRequest>,
) -> Json<String> {
    let requested_mode = payload.mode.clone();

    // Web/API 是本程序的用户意图来源，必须同步 enforcement 目标；
    // 否则 runtime monitoring 会把 API 切换误判成外部修改并回滚。
    if let Some(state) = &server_state.app_state {
        if let Ok(mut target) = state.current_mode.lock() {
            *target = requested_mode.clone();
        }
        state.force_refresh.store(true, Ordering::Relaxed);
    }

    // Attempt to set via Agent in Active Session
    // This is crucial for Service mode as SetDisplayConfig might fail in Session 0
    let mode_str = match &requested_mode {
        TopologyMode::Internal => "internal".to_string(),
        TopologyMode::External => "external".to_string(),
        TopologyMode::Extend => "extend".to_string(),
        TopologyMode::Clone => "clone".to_string(),
        TopologyMode::Single(id_str) => format!("single:{}", id_str),
    };

    if let Ok(output) = process_win::run_agent_in_active_session(&["set", &mode_str]) {
        // Agent usually prints "Success" or error
        return Json(output.trim().to_string());
    }

    // Fallback to local call
    match display::set_topology(requested_mode) {
        Ok(_) => Json("Success".to_string()),
        Err(e) => Json(format!("Error: {}", e)),
    }
}

async fn serve_assets(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
                content.data,
            )
                .into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}
