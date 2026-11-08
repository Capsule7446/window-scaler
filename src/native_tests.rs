use super::*;
use crate::backend::{Action, AppliedChange, Backend, Event};
use std::{sync::mpsc, thread, time::Instant};
use windows_sys::Win32::{
    Foundation::{LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
};

const CLOSE_FIXTURE: u32 = WM_APP + 47;
const BLOCK_FIXTURE: u32 = WM_APP + 48;
const CLAMP_CURRENT_MAX: isize = 1;
const CUSTOM_MAX: isize = 2;
const CUSTOM_SYSTEM_COMMAND: isize = 3;
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
                let mode = GetWindowLongPtrW(window, GWLP_USERDATA);
                if matches!(mode, CLAMP_CURRENT_MAX | CUSTOM_SYSTEM_COMMAND) {
                    // A window may accept WS_MAXIMIZE while restricting its
                    // maximized dimensions to the current miniature frame.
                    let mut frame = RECT::default();
                    GetWindowRect(window, &mut frame);
                    info.ptMinTrackSize.x = 1;
                    info.ptMinTrackSize.y = 1;
                    info.ptMaxSize.x = frame.right - frame.left;
                    info.ptMaxSize.y = frame.bottom - frame.top;
                } else if mode == CUSTOM_MAX {
                    info.ptMinTrackSize.x = 1;
                    info.ptMinTrackSize.y = 1;
                    info.ptMaxSize.x = 600;
                    info.ptMaxSize.y = 400;
                } else {
                    info.ptMinTrackSize.x = 800;
                    info.ptMinTrackSize.y = 450;
                }
            }
            0
        }
        WM_SYSCOMMAND => unsafe {
            if w & 0xfff0 == SC_MAXIMIZE as usize
                && GetWindowLongPtrW(window, GWLP_USERDATA) == CUSTOM_SYSTEM_COMMAND
            {
                SetWindowLongPtrW(window, GWLP_USERDATA, CUSTOM_MAX);
            }
            DefWindowProcW(window, message, w, l)
        },
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

#[test]
fn exit_restores_original_size_when_system_maximize_stays_shrunken() {
    let fixture = Fixture::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let id = fixture.register(&backend.registry);
    let original = client_size(fixture.handle as HWND).unwrap();
    backend.run(vec![id], Action::Shrink(240), false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("expected shrink result")
    };
    let miniature = outcomes[0].result.as_ref().unwrap().size();
    assert_eq!(miniature.width, 240);
    unsafe { SetWindowLongPtrW(fixture.handle as HWND, GWLP_USERDATA, CLAMP_CURRENT_MAX) };

    // Exit restores all affected windows even when the current selection is empty.
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("expected exit result")
    };
    assert!(exiting);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        *outcomes[0].result.as_ref().unwrap(),
        AppliedChange::RestoredOriginal(original)
    );
    assert_eq!(client_size(fixture.handle as HWND).unwrap(), original);
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("expected retry result")
    };
    assert_eq!(
        *outcomes[0].result.as_ref().unwrap(),
        AppliedChange::RestoredOriginal(original)
    );
    assert_eq!(client_size(fixture.handle as HWND).unwrap(), original);
}

#[test]
fn exit_cannot_skip_a_pending_maximize_on_a_never_shrunken_window() {
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
    backend.run(vec![id], Action::Maximize, false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("maximize batch")
    };
    assert!(outcomes[0].result.as_ref().unwrap_err().contains("超时"));
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("exit batch")
    };
    assert!(exiting);
    assert_eq!(
        outcomes.len(),
        1,
        "pending maximize must prevent a successful empty exit"
    );
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
        panic!("retry batch")
    };
    assert!(
        outcomes.is_empty(),
        "completed maximize-only target needs no exit operation"
    );
    assert_ne!(unsafe { IsZoomed(fixture.handle as HWND) }, 0);
}

