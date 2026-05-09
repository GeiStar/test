//! 日志模块 - 静默失败、追加写入、毫秒级时间戳
//!
//! 对应 PS1 Write-Log 函数的 Rust 移植
//! 特性：
//! - 时间戳精度：毫秒级 (.fff)
//! - 编码：UTF-8
//! - 失败策略：静默失败 (try/catch + 空处理)
//! - 写入模式：追加 (append)

use chrono::Local;
use once_cell::sync::Lazy;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 日志实例管理器
static LOGGER: Lazy<Mutex<Option<Logger>>> = Lazy::new(|| Mutex::new(None));

/// 日志结构体
#[derive(Debug)]
pub struct Logger {
    log_path: Option<PathBuf>,
}

impl Logger {
    /// 初始化日志系统
    ///
    /// # Arguments
    /// * `log_path` - 日志文件路径，如果为 None 则不记录
    pub fn init(log_path: Option<impl AsRef<Path>>) {
        let logger = Self {
            log_path: log_path.map(|p| p.as_ref().to_path_buf()),
        };

        if let Ok(mut guard) = LOGGER.lock() {
            *guard = Some(logger);
        }
    }

    /// 写入日志 (私有实现)
    fn write_log(&self, message: &str, event_driven: bool) {
        let log_path = match &self.log_path {
            Some(p) => p,
            None => return, // 日志路径未设置，静默返回
        };

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        // 处理换行符
        let clean_msg = if event_driven {
            message.replace(|c: char| c == '\r' || c == '\n', " ")
        } else {
            message.to_string()
        };

        let log_line = format!("[{}] {}\r\n", timestamp, clean_msg);

        // 静默失败 - 使用 let _ 忽略所有错误
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .and_then(|mut file| file.write_all(log_line.as_bytes()));
    }
}

/// 写入日志 - 公共接口
///
/// # Arguments
/// * `message` - 日志消息
///
/// # Example
/// ```rust
/// log!("Application started");
/// ```
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::logger::write_log(&format!($($arg)*), false);
    };
}

/// 写入日志（事件驱动模式 - 折叠换行符）
#[macro_export]
macro_rules! log_event {
    ($($arg:tt)*) => {
        $crate::logger::write_log(&format!($($arg)*), true);
    };
}

/// 直接写入日志的函数接口
pub fn write_log(message: &str, event_driven: bool) {
    if let Ok(guard) = LOGGER.lock() {
        if let Some(ref logger) = *guard {
            logger.write_log(message, event_driven);
        }
    }
}

/// 输出启动日志头部
///
/// # Arguments
/// * `version` - 应用版本号
/// * `initial_mode` - 初始显示模式
pub fn log_header(version: &str, initial_mode: &str) {
    log!("==========================================");
    log!("Display Manager {} Started", version);
    log!("OS: Windows {:?}", std::env::consts::OS);
    log!("Initial Mode: {}", initial_mode);
    log!("==========================================");
}

/// 输出关闭日志
pub fn log_footer() {
    log!("Cleanup...");
    log!("Terminated");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_logger_silent_failure() {
        // 测试路径无效时静默失败
        Logger::init(Some("/nonexistent/path/to/log.log"));
        write_log("Test message", false);
        // 不应 panic
    }

    #[test]
    fn test_logger_with_file() {
        let temp_path = std::env::temp_dir().join("test_monitor_service.log");

        // 清理之前的测试文件
        let _ = fs::remove_file(&temp_path);

        Logger::init(Some(&temp_path));
        write_log("Test log entry", false);

        // 验证文件存在且包含内容
        let content = fs::read_to_string(&temp_path).unwrap();
        assert!(content.contains("Test log entry"));

        // 清理
        let _ = fs::remove_file(&temp_path);
    }
}
