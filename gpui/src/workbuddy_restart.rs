//! Explicit user-triggered app lifecycle; no shell, process-name kill or forced quit.
pub fn restart() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn wb_restart_workbuddy() -> std::ffi::c_int;
        }
        // This function must run on the settings worker, never the main thread.
        result(unsafe { wb_restart_workbuddy() })
    }
    #[cfg(not(target_os = "macos"))]
    Err("Automatic restart currently supports macOS; please reopen WorkBuddy manually / 自动重启暂仅支持 macOS，请手动重开 WorkBuddy".into())
}

#[cfg(any(target_os = "macos", test))]
fn result(code: i32) -> Result<(), String> {
    match code {
        0 => Ok(()),
        1 => Err("WorkBuddy app was not found; please open it manually / 未找到 WorkBuddy 应用，请手动打开".into()),
        2 => Err("Multiple WorkBuddy instances are running; please restart them manually / 检测到多个 WorkBuddy 实例，请手动重启".into()),
        3 => Err("WorkBuddy could not be asked to quit; please restart it manually / 无法请求 WorkBuddy 正常退出，请手动重启".into()),
        4 => Err("WorkBuddy did not exit within 20 seconds; no forced quit was attempted. Please handle any WorkBuddy dialog and reopen it manually / WorkBuddy 在 20 秒内未退出，未强制结束；请处理 WorkBuddy 的弹窗后手动重开".into()),
        5 => Err("WorkBuddy exited but could not be reopened; please open it manually / WorkBuddy 已退出但重新打开失败，请手动打开".into()),
        _ => Err("WorkBuddy launch was not confirmed; check whether it opened / 尚未确认 WorkBuddy 启动成功，请检查是否已打开".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lifecycle_errors_preserve_the_actual_stage() {
        assert!(super::result(0).is_ok());
        assert!(super::result(4).unwrap_err().contains("未强制结束"));
        assert!(super::result(5).unwrap_err().contains("已退出"));
        assert!(super::result(6).unwrap_err().contains("尚未确认"));
    }
}
