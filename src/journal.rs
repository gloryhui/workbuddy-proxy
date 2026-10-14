//! The bounded core journal, rendered once in chronological order.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn clock(ms: u64) -> String {
    #[cfg(any(windows, target_os = "macos"))]
    {
        unsafe extern "C" {
            fn wb_format_time(ms: u64, buffer: *mut std::ffi::c_char, size: u32);
        }
        let mut buffer = [0u8; 16];
        unsafe {
            wb_format_time(ms, buffer.as_mut_ptr().cast(), buffer.len() as u32);
        }
        String::from_utf8_lossy(&buffer[..buffer.iter().position(|b| *b == 0).unwrap_or(15)])
            .into_owned()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let seconds = ms / 1000;
        format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600 % 24,
            seconds / 60 % 60,
            seconds % 60
        )
    }
}
pub fn render(mut rows: Vec<(u64, String)>, zh: bool) -> String {
    rows.sort_by_key(|row| row.0);
    let mut output = String::new();
    for (time, message) in rows.iter().skip(rows.len().saturating_sub(100)) {
        let translated = crate::i18n::message(message, zh);
        for (i, line) in translated.lines().enumerate() {
            if i == 0 {
                output.push_str(&format!(
                    "{}{}{}\n",
                    clock(*time),
                    if zh { "：" } else { ": " },
                    line
                ));
            } else {
                output.push_str(&format!("    {line}\n"));
            }
        }
    }
    output
}
#[cfg(test)]
mod tests {
    #[test]
    fn entries_sort_in_time_order_and_remain_bounded() {
        let now = super::now_ms();
        let mut rows: Vec<_> = (0..105)
            .rev()
            .map(|n| (now + n, format!("event {n}")))
            .collect();
        rows.push((now + 106, "test\nmodel detail".into()));
        let output = super::render(rows, false);
        assert!(!output.contains(": event 5\n"));
        assert_eq!(output.matches(": event ").count(), 99);
        assert!(output.ends_with(": test\n    model detail\n"));
        assert!(output.find(": event 10\n").unwrap() < output.find(": event 11\n").unwrap());
    }
}