#[test]
fn maximize_does_not_trust_a_flag_without_larger_geometry() {
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    let miniature = registry
        .resize(id, 240, &mut None, &AtomicBool::new(false))
        .unwrap();
    unsafe { SetWindowLongPtrW(fixture.handle as HWND, GWLP_USERDATA, CLAMP_CURRENT_MAX) };
    assert!(registry.maximize(id).is_err());
    assert!(registry.maximize(id).is_err());
    assert_eq!(client_size(fixture.handle as HWND).unwrap(), miniature);
}

#[test]
fn repeated_custom_maximize_does_not_restore_historical_size() {
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    let original = client_size(fixture.handle as HWND).unwrap();
    registry
        .resize(id, 240, &mut None, &AtomicBool::new(false))
        .unwrap();
    unsafe { SetWindowLongPtrW(fixture.handle as HWND, GWLP_USERDATA, CUSTOM_MAX) };
    let maximized = registry.maximize(id).unwrap();
    assert!(maximized.width > 240 && maximized.width < original.width);
    assert_eq!(registry.maximize(id).unwrap(), maximized);
    assert_ne!(unsafe { IsZoomed(fixture.handle as HWND) }, 0);
}

#[test]
fn maximize_dispatches_the_targets_title_bar_command() {
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    let miniature = registry
        .resize(id, 240, &mut None, &AtomicBool::new(false))
        .unwrap();
    unsafe {
        SetWindowLongPtrW(fixture.handle as HWND, GWLP_USERDATA, CUSTOM_SYSTEM_COMMAND);
        ShowWindow(fixture.handle as HWND, SW_MAXIMIZE);
    }
    assert_ne!(unsafe { IsZoomed(fixture.handle as HWND) }, 0);
    assert_eq!(client_size(fixture.handle as HWND).unwrap(), miniature);
    let maximized = registry.maximize(id).unwrap();
    assert!(maximized.width > miniature.width && maximized.height > miniature.height);
    assert_eq!(registry.maximize(id).unwrap(), maximized);
}

#[test]
fn maximize_handles_minimized_windows_with_or_without_a_maximized_restore_state() {
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    unsafe { ShowWindow(fixture.handle as HWND, SW_MINIMIZE) };
    let maximized = registry.maximize(id).unwrap();
    assert_ne!(unsafe { IsZoomed(fixture.handle as HWND) }, 0);
    unsafe { ShowWindow(fixture.handle as HWND, SW_MINIMIZE) };
    assert_eq!(registry.maximize(id).unwrap(), maximized);
    assert_eq!(unsafe { IsIconic(fixture.handle as HWND) }, 0);
}

#[test]
fn failed_maximize_without_original_size_does_not_invent_a_restore_size() {
    let fixture = Fixture::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let id = fixture.register(&backend.registry);
    let original = client_size(fixture.handle as HWND).unwrap();
    unsafe {
        SetWindowLongPtrW(fixture.handle as HWND, GWLP_USERDATA, CLAMP_CURRENT_MAX);
    }
    backend.run(vec![id], Action::Maximize, false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("expected maximize result")
    };
    assert!(
        outcomes[0]
            .result
            .as_ref()
            .unwrap_err()
            .contains("没有原尺寸记录")
    );
    assert_eq!(client_size(fixture.handle as HWND).unwrap(), original);
}

