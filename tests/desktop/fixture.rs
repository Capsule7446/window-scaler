use std::{
    ffi::c_void,
    path::Path,
    process::{Child, Command},
    ptr::null_mut,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};

pub const BLOCK: u32 = WM_APP + 60;
pub const REJECT: u32 = WM_APP + 61;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

unsafe extern "system" fn procedure(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        BLOCK => {
            unsafe {
                SetPropW(
                    window,
                    wide("WindowScaler.Acceptance.BlockStarted").as_ptr(),
                    1usize as _,
                );
            }
            thread::sleep(Duration::from_millis(5500));
            0
        }
        REJECT => {
            unsafe {
                SetWindowLongPtrW(window, GWLP_USERDATA, 1);
            }
            0
        }
        WM_SYSCOMMAND if unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } == 1 => 0,
        WM_WINDOWPOSCHANGED => unsafe {
            let result = DefWindowProcW(window, message, w, l);
            let mut rect = RECT::default();
            GetWindowRect(window, &mut rect);
            if GetWindowLongPtrW(window, GWLP_USERDATA) == 1 && rect.right - rect.left > 300 {
                SetWindowPos(
                    window,
                    null_mut(),
                    0,
                    0,
                    256,
                    174,
                    SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
                );
            }
            result
        },
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, message, w, l) },
    }
}

pub fn size(handle: usize) -> crate::model::Size {
    let mut rect = RECT::default();
    assert_ne!(unsafe { GetClientRect(handle as HWND, &mut rect) }, 0);
    crate::model::Size {
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    }
}

pub fn frame(handle: usize) -> [i32; 4] {
    let _dpi = crate::native::DpiGuard::new();
    let mut rect = RECT::default();
    assert_ne!(unsafe { GetWindowRect(handle as HWND, &mut rect) }, 0);
    [rect.left, rect.top, rect.right, rect.bottom]
}

pub fn block_started(handle: usize) -> bool {
    unsafe {
        !GetPropW(
            handle as HWND,
            wide("WindowScaler.Acceptance.BlockStarted").as_ptr(),
        )
        .is_null()
    }
}

pub fn serve(path: &str) {
    let mut workers = Vec::new();
    let mut handles = Vec::new();
    for index in 0..2 {
        let (tx, rx) = std::sync::mpsc::channel();
        let (stop, stopped) = std::sync::mpsc::channel();
        workers.push((
            stop,
            thread::spawn(move || unsafe {
                let _dpi = crate::native::DpiGuard::new();
                let class = wide(&format!(
                    "WindowScalerAcceptance.{}.{}",
                    std::process::id(),
                    index
                ));
                let module = GetModuleHandleW(null_mut());
                let descriptor = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: module,
                    lpszClassName: class.as_ptr(),
                    ..Default::default()
                };
                assert_ne!(RegisterClassW(&descriptor), 0);
                let window = CreateWindowExW(
                    WS_EX_NOACTIVATE,
                    class.as_ptr(),
                    wide(&format!("Scaler Acceptance {index}")).as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    80 + index * 40,
                    80 + index * 40,
                    900,
                    600,
                    null_mut(),
                    null_mut(),
                    module,
                    null_mut::<c_void>(),
                );
                assert!(!window.is_null());
                ShowWindow(window, SW_SHOWNOACTIVATE);
                tx.send(window as usize).unwrap();
                let mut message = MSG::default();
                'messages: loop {
                    if stopped.try_recv().is_ok() {
                        // Only the creating thread destroys its still-live window.
                        // A closed fixture has already left this loop via WM_QUIT.
                        DestroyWindow(window);
                        break;
                    }
                    while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                        if message.message == WM_QUIT {
                            break 'messages;
                        }
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                UnregisterClassW(class.as_ptr(), module);
            }),
        ));
        handles.push(rx.recv().unwrap());
    }
    std::fs::write(path, serde_json::to_vec(&handles).unwrap()).unwrap();
    // A watchdog also removes fixture windows if the acceptance parent crashes.
    let start = Instant::now();
    while !Path::new(&format!("{path}.stop")).exists() && start.elapsed() < Duration::from_secs(40)
    {
        thread::sleep(Duration::from_millis(50));
    }
    for (stop, _) in &workers {
        let _ = stop.send(());
    }
    for (_, worker) in workers {
        worker.join().unwrap();
    }
}

pub struct Fixtures {
    pub handles: Vec<usize>,
    pub pid: u32,
    child: Child,
    path: String,
}
impl Fixtures {
    pub fn start() -> Self {
        let path = std::env::temp_dir()
            .join(format!(
                "window-scaler-fixture-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .to_string_lossy()
            .into_owned();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--fixture", &path])
            .spawn()
            .unwrap();
        let pid = child.id();
        let start = Instant::now();
        let handles = loop {
            if let Ok(bytes) = std::fs::read(&path)
                && let Ok(handles) = serde_json::from_slice(&bytes)
            {
                break handles;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "fixture startup timeout"
            );
            thread::sleep(Duration::from_millis(20));
        };
        Self {
            handles,
            pid,
            child,
            path,
        }
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) {
        let stop = format!("{}.stop", self.path);
        let _ = std::fs::write(&stop, b"stop");
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(stop);
    }
}
