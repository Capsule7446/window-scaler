use crate::layout::{Position, Rect};
use crate::model::{Size, WindowId, WindowInfo, scaled_size};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    mem::size_of,
    ptr::null_mut,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM, RECT},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
    },
    System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    },
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
        WindowsAndMessaging::*,
    },
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
fn hwnd(id: WindowId) -> HWND {
    id.hwnd as HWND
}
fn error(context: &str) -> String {
    format!("{context}：{}", std::io::Error::last_os_error())
}

pub struct DpiGuard(*mut c_void);
impl DpiGuard {
    pub fn new() -> Self {
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}
impl Drop for DpiGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}

/// A private property is removed by Windows when its HWND is destroyed. Unlike
/// HWND + PID alone, the cookie cannot accidentally identify a reused window.
pub struct Registry {
    property: Vec<u16>,
    counter: AtomicUsize,
    registered: std::sync::Mutex<HashSet<WindowId>>,
    verified_maxima: std::sync::Mutex<HashMap<WindowId, WindowGeometry>>,
    hidden: std::sync::Mutex<HashMap<WindowId, HiddenWindow>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TilingSnapshot {
    pub frame: Rect,
    pub client: Size,
}

impl Registry {
    pub fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            property: wide(&format!(
                "WindowScaler.Identity.{}.{stamp}",
                std::process::id()
            )),
            counter: AtomicUsize::new(1),
            registered: Default::default(),
            verified_maxima: Default::default(),
            hidden: Default::default(),
        }
    }

    pub fn valid(&self, id: WindowId) -> bool {
        if id.cookie == 0 {
            return false;
        }
        unsafe {
            let mut pid = 0;
            IsWindow(hwnd(id)) != 0
                && GetWindowThreadProcessId(hwnd(id), &mut pid) != 0
                && pid == id.pid
                && GetPropW(hwnd(id), self.property.as_ptr()) as usize == id.cookie
        }
    }

    pub fn require(&self, id: WindowId) -> Result<(), String> {
        if self.valid(id) {
            Ok(())
        } else {
            Err("目标已关闭、身份改变或权限不足，请刷新列表".into())
        }
    }

    pub fn discover(&self) -> Result<Vec<WindowInfo>, String> {
        let _dpi = DpiGuard::new();
        unsafe extern "system" fn enumerate(window: HWND, data: LPARAM) -> i32 {
            unsafe {
                (&mut *(data as *mut Vec<usize>)).push(window as usize);
            }
            1
        }
        let mut handles = Vec::<usize>::new();
        if unsafe { EnumWindows(Some(enumerate), &mut handles as *mut _ as LPARAM) } == 0 {
            return Err(error("无法读取窗口列表"));
        }
        let mut result = Vec::new();
        for handle in handles {
            let window = handle as HWND;
            unsafe {
                let mut pid = 0;
                if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == GetCurrentProcessId() {
                    continue;
                }
                let mut cookie = GetPropW(window, self.property.as_ptr()) as usize;
                let managed = self.needs_show(WindowId {
                    hwnd: handle,
                    pid,
                    cookie,
                });
                let extended = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
                if !managed
                    && (IsWindowVisible(window) == 0
                        || extended & WS_EX_TOOLWINDOW != 0
                        || (!GetWindow(window, GW_OWNER).is_null()
                            && extended & WS_EX_APPWINDOW == 0))
                {
                    continue;
                }
                let mut cloaked: u32 = 0;
                if !managed
                    && DwmGetWindowAttribute(
                        window,
                        DWMWA_CLOAKED as u32,
                        &mut cloaked as *mut _ as *mut c_void,
                        size_of::<u32>() as u32,
                    ) >= 0
                    && cloaked != 0
                {
                    continue;
                }
                let class = window_class(window);
                if !managed
                    && matches!(
                        class.as_str(),
                        "Shell_TrayWnd"
                            | "Shell_SecondaryTrayWnd"
                            | "Progman"
                            | "WorkerW"
                            | "tooltips_class32"
                    )
                {
                    continue;
                }
                let mut title = window_title(window);
                if title.trim().is_empty() {
                    if !managed {
                        continue;
                    }
                    title = "无标题窗口".into();
                }
                let mut unavailable = None;
                if cookie == 0 {
                    cookie = self.counter.fetch_add(1, Ordering::Relaxed);
                    if SetPropW(window, self.property.as_ptr(), cookie as *mut c_void) == 0 {
                        cookie = 0;
                        unavailable = Some("无法验证窗口身份，可能需要相同权限".into());
                    }
                }
                let id = WindowId {
                    hwnd: handle,
                    pid,
                    cookie,
                };
                if cookie != 0 {
                    self.registered.lock().unwrap().insert(id);
                }
                result.push(WindowInfo {
                    id,
                    title,
                    program: process_name(pid),
                    size: if IsIconic(window) != 0 {
                        None
                    } else {
                        client_size(window).ok()
                    },
                    hidden: IsWindowVisible(window) == 0,
                    unavailable,
                });
            }
        }
        self.registered.lock().unwrap().retain(|id| self.valid(*id));
        self.verified_maxima
            .lock()
            .unwrap()
            .retain(|id, _| self.valid(*id));
        self.hidden.lock().unwrap().retain(|id, _| self.valid(*id));
        result.sort_by(|a, b| {
            a.program
                .to_lowercase()
                .cmp(&b.program.to_lowercase())
                .then(a.id.pid.cmp(&b.id.pid))
                .then(a.title.cmp(&b.title))
        });
        Ok(result)
    }

    pub fn needs_show(&self, id: WindowId) -> bool {
        self.hidden.lock().unwrap().contains_key(&id)
    }

    pub fn measure_tiling(&self, id: WindowId) -> Result<TilingSnapshot, String> {
        let _dpi = DpiGuard::new();
        self.require(id)?;
        if self.is_hidden(id) {
            return Err("排列只移动位置；请先显示此窗口".into());
        }
        {
            let mut hidden = self.hidden.lock().unwrap();
            if hidden.get(&id).is_some_and(|saved| saved.restore_started) {
                return Err("此前显示恢复尚未完成；请先重试显示此窗口".into());
            }
            // Like Show, accept a target which has made itself visible. Its
            // obsolete hidden geometry must not be reused by the next Hide.
            hidden.remove(&id);
        }
        if unsafe { IsIconic(hwnd(id)) != 0 || IsZoomed(hwnd(id)) != 0 } {
            return Err("排列只移动位置；请先将此窗口恢复为普通窗口".into());
        }
        let [left, top, right, bottom] = window_frame(hwnd(id))?;
        let frame = Rect {
            left,
            top,
            right,
            bottom,
        };
        frame.size()?;
        Ok(TilingSnapshot {
            frame,
            client: client_size(hwnd(id))?,
        })
    }

    pub fn move_tiled(
        &self,
        id: WindowId,
        expected: TilingSnapshot,
        target: Position,
    ) -> Result<Size, String> {
        let _dpi = DpiGuard::new();
        if self.measure_tiling(id)? != expected {
            return Err("窗口位置或大小已改变，未移动；请重新排列".into());
        }
        let size = expected.frame.size()?;
        let right = target.x.checked_add(size.width).ok_or("排列位置超出范围")?;
        let bottom = target
            .y
            .checked_add(size.height)
            .ok_or("排列位置超出范围")?;
        self.require(id)?;
        if unsafe {
            SetWindowPos(
                hwnd(id),
                null_mut(),
                target.x,
                target.y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
            )
        } == 0
        {
            return Err(error("目标未接受排列"));
        }
        std::thread::sleep(Duration::from_millis(120));
        let actual = self.measure_tiling(id)?;
        let wanted = Rect {
            left: target.x,
            top: target.y,
            right,
            bottom,
        };
        if actual.frame != wanted || actual.client != expected.client {
            return Err("目标未保持排列位置或原大小，请检查窗口后重试".into());
        }
        Ok(actual.client)
    }

    pub fn is_hidden(&self, id: WindowId) -> bool {
        self.valid(id) && unsafe { IsWindowVisible(hwnd(id)) == 0 }
    }

    pub fn hide(&self, id: WindowId) -> Result<(), String> {
        let _dpi = DpiGuard::new();
        self.require(id)?;
        if !self.needs_show(id) {
            if self.is_hidden(id) {
                return Err("目标已被其他程序隐藏，未改变其状态".into());
            }
            let frame = window_frame(hwnd(id))?;
            let mut placement = WINDOWPLACEMENT {
                length: size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if unsafe { GetWindowPlacement(hwnd(id), &mut placement) } == 0 {
                return Err(error("无法读取隐藏前的窗口状态"));
            }
            let state = unsafe { (IsIconic(hwnd(id)) != 0, IsZoomed(hwnd(id)) != 0) };
            self.require(id)?;
            // Record before submitting a synchronous request: a timed-out worker
            // may still hide the target, so exit must retain its recovery entry.
            self.hidden.lock().unwrap().insert(
                id,
                HiddenWindow {
                    frame,
                    placement,
                    state,
                    restore_started: false,
                },
            );
        }
        unsafe { ShowWindow(hwnd(id), SW_HIDE) };
        std::thread::sleep(Duration::from_millis(120));
        self.require(id)?;
        if !self.is_hidden(id) {
            return Err("目标未接受隐藏".into());
        }
        Ok(())
    }

    pub fn show(&self, id: WindowId) -> Result<(), String> {
        let _dpi = DpiGuard::new();
        self.require(id)?;
        let saved = self.hidden.lock().unwrap().get(&id).copied();
        let Some(saved) = saved else {
            return if self.is_hidden(id) {
                Err("目标不是由本工具隐藏，无法恢复隐藏前状态".into())
            } else {
                Ok(())
            };
        };
        if !saved.restore_started && !self.is_hidden(id) {
            // A target which has shown itself no longer needs our recovery.
            // Do not move it back when the user clicks Show again.
            self.hidden.lock().unwrap().remove(&id);
            return Ok(());
        }
        if let Some(record) = self.hidden.lock().unwrap().get_mut(&id) {
            // A failed restoration may leave a visible window at the wrong
            // geometry. Keep that case retryable instead of accepting it as-is.
            record.restore_started = true;
        }
        let frame = saved.frame;
        let current_state = unsafe { (IsIconic(hwnd(id)) != 0, IsZoomed(hwnd(id)) != 0) };
        if current_state != saved.state {
            let mut placement = saved.placement;
            // Keep cross-thread requests synchronous, like all other operations.
            placement.flags &= !WPF_ASYNCWINDOWPLACEMENT;
            self.require(id)?;
            if unsafe { SetWindowPlacement(hwnd(id), &placement) } == 0 {
                return Err(error("无法恢复隐藏前的窗口状态"));
            }
            self.require(id)?;
        }
        // WINDOWPLACEMENT uses workspace coordinates; only GetWindowRect's
        // screen coordinates go to SetWindowPos. Avoid clamping small windows.
        if unsafe {
            SetWindowPos(
                hwnd(id),
                null_mut(),
                frame[0],
                frame[1],
                frame[2] - frame[0],
                frame[3] - frame[1],
                SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
            )
        } == 0
        {
            return Err(error("目标未接受显示"));
        }
        std::thread::sleep(Duration::from_millis(120));
        self.require(id)?;
        let state = unsafe { (IsIconic(hwnd(id)) != 0, IsZoomed(hwnd(id)) != 0) };
        if self.is_hidden(id) || window_frame(hwnd(id))? != frame || state != saved.state {
            return Err("目标未保持显示状态或隐藏前的位置和大小".into());
        }
        self.hidden.lock().unwrap().remove(&id);
        Ok(())
    }

    pub fn resize(
        &self,
        id: WindowId,
        width: u32,
        baseline: &mut Option<Size>,
        affected: &AtomicBool,
    ) -> Result<Size, String> {
        let _dpi = DpiGuard::new();
        self.require(id)?;
        let window = hwnd(id);
        unsafe {
            // Restoring a minimized window can return it to maximized. Handle
            // those two transitions separately (WPF_RESTORETOMAXIMIZED).
            let was_minimized = IsIconic(window) != 0;
            if was_minimized {
                affected.store(true, Ordering::SeqCst);
                ShowWindow(window, SW_RESTORE);
                self.require(id)?;
                if IsIconic(window) != 0 {
                    return Err("窗口未接受恢复显示".into());
                }
            }
            let original = baseline.unwrap_or(client_size(window)?);
            // Preserve the baseline once a state-changing call may take effect.
            // A validation-only failure on a normal window must not freeze it.
            if was_minimized || IsZoomed(window) != 0 {
                *baseline = Some(original);
            }
            if IsZoomed(window) != 0 {
                affected.store(true, Ordering::SeqCst);
                ShowWindow(window, SW_RESTORE);
                self.require(id)?;
                if IsIconic(window) != 0 || IsZoomed(window) != 0 {
                    return Err("窗口未接受恢复显示".into());
                }
            }
            let current = client_size(window)?;
            let wanted = scaled_size(width, original)?;
            let mut frame = RECT::default();
            if GetWindowRect(window, &mut frame) == 0 {
                return Err(error("无法读取窗口边框"));
            }
            let extra_w =
                (i64::from(frame.right) - i64::from(frame.left) - i64::from(current.width)).max(0);
            let extra_h =
                (i64::from(frame.bottom) - i64::from(frame.top) - i64::from(current.height)).max(0);
            let outer_w = i64::from(wanted.width) + extra_w;
            let outer_h = i64::from(wanted.height) + extra_h;
            let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return Err(error("无法读取显示器工作区"));
            }
            let area = info.rcWork;
            if outer_w > i64::from(area.right) - i64::from(area.left)
                || outer_h > i64::from(area.bottom) - i64::from(area.top)
            {
                return Err(format!(
                    "所需尺寸 {}×{} 超出当前显示器工作区",
                    wanted.width, wanted.height
                ));
            }
            let x = frame.left.clamp(area.left, area.right - outer_w as i32);
            let y = frame.top.clamp(area.top, area.bottom - outer_h as i32);
            self.require(id)?;
            *baseline = Some(original);
            affected.store(true, Ordering::SeqCst);
            // Synchronous on this window's dedicated worker. Never queue an async
            // resize which could run after exit's maximize operation.
            if SetWindowPos(
                window,
                null_mut(),
                x,
                y,
                outer_w as i32,
                outer_h as i32,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
            ) == 0
            {
                return Err(error("窗口未接受缩小"));
            }
            std::thread::sleep(Duration::from_millis(120));
            self.require(id)?;
            let actual = client_size(window)?;
            if actual != wanted {
                return Err(format!(
                    "目标未保持请求尺寸；实际 {}×{}，请求 {}×{}",
                    actual.width, actual.height, wanted.width, wanted.height
                ));
            }
            Ok(actual)
        }
    }

    pub fn restore_original(
        &self,
        id: WindowId,
        original: Size,
        affected: &AtomicBool,
    ) -> Result<Size, String> {
        let width = u32::try_from(original.width).map_err(|_| "原尺寸无效".to_owned())?;
        self.resize(id, width, &mut Some(original), affected)
    }

    pub fn maximize(&self, id: WindowId) -> Result<Size, String> {
        let _dpi = DpiGuard::new();
        self.require(id)?;
        let window = hwnd(id);
        let initial = if unsafe { IsIconic(window) } == 0 {
            Some(window_geometry(window)?)
        } else {
            None
        };
        unsafe {
            if IsIconic(window) != 0 {
                SendMessageW(window, WM_SYSCOMMAND, SC_RESTORE as usize, 0);
                std::thread::sleep(Duration::from_millis(120));
                self.require(id)?;
                if IsIconic(window) != 0 {
                    return Err("目标未接受恢复显示".into());
                }
            }
            if IsZoomed(window) != 0 {
                let current = window_geometry(window)?;
                let verified = self.verified_maxima.lock().unwrap().get(&id).copied();
                if verified == Some(current) || current.covers_work_area() {
                    return Ok(current.client);
                }
            }
            // A stale WS_MAXIMIZE flag can coexist with a miniature frame.
            // Use the target's system-command path, as its title-bar buttons do.
            // Keep these calls synchronous on this window's dedicated worker.
            if IsZoomed(window) != 0 {
                self.require(id)?;
                SendMessageW(window, WM_SYSCOMMAND, SC_RESTORE as usize, 0);
                std::thread::sleep(Duration::from_millis(120));
                self.require(id)?;
                if IsIconic(window) != 0 || IsZoomed(window) != 0 {
                    return Err("目标未接受还原显示，无法重新确认最大化".into());
                }
            }
            let normal = client_size(window)?;
            self.require(id)?;
            SendMessageW(window, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0);
            std::thread::sleep(Duration::from_millis(120));
            self.require(id)?;
            if IsIconic(window) != 0 || IsZoomed(window) == 0 {
                return Err("目标未进入最大化状态".into());
            }
            let actual = window_geometry(window)?;
            if !actual.covers_work_area()
                && !initial.is_some_and(|initial| actual.grew_from(initial.client))
                && !actual.grew_from(normal)
            {
                return Err(format!(
                    "目标报告最大化，但实际尺寸未变大（{}×{}）；请在目标程序内确认",
                    actual.client.width, actual.client.height
                ));
            }
            self.verified_maxima.lock().unwrap().insert(id, actual);
            Ok(actual.client)
        }
    }

    pub fn cleanup(&self) {
        for id in self.registered.lock().unwrap().drain() {
            if self.valid(id) {
                unsafe {
                    RemovePropW(hwnd(id), self.property.as_ptr());
                }
            }
        }
        self.verified_maxima.lock().unwrap().clear();
        self.hidden.lock().unwrap().clear();
    }
}

