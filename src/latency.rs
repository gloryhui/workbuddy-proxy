// Request-scoped timer precision on Windows. No busy loop, periodic polling,
// extra thread, or precision request while the proxy is idle.
#[cfg(windows)]
#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
}
pub struct ActiveRequest {
    #[cfg(windows)]
    enabled: bool,
}
impl ActiveRequest {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            enabled: unsafe { timeBeginPeriod(1) == 0 },
        }
    }
}
#[cfg(windows)]
impl Drop for ActiveRequest {
    fn drop(&mut self) {
        if self.enabled {
            unsafe {
                timeEndPeriod(1);
            }
        }
    }
}
