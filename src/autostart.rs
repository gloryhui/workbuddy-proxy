//! Opt-in, per-user login startup. Never register anything on ordinary startup.
use std::path::Path;

pub fn set(enabled: bool, dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    return macos::set(&crate::config::home()?, &exe, dir, enabled);
    #[cfg(windows)]
    return windows::set(&exe, dir, enabled);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (enabled, exe, dir);
        Err(
            "Login startup is supported on Windows and macOS / 开机启动仅支持 Windows 和 macOS"
                .into(),
        )
    }
}

#[cfg(any(target_os = "macos", test))]
mod macos {
    use std::{fs, path::Path};

    pub const LABEL: &str = "io.github.gloryhui.workbuddy-proxy.autostart";

    fn xml(path: &Path) -> Result<String, String> {
        let text = path
            .to_str()
            .ok_or("Invalid startup path / 开机启动路径无效")?;
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
        {
            return Err("Invalid startup path / 开机启动路径无效".into());
        }
        Ok(text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;"))
    }

    pub fn set(home: &Path, exe: &Path, dir: &Path, enabled: bool) -> Result<(), String> {
        let agents = home.join("Library/LaunchAgents");
        let path = agents.join(format!("{LABEL}.plist"));
        if !enabled {
            return match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            };
        }
        let (exe, dir) = (xml(exe)?, xml(dir)?);
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{exe}</string><string>--autostart</string><string>--config-dir</string><string>{dir}</string></array>
<key>RunAtLoad</key><true/>
<key>LimitLoadToSessionType</key><string>Aqua</string>
<key>ProcessType</key><string>Interactive</string>
<key>AssociatedBundleIdentifiers</key><array><string>io.github.gloryhui.workbuddy-proxy</string></array>
</dict></plist>
"#
        );
        fs::create_dir_all(&agents).map_err(|e| e.to_string())?;
        crate::config::write_bytes(&path, plist.as_bytes())
    }
}

#[cfg(any(windows, test))]
fn windows_command(exe: &Path, dir: &Path) -> Result<String, String> {
    // Quote arguments using the Windows command-line rules, including trailing
    // backslashes in a configuration directory. REG_SZ does not expand %vars%.
    fn quote(path: &Path) -> Result<String, String> {
        let text = path
            .to_str()
            .ok_or("Invalid startup path / 开机启动路径无效")?;
        if text.contains('\0') {
            return Err("Invalid startup path / 开机启动路径无效".into());
        }
        let mut out = String::from("\"");
        let mut slashes = 0;
        for c in text.chars() {
            if c == '\\' {
                slashes += 1;
                continue;
            }
            if c == '"' {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
            } else {
                out.extend(std::iter::repeat_n('\\', slashes));
            }
            slashes = 0;
            out.push(c);
        }
        out.extend(std::iter::repeat_n('\\', slashes * 2));
        out.push('"');
        Ok(out)
    }
    let command = format!("{} --autostart --config-dir {}", quote(exe)?, quote(dir)?);
    if command.encode_utf16().count() > 260 {
        return Err("Startup command exceeds 260 characters; use a shorter installation/configuration path / 开机启动命令超过 260 字符，请使用较短的安装或配置路径".into());
    }
    Ok(command)
}

#[cfg(windows)]
mod windows {
    use std::{
        ffi::{OsStr, c_void},
        os::windows::ffi::OsStrExt,
        path::Path,
        ptr,
    };
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "WorkBuddyProxy";
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegCreateKeyExW(
            key: *mut c_void,
            subkey: *const u16,
            reserved: u32,
            class: *mut u16,
            options: u32,
            access: u32,
            security: *const c_void,
            result: *mut *mut c_void,
            disposition: *mut u32,
        ) -> i32;
        fn RegOpenKeyExW(
            key: *mut c_void,
            subkey: *const u16,
            options: u32,
            access: u32,
            result: *mut *mut c_void,
        ) -> i32;
        fn RegSetValueExW(
            key: *mut c_void,
            name: *const u16,
            reserved: u32,
            kind: u32,
            data: *const u8,
            size: u32,
        ) -> i32;
        fn RegDeleteValueW(key: *mut c_void, name: *const u16) -> i32;
        fn RegCloseKey(key: *mut c_void) -> i32;
    }
    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(Some(0)).collect()
    }
    pub fn set(exe: &Path, dir: &Path, enabled: bool) -> Result<(), String> {
        // Validate before creating or modifying any registry key.
        let command = enabled
            .then(|| super::windows_command(exe, dir))
            .transpose()?;
        let subkey = wide(OsStr::new(RUN));
        let name = wide(OsStr::new(NAME));
        let current_user = 0x80000001u32 as i32 as isize as *mut c_void;
        let mut key = ptr::null_mut();
        let status = unsafe {
            if enabled {
                RegCreateKeyExW(
                    current_user,
                    subkey.as_ptr(),
                    0,
                    ptr::null_mut(),
                    0,
                    2,
                    ptr::null(),
                    &mut key,
                    ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(current_user, subkey.as_ptr(), 0, 2, &mut key)
            }
        };
        if !enabled && status == 2 {
            return Ok(());
        }
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status).to_string());
        }
        let result = unsafe {
            if let Some(command) = command {
                let value = wide(OsStr::new(&command));
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    1,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            } else {
                RegDeleteValueW(key, name.as_ptr())
            }
        };
        unsafe {
            RegCloseKey(key);
        }
        if result == 0 || (!enabled && result == 2) {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(result).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn registration_is_opt_in_and_removes_only_our_agent() {
        let home = tempfile::tempdir().unwrap();
        let exe = Path::new(
            "/Applications/中文 & Friends/WorkBuddy Proxy.app/Contents/MacOS/workbuddy-proxy",
        );
        let dir = Path::new("/Users/test/Config <private>");
        let agents = home.path().join("Library/LaunchAgents");
        macos::set(home.path(), exe, dir, false).unwrap();
        assert!(!agents.exists());
        macos::set(home.path(), exe, dir, true).unwrap();
        let plist = fs::read_to_string(agents.join(format!("{}.plist", macos::LABEL))).unwrap();
        assert!(plist.contains("中文 &amp; Friends"));
        assert!(plist.contains("Config &lt;private&gt;"));
        assert!(plist.contains("<string>--autostart</string>"));
        assert!(!plist.contains("KeepAlive"));
        let other = agents.join("other.plist");
        fs::write(&other, b"keep").unwrap();
        macos::set(home.path(), exe, dir, false).unwrap();
        macos::set(home.path(), exe, dir, false).unwrap();
        assert_eq!(fs::read_dir(&agents).unwrap().count(), 1);
        assert_eq!(fs::read(other).unwrap(), b"keep");
    }

    #[test]
    fn windows_startup_quotes_unicode_spaces_and_trailing_slashes() {
        assert_eq!(
            windows_command(
                Path::new(r"C:\中文 App\workbuddy-proxy.exe"),
                Path::new("C:\\配置 目录\\")
            )
            .unwrap(),
            "\"C:\\中文 App\\workbuddy-proxy.exe\" --autostart --config-dir \"C:\\配置 目录\\\\\""
        );
        assert!(windows_command(Path::new(&"x".repeat(261)), Path::new("config")).is_err());
    }
}