#[test]
fn rejected_first_resize_does_not_freeze_an_unused_original_size() {
    let fixture = Fixture::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let id = fixture.register(&backend.registry);
    backend.run(vec![id], Action::Shrink(20000), false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(outcomes[0].result.is_err());
    unsafe {
        SetWindowPos(
            fixture.handle as HWND,
            null_mut(),
            80,
            80,
            850,
            600,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
        );
    }
    let original = client_size(fixture.handle as HWND).unwrap();
    backend.run(vec![id], Action::Shrink(240), false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(outcomes[0].result.is_ok());
    unsafe {
        SetWindowLongPtrW(fixture.handle as HWND, GWLP_USERDATA, CLAMP_CURRENT_MAX);
    }
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert_eq!(
        outcomes[0].result.as_ref().unwrap(),
        &AppliedChange::RestoredOriginal(original)
    );
}

#[test]
fn exit_reports_failure_when_original_size_no_longer_fits_the_work_area() {
    let fixture = Fixture::new();
    unsafe {
        assert_ne!(
            SetWindowPos(
                fixture.handle as HWND,
                null_mut(),
                80,
                80,
                20000,
                10000,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING
            ),
            0
        );
    }
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let id = fixture.register(&backend.registry);
    backend.run(vec![id], Action::Shrink(240), false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("expected shrink result")
    };
    let miniature = outcomes[0].result.as_ref().unwrap().size();
    unsafe { SetWindowLongPtrW(fixture.handle as HWND, GWLP_USERDATA, CLAMP_CURRENT_MAX) };
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("expected exit result")
    };
    assert!(exiting);
    let error = outcomes[0].result.as_ref().unwrap_err();
    assert!(error.contains("恢复原尺寸失败") && error.contains("工作区"));
    assert_eq!(client_size(fixture.handle as HWND).unwrap(), miniature);
}

/// Opt-in only: the caller must supply the exact live game process IDs. A
/// documented 1280x720 recovery size is used because old runs did not retain
/// their originals. This is not presented as a recovered historical size.
#[test]
#[ignore = "changes live game windows; requires WINDOW_SCALER_GAME_PIDS"]
fn live_game_restore_acceptance() {
    let pids: Vec<u32> = std::env::var("WINDOW_SCALER_GAME_PIDS")
        .expect("explicit game PIDs required")
        .split(',')
        .map(|pid| pid.parse().unwrap())
        .collect();
    assert!(!pids.is_empty());
    let setup = Registry::new();
    let targets: Vec<_> = setup
        .discover()
        .unwrap()
        .into_iter()
        .filter(|window| pids.contains(&window.id.pid))
        .collect();
    assert_eq!(
        targets.len(),
        pids.len(),
        "one ordinary window per game PID"
    );
    for window in &targets {
        assert!(window.unavailable.is_none(), "{window:?}");
        assert!(
            window.program.eq_ignore_ascii_case("LaTaleClient_x64.exe"),
            "unexpected target: {window:?}"
        );
        let recovery = Size {
            width: 1280,
            height: 720,
        };
        let actual = setup
            .restore_original(window.id, recovery, &AtomicBool::new(false))
            .unwrap();
        eprintln!(
            "recovery PID {} HWND {} -> {actual:?}",
            window.id.pid, window.id.hwnd
        );
    }
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let ids: Vec<_> = backend
        .registry
        .discover()
        .unwrap()
        .into_iter()
        .filter(|window| pids.contains(&window.id.pid))
        .map(|window| window.id)
        .collect();
    backend.run(ids.clone(), Action::Shrink(240), false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("shrink batch")
    };
    // Always request restoration before asserting so a failed check cannot
    // leave the user's live windows miniature.
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch {
        outcomes: restored,
        exiting,
    } = rx.recv_blocking().unwrap()
    else {
        panic!("exit batch")
    };
    assert!(exiting);
    for outcome in &restored {
        eprintln!(
            "exit PID {}: {:?}; actual {:?}",
            outcome.id.pid,
            outcome.result,
            client_size(hwnd(outcome.id))
        );
    }
    assert_eq!(restored.len(), ids.len());
    for outcome in restored {
        let actual = outcome.result.unwrap().size();
        assert!(actual.width >= 1280 && actual.height >= 720);
        assert_eq!(client_size(hwnd(outcome.id)).unwrap(), actual);
    }
    for outcome in outcomes {
        assert_eq!(
            outcome.result.unwrap(),
            AppliedChange::Shrunk(Size {
                width: 240,
                height: 135
            })
        );
    }
}
