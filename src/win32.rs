#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, CloseHandle};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_NAME_FORMAT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetSystemMetrics, GetWindow, GetWindowRect, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible, GW_OWNER, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    /// LoL 客户端窗口信息
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    pub struct LolWindow {
        pub hwnd: isize,
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
        pub minimized: bool,
    }

    /// 查找 LoL 客户端窗口
    pub fn find_lol_client_window() -> Option<LolWindow> {
        let mut best: Option<(isize, i32)> = None; // (hwnd, area)

        unsafe {
            let _ = EnumWindows(
                Some(enum_callback),
                LPARAM(&mut best as *mut _ as isize),
            );
        }

        let (hwnd_val, _) = best?;
        let hwnd = HWND(hwnd_val as *mut _);
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut rect);
        }
        let minimized = unsafe { IsIconic(hwnd).as_bool() };

        Some(LolWindow {
            hwnd: hwnd_val,
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            minimized,
        })
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let best = &mut *(lparam.0 as *mut Option<(isize, i32)>);

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        // 排除子窗口（有非空 owner 的窗口）
        if let Ok(h) = GetWindow(hwnd, GW_OWNER) {
            if !h.is_invalid() && h.0 != std::ptr::null_mut() {
                return BOOL(1);
            }
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }

        let exe_name = get_process_image_name(pid);
        if exe_name.is_empty() {
            return BOOL(1);
        }

        let exe_lower = exe_name.to_lowercase();
        let target_exes = [
            "leagueclientux.exe",
            "leagueclient.exe",
            "riotclientservices.exe",
        ];
        let is_target = target_exes.iter().any(|t| exe_lower.ends_with(t));
        if !is_target {
            return BOOL(1);
        }

        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        let minimized = IsIconic(hwnd).as_bool();

        // 最小化窗口 rect 异常（如 -32000），跳过尺寸检查
        if !minimized && (w < 500 || h < 500) {
            return BOOL(1);
        }

        let area = if minimized { 1 } else { w * h };
        let better = match best {
            Some((_, best_area)) => area > *best_area,
            None => true,
        };
        if better {
            *best = Some((hwnd.0 as isize, area));
        }

        BOOL(1)
    }

    fn get_process_image_name(pid: u32) -> String {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
            let Ok(handle) = handle else {
                return String::new();
            };

            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(handle);

            if ok.is_ok() {
                String::from_utf16_lossy(&buf[..size as usize])
            } else {
                String::new()
            }
        }
    }

    /// 获取虚拟屏幕边界 (x, y, w, h)，覆盖所有显示器
    pub fn virtual_screen_rect() -> (i32, i32, i32, i32) {
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            (x, y, w, h)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    #[derive(Debug, Clone)]
    pub struct LolWindow {
        pub hwnd: isize,
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
        pub minimized: bool,
    }

    pub fn find_lol_client_window() -> Option<LolWindow> {
        None
    }

    pub fn virtual_screen_rect() -> (i32, i32, i32, i32) {
        (0, 0, 1920, 1080)
    }
}

pub use imp::*;