#[derive(Clone, Copy)]
struct HiddenWindow {
    frame: [i32; 4],
    placement: WINDOWPLACEMENT,
    state: (bool, bool),
    restore_started: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct WindowGeometry {
    client: Size,
    frame: [i32; 4],
    work_area: [i32; 4],
}

impl WindowGeometry {
    fn covers_work_area(self) -> bool {
        self.frame[0] <= self.work_area[0]
            && self.frame[1] <= self.work_area[1]
            && self.frame[2] >= self.work_area[2]
            && self.frame[3] >= self.work_area[3]
    }

    fn grew_from(self, before: Size) -> bool {
        self.client.width >= before.width
            && self.client.height >= before.height
            && self.client != before
    }
}

fn window_frame(window: HWND) -> Result<[i32; 4], String> {
    let mut frame = RECT::default();
    if unsafe { GetWindowRect(window, &mut frame) } == 0 {
        return Err(error("无法读取窗口边框"));
    }
    Ok([frame.left, frame.top, frame.right, frame.bottom])
}

pub fn work_area(reference: usize) -> Result<Rect, String> {
    let _dpi = DpiGuard::new();
    let monitor = unsafe { MonitorFromWindow(reference as HWND, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return Err(error("无法读取显示器工作区"));
    }
    Ok(Rect {
        left: info.rcWork.left,
        top: info.rcWork.top,
        right: info.rcWork.right,
        bottom: info.rcWork.bottom,
    })
}

fn window_geometry(window: HWND) -> Result<WindowGeometry, String> {
    unsafe {
        let mut frame = RECT::default();
        if GetWindowRect(window, &mut frame) == 0 {
            return Err(error("无法读取窗口边框"));
        }
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err(error("无法读取显示器工作区"));
        }
        Ok(WindowGeometry {
            client: client_size(window)?,
            frame: [frame.left, frame.top, frame.right, frame.bottom],
            work_area: [
                info.rcWork.left,
                info.rcWork.top,
                info.rcWork.right,
                info.rcWork.bottom,
            ],
        })
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn client_size(window: HWND) -> Result<Size, String> {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(window, &mut rect) } == 0 {
        return Err(error("无法读取内容区尺寸"));
    }
    let size = Size {
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    };
    if size.width <= 0 || size.height <= 0 {
        return Err("窗口没有有效的内容区尺寸".into());
    }
    Ok(size)
}

fn window_title(window: HWND) -> String {
    let mut text = [0u16; 2048];
    let len = unsafe { GetWindowTextW(window, text.as_mut_ptr(), text.len() as i32) };
    String::from_utf16_lossy(&text[..len.max(0) as usize])
}
fn window_class(window: HWND) -> String {
    let mut text = [0u16; 256];
    let len = unsafe { GetClassNameW(window, text.as_mut_ptr(), text.len() as i32) };
    String::from_utf16_lossy(&text[..len.max(0) as usize])
}
fn process_name(pid: u32) -> String {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return "未知程序".into();
        }
        let mut text = [0u16; 32768];
        let mut len = text.len() as u32;
        let success = QueryFullProcessImageNameW(process, 0, text.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if success == 0 {
            return "未知程序".into();
        }
        String::from_utf16_lossy(&text[..len as usize])
            .rsplit('\\')
            .next()
            .unwrap_or("未知程序")
            .to_owned()
    }
}

pub fn show_main(handle: usize) {
    unsafe {
        ShowWindow(handle as HWND, SW_RESTORE);
        SetForegroundWindow(handle as HWND);
    }
}
pub fn hide_main(handle: usize) {
    unsafe {
        ShowWindow(handle as HWND, SW_HIDE);
    }
}

pub fn fatal_message(message: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("窗口缩放器").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
