use super::*;
use crate::backend::{Action, Backend, Event};
use std::{sync::mpsc, thread, time::Instant};
use windows_sys::Win32::{
    Foundation::{LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
};

const CLOSE_FIXTURE: u32 = WM_APP + 47;
const BLOCK_FIXTURE: u32 = WM_APP + 48;
static BLOCK_STARTED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn fixture_proc(
    window: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match message {
        WM_GETMINMAXINFO => {
            unsafe {
                let info = &mut *(l as *mut MINMAXINFO);
                info.ptMinTrackSize.x = 800;
                info.ptMinTrackSize.y = 450;
            }
            0
        }
        CLOSE_FIXTURE => {
            unsafe {
                DestroyWindow(window);
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        BLOCK_FIXTURE => {
            BLOCK_STARTED.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(5500));
            0
        }
        _ => unsafe { DefWindowProcW(window, message, w, l) },
    }
}

struct Fixture {
    handle: usize,
    thread: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || unsafe {
            let _dpi = DpiGuard::new();
            let class = wide(&format!("WindowScalerTest.{}", std::process::id()));
            let module = GetModuleHandleW(std::ptr::null());
            let descriptor = WNDCLASSW {
                lpfnWndProc: Some(fixture_proc),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            RegisterClassW(&descriptor);
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("窗口缩放器自动验收窗口").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                80,
                80,
                1000,
                700,
                null_mut(),
                null_mut(),
                module,
                std::ptr::null(),
            );
            assert!(!window.is_null());
            tx.send(window as usize).unwrap();
            let mut message = MSG::default();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            UnregisterClassW(class.as_ptr(), module);
        });
        Self {
            handle: rx.recv().unwrap(),
            thread: Some(worker),
        }
    }

    fn register(&self, registry: &Registry) -> WindowId {
        let id = WindowId {
            hwnd: self.handle,
            pid: std::process::id(),
            cookie: registry.counter.fetch_add(1, Ordering::Relaxed),
        };
        assert_ne!(
            unsafe {
                SetPropW(
                    self.handle as HWND,
                    registry.property.as_ptr(),
                    id.cookie as *mut c_void,
                )
            },
            0
        );
        registry.registered.lock().unwrap().insert(id);
        id
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.handle as HWND, CLOSE_FIXTURE, 0, 0);
        }
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn real_window_bypasses_minimum_clamp_preserves_ratio_and_maximizes() {
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    let original = client_size(fixture.handle as HWND).unwrap();
    // Control: a normal resize is clamped by WM_GETMINMAXINFO.
    unsafe {
        SetWindowPos(
            fixture.handle as HWND,
            null_mut(),
            80,
            80,
            240,
            135,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    assert!(client_size(fixture.handle as HWND).unwrap().width > 240);
    let affected = AtomicBool::new(false);
    let mut baseline = Some(original);
    for width in [240, 481, 240] {
        let actual = registry
            .resize(id, width, &mut baseline, &affected)
            .unwrap();
        assert_eq!(actual, scaled_size(width, original).unwrap());
    }
    assert!(affected.load(Ordering::SeqCst));
    registry.maximize(id).unwrap();
    assert_ne!(unsafe { IsZoomed(fixture.handle as HWND) }, 0);
    let actual = registry.resize(id, 240, &mut baseline, &affected).unwrap();
    assert_eq!(actual, scaled_size(240, original).unwrap());
    registry.cleanup();
    assert_eq!(
        unsafe { GetPropW(fixture.handle as HWND, registry.property.as_ptr()) } as usize,
        0
    );
}

#[test]
fn reused_identity_cannot_receive_an_old_selection_operation() {
    let fixture = Fixture::new();
    let registry = Registry::new();
    let old = fixture.register(&registry);
    assert!(registry.valid(old));
    unsafe {
        RemovePropW(fixture.handle as HWND, registry.property.as_ptr());
    }
    let new = fixture.register(&registry);
    assert!(!registry.valid(old));
    assert!(registry.valid(new));
    assert!(
        registry
            .resize(old, 240, &mut None, &AtomicBool::new(false))
            .is_err()
    );
    assert!(registry.maximize(old).is_err());
    drop(fixture);
    assert!(!registry.valid(new));
}

#[test]
fn minimized_from_maximized_window_can_be_shrunk_on_first_operation() {
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    let maximized = registry.maximize(id).unwrap();
    unsafe {
        ShowWindow(fixture.handle as HWND, SW_MINIMIZE);
    }
    assert_ne!(unsafe { IsIconic(fixture.handle as HWND) }, 0);
    let mut baseline = None;
    let actual = registry
        .resize(id, 240, &mut baseline, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(baseline, Some(maximized));
    assert_eq!(actual, scaled_size(240, maximized).unwrap());
    assert_eq!(unsafe { IsIconic(fixture.handle as HWND) }, 0);
    assert_eq!(unsafe { IsZoomed(fixture.handle as HWND) }, 0);
}

#[test]
fn timeout_does_not_allow_exit_to_overtake_an_unfinished_resize() {
    let fixture = Fixture::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let id = fixture.register(&backend.registry);
    BLOCK_STARTED.store(false, Ordering::SeqCst);
    unsafe {
        PostMessageW(fixture.handle as HWND, BLOCK_FIXTURE, 0, 0);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while !BLOCK_STARTED.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    backend.run(vec![id], Action::Shrink(240), false);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("expected batch")
    };
    assert!(!exiting);
    assert!(outcomes[0].result.as_ref().unwrap_err().contains("超时"));
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("expected exit")
    };
    assert!(exiting);
    assert!(
        outcomes[0]
            .result
            .as_ref()
            .unwrap_err()
            .contains("尚未完成")
    );
    thread::sleep(Duration::from_secs(2));
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("expected retry")
    };
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].result.is_ok());
    assert_ne!(unsafe { IsZoomed(fixture.handle as HWND) }, 0);
}
