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
const REJECT_SHOW: isize = 4;
const REJECT_SHOW_GEOMETRY: isize = 5;
const REJECT_MOVE: isize = 6;
static BLOCK_STARTED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn fixture_proc(
    window: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match message {
        WM_WINDOWPOSCHANGED => unsafe {
            let result = DefWindowProcW(window, message, w, l);
            if GetWindowLongPtrW(window, GWLP_USERDATA) == REJECT_MOVE {
                let mut frame = RECT::default();
                GetWindowRect(window, &mut frame);
                if frame.left != 120 || frame.top != 120 {
                    SetWindowPos(
                        window,
                        null_mut(),
                        120,
                        120,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
                    );
                }
            }
            if GetWindowLongPtrW(window, GWLP_USERDATA) == REJECT_SHOW
                && IsWindowVisible(window) != 0
            {
                ShowWindow(window, SW_HIDE);
            }
            if GetWindowLongPtrW(window, GWLP_USERDATA) == REJECT_SHOW_GEOMETRY
                && IsWindowVisible(window) != 0
            {
                let mut frame = RECT::default();
                GetWindowRect(window, &mut frame);
                if frame.right - frame.left != 900 {
                    SetWindowPos(
                        window,
                        null_mut(),
                        0,
                        0,
                        900,
                        650,
                        SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
                    );
                }
            }
            result
        },
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

fn tiling_fixture(backend: &Backend, width: i32, height: i32) -> (Fixture, WindowId) {
    let fixture = Fixture::new();
    let id = fixture.register(&backend.registry);
    unsafe {
        SetWindowPos(
            hwnd(id),
            null_mut(),
            120,
            120,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING | SWP_SHOWWINDOW,
        );
    }
    (fixture, id)
}

#[test]
fn tiling_accepts_self_shown_windows_but_keeps_failed_show_recovery_pending() {
    use crate::layout::{Corner, Direction, Rect, TilingOptions};
    let _dpi = DpiGuard::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let (_fixture, id) = tiling_fixture(&backend, 240, 160);
    backend.registry.hide(id).unwrap();
    unsafe {
        ShowWindow(hwnd(id), SW_SHOWNOACTIVATE);
    }
    let area = Rect {
        left: 80,
        top: 80,
        right: 680,
        bottom: 580,
    };
    let options = TilingOptions {
        corner: Corner::TopLeft,
        direction: Direction::Horizontal,
        gap: 8,
    };
    backend.tile(vec![id], area, options);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(outcomes[0].result.is_ok(), "{outcomes:?}");
    assert_eq!(window_frame(hwnd(id)).unwrap(), [80, 80, 320, 240]);
    backend.registry.hide(id).unwrap();
    backend.registry.show(id).unwrap();
    assert_eq!(
        window_frame(hwnd(id)).unwrap(),
        [80, 80, 320, 240],
        "new hide must not reuse an obsolete frame"
    );
    backend.registry.hide(id).unwrap();
    unsafe {
        SetWindowLongPtrW(hwnd(id), GWLP_USERDATA, REJECT_SHOW_GEOMETRY);
    }
    assert!(backend.registry.show(id).is_err());
    assert_ne!(unsafe { IsWindowVisible(hwnd(id)) }, 0);
    let pending_frame = window_frame(hwnd(id)).unwrap();
    backend.tile(vec![id], area, options);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(outcomes[0].result.as_ref().unwrap_err().contains("显示"));
    assert_eq!(window_frame(hwnd(id)).unwrap(), pending_frame);
    assert!(backend.registry.needs_show(id));
    unsafe {
        SetWindowLongPtrW(hwnd(id), GWLP_USERDATA, 0);
    }
    backend.registry.show(id).unwrap();
}

#[test]
fn tiling_reports_stale_geometry_and_refused_moves_without_stopping_other_targets() {
    use crate::layout::{Corner, Direction, Position, Rect, TilingOptions};
    let _dpi = DpiGuard::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let (_first, a) = tiling_fixture(&backend, 240, 160);
    let (_second, b) = tiling_fixture(&backend, 240, 160);
    let snapshot = backend.registry.measure_tiling(a).unwrap();
    unsafe {
        SetWindowPos(
            hwnd(a),
            null_mut(),
            180,
            180,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    assert!(
        backend
            .registry
            .move_tiled(a, snapshot, Position { x: 80, y: 80 })
            .unwrap_err()
            .contains("已改变")
    );
    assert_eq!(window_frame(hwnd(a)).unwrap(), [180, 180, 420, 340]);
    unsafe {
        SetWindowPos(
            hwnd(a),
            null_mut(),
            120,
            120,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        );
        SetWindowLongPtrW(hwnd(a), GWLP_USERDATA, REJECT_MOVE);
    }
    backend.tile(
        vec![a, b],
        Rect {
            left: 80,
            top: 80,
            right: 680,
            bottom: 580,
        },
        TilingOptions {
            corner: Corner::TopLeft,
            direction: Direction::Horizontal,
            gap: 8,
        },
    );
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(
        outcomes
            .iter()
            .find(|o| o.id == a)
            .unwrap()
            .result
            .as_ref()
            .unwrap_err()
            .contains("未保持")
    );
    assert!(outcomes.iter().find(|o| o.id == b).unwrap().result.is_ok());
    assert_eq!(window_frame(hwnd(a)).unwrap(), [120, 120, 360, 280]);
    assert_eq!(window_frame(hwnd(b)).unwrap(), [328, 80, 568, 240]);
}

#[test]
fn tiling_timeout_keeps_the_window_reserved_until_the_move_finishes() {
    use crate::layout::{Corner, Direction, Rect, TilingOptions};
    let _dpi = DpiGuard::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let (_first, a) = tiling_fixture(&backend, 240, 160);
    let (_second, b) = tiling_fixture(&backend, 240, 160);
    BLOCK_STARTED.store(false, Ordering::SeqCst);
    unsafe {
        PostMessageW(hwnd(a), BLOCK_FIXTURE, 0, 0);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while !BLOCK_STARTED.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let area = Rect {
        left: 80,
        top: 80,
        right: 680,
        bottom: 580,
    };
    let options = TilingOptions {
        corner: Corner::TopLeft,
        direction: Direction::Horizontal,
        gap: 8,
    };
    backend.tile(vec![a, b], area, options);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(
        outcomes
            .iter()
            .find(|o| o.id == a)
            .unwrap()
            .result
            .as_ref()
            .unwrap_err()
            .contains("超时")
    );
    assert!(outcomes.iter().find(|o| o.id == b).unwrap().result.is_ok());
    backend.tile(vec![a], area, options);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(
        outcomes[0]
            .result
            .as_ref()
            .unwrap_err()
            .contains("尚未完成")
    );
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(exiting && outcomes[0].result.is_err());
    thread::sleep(Duration::from_secs(2));
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(exiting && outcomes.is_empty());
    assert_eq!(window_frame(hwnd(a)).unwrap(), [80, 80, 320, 240]);
}

#[test]
fn tiling_four_corners_and_both_directions_keep_list_order_and_size() {
    use crate::layout::{Corner::*, Direction::*, Rect, TilingOptions};
    let _dpi = DpiGuard::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let fixtures = [
        tiling_fixture(&backend, 280, 160),
        tiling_fixture(&backend, 230, 200),
        tiling_fixture(&backend, 340, 180),
    ];
    let ids: Vec<_> = fixtures.iter().map(|(_, id)| *id).collect();
    let clients: Vec<_> = ids
        .iter()
        .map(|id| client_size(hwnd(*id)).unwrap())
        .collect();
    // Literal examples for a 700 x 500 work area. Unequal windows force wraps.
    for (corner, direction, expected) in [
        (TopLeft, Horizontal, [[100, 100], [390, 100], [100, 310]]),
        (TopRight, Horizontal, [[520, 100], [280, 100], [460, 310]]),
        (BottomLeft, Horizontal, [[100, 440], [390, 400], [100, 210]]),
        (
            BottomRight,
            Horizontal,
            [[520, 440], [280, 400], [460, 210]],
        ),
        (TopLeft, Vertical, [[100, 100], [100, 270], [390, 100]]),
        (TopRight, Vertical, [[520, 100], [570, 270], [170, 100]]),
        (BottomLeft, Vertical, [[100, 440], [100, 230], [390, 420]]),
        (BottomRight, Vertical, [[520, 440], [570, 230], [170, 420]]),
    ] {
        backend.tile(
            ids.clone(),
            Rect {
                left: 100,
                top: 100,
                right: 800,
                bottom: 600,
            },
            TilingOptions {
                corner,
                direction,
                gap: 10,
            },
        );
        let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
            panic!("batch")
        };
        assert!(
            outcomes.iter().all(|o| o.result.is_ok()),
            "{corner:?} {direction:?}: {outcomes:?}"
        );
        for ((id, position), client) in ids.iter().zip(expected).zip(&clients) {
            let frame = window_frame(hwnd(*id)).unwrap();
            assert_eq!([frame[0], frame[1]], position, "{corner:?} {direction:?}");
            assert_eq!(client_size(hwnd(*id)).unwrap(), *client);
        }
    }
}

#[test]
fn tiling_preflight_leaves_every_window_untouched_when_layout_cannot_fit() {
    use crate::layout::{Corner, Direction, Rect, TilingOptions};
    let _dpi = DpiGuard::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let fixtures = [
        tiling_fixture(&backend, 280, 160),
        tiling_fixture(&backend, 230, 200),
    ];
    let ids: Vec<_> = fixtures.iter().map(|(_, id)| *id).collect();
    let before: Vec<_> = ids
        .iter()
        .map(|id| window_frame(hwnd(*id)).unwrap())
        .collect();
    for (area, gap) in [
        (
            Rect {
                left: 0,
                top: 0,
                right: 500,
                bottom: 200,
            },
            10,
        ),
        (
            Rect {
                left: 0,
                top: 0,
                right: 250,
                bottom: 800,
            },
            0,
        ),
        (
            Rect {
                left: 0,
                top: 0,
                right: 600,
                bottom: 400,
            },
            u32::MAX,
        ),
        (
            Rect {
                left: i32::MIN,
                top: 0,
                right: i32::MAX,
                bottom: 400,
            },
            0,
        ),
    ] {
        backend.tile(
            ids.clone(),
            area,
            TilingOptions {
                corner: Corner::TopLeft,
                direction: Direction::Horizontal,
                gap,
            },
        );
        let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
            panic!("batch")
        };
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|o| o.result.is_err()));
        assert_eq!(
            ids.iter()
                .map(|id| window_frame(hwnd(*id)).unwrap())
                .collect::<Vec<_>>(),
            before
        );
    }
    // A rejected batch releases its reservation. Zero gap means touching frames.
    backend.tile(
        ids.clone(),
        Rect {
            left: -600,
            top: 80,
            right: 0,
            bottom: 480,
        },
        TilingOptions {
            corner: Corner::TopLeft,
            direction: Direction::Horizontal,
            gap: 0,
        },
    );
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(outcomes.iter().all(|o| o.result.is_ok()), "{outcomes:?}");
    assert_eq!(window_frame(hwnd(ids[0])).unwrap(), [-600, 80, -320, 240]);
    assert_eq!(window_frame(hwnd(ids[1])).unwrap(), [-320, 80, -90, 280]);
}

#[test]
fn tiling_skips_special_states_without_restoring_or_showing_them() {
    use crate::layout::{Corner, Direction, Rect, TilingOptions};
    let _dpi = DpiGuard::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let fixtures: Vec<_> = (0..5).map(|_| tiling_fixture(&backend, 240, 160)).collect();
    let ids: Vec<_> = fixtures.iter().map(|(_, id)| *id).collect();
    backend.registry.hide(ids[1]).unwrap();
    unsafe {
        ShowWindow(hwnd(ids[2]), SW_MINIMIZE);
        ShowWindow(hwnd(ids[3]), SW_MAXIMIZE);
    }
    unsafe {
        RemovePropW(hwnd(ids[4]), backend.registry.property.as_ptr());
    }
    let before: Vec<_> = ids
        .iter()
        .map(|id| window_frame(hwnd(*id)).unwrap())
        .collect();
    backend.tile(
        ids.clone(),
        Rect {
            left: 80,
            top: 80,
            right: 680,
            bottom: 580,
        },
        TilingOptions {
            corner: Corner::TopLeft,
            direction: Direction::Horizontal,
            gap: 8,
        },
    );
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert_eq!(outcomes.len(), 5);
    assert!(
        outcomes
            .iter()
            .find(|o| o.id == ids[0])
            .unwrap()
            .result
            .is_ok()
    );
    assert!(
        outcomes
            .iter()
            .filter(|o| o.id != ids[0])
            .all(|o| o.result.is_err())
    );
    assert_eq!(window_frame(hwnd(ids[0])).unwrap(), [80, 80, 320, 240]);
    for (id, old) in ids.iter().zip(before).skip(1) {
        assert_eq!(window_frame(hwnd(*id)).unwrap(), old);
    }
    assert_eq!(unsafe { IsWindowVisible(hwnd(ids[1])) }, 0);
    assert_ne!(unsafe { IsIconic(hwnd(ids[2])) }, 0);
    assert_ne!(unsafe { IsZoomed(hwnd(ids[3])) }, 0);
    assert!(backend.registry.needs_show(ids[1]));
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].id, ids[1]);
    assert!(outcomes[0].result.is_ok());
}

#[test]
fn tiling_preserves_sizes_wraps_rows_and_does_not_add_exit_resize() {
    use crate::layout::{Corner, Direction, Rect, TilingOptions};
    let _dpi = DpiGuard::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let fixtures = [Fixture::new(), Fixture::new(), Fixture::new()];
    let sizes = [(280, 160), (230, 200), (340, 180)];
    let ids: Vec<_> = fixtures
        .iter()
        .map(|f| f.register(&backend.registry))
        .collect();
    for (id, (width, height)) in ids.iter().zip(sizes) {
        unsafe {
            SetWindowPos(
                hwnd(*id),
                null_mut(),
                90,
                90,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING | SWP_SHOWWINDOW,
            );
        }
    }
    let clients: Vec<_> = ids
        .iter()
        .map(|id| client_size(hwnd(*id)).unwrap())
        .collect();
    backend.tile(
        ids.clone(),
        Rect {
            left: 80,
            top: 80,
            right: 680,
            bottom: 580,
        },
        TilingOptions {
            corner: Corner::TopLeft,
            direction: Direction::Horizontal,
            gap: 10,
        },
    );
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(!exiting);
    assert_eq!(outcomes.len(), 3);
    assert!(outcomes.iter().all(|o| o.result.is_ok()), "{outcomes:?}");
    for (id, expected) in
        ids.iter()
            .zip([[80, 80, 360, 240], [370, 80, 600, 280], [80, 290, 420, 470]])
    {
        assert_eq!(window_frame(hwnd(*id)).unwrap(), expected);
    }
    for (id, before) in ids.iter().zip(clients) {
        assert_eq!(client_size(hwnd(*id)).unwrap(), before);
    }
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("batch")
    };
    assert!(exiting && outcomes.is_empty());
    assert_eq!(window_frame(hwnd(ids[0])).unwrap(), [80, 80, 360, 240]);
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
    let miniature = outcomes[0].result.as_ref().unwrap().size().unwrap();
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
    let miniature = outcomes[0].result.as_ref().unwrap().size().unwrap();
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

#[test]
fn failed_visible_show_can_retry_the_hidden_geometry() {
    let _dpi = DpiGuard::new();
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    unsafe { ShowWindow(hwnd(id), SW_SHOWNOACTIVATE) };
    let original = window_frame(hwnd(id)).unwrap();
    registry.hide(id).unwrap();
    unsafe { SetWindowLongPtrW(hwnd(id), GWLP_USERDATA, REJECT_SHOW_GEOMETRY) };
    assert!(registry.show(id).is_err());
    assert_ne!(unsafe { IsWindowVisible(hwnd(id)) }, 0);
    assert_ne!(window_frame(hwnd(id)).unwrap(), original);
    unsafe { SetWindowLongPtrW(hwnd(id), GWLP_USERDATA, 0) };
    registry.show(id).unwrap();
    assert_eq!(window_frame(hwnd(id)).unwrap(), original);
}

#[test]
fn show_keeps_a_targets_current_geometry_after_it_has_shown_itself() {
    let _dpi = DpiGuard::new();
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    unsafe { ShowWindow(hwnd(id), SW_SHOWNOACTIVATE) };
    registry.hide(id).unwrap();
    unsafe {
        assert_ne!(
            SetWindowPos(
                hwnd(id),
                null_mut(),
                120,
                140,
                850,
                600,
                SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING
            ),
            0
        );
    }
    let current = window_frame(hwnd(id)).unwrap();
    registry.show(id).unwrap();
    assert_eq!(window_frame(hwnd(id)).unwrap(), current);
    assert!(!registry.needs_show(id));
}

#[test]
fn hidden_identity_cannot_transfer_its_show_or_exit_record() {
    let fixture = Fixture::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let old = fixture.register(&backend.registry);
    unsafe { ShowWindow(hwnd(old), SW_SHOWNOACTIVATE) };
    backend.run(vec![old], Action::Hide, false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("hide")
    };
    assert!(outcomes[0].result.is_ok());
    let new = fixture.register(&backend.registry);
    assert!(!backend.registry.valid(old));
    assert!(backend.registry.valid(new));
    assert!(backend.registry.show(old).is_err());
    assert!(
        backend.registry.show(new).is_err(),
        "a new identity has no recovery record"
    );
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("exit")
    };
    assert!(outcomes.is_empty());
    assert_eq!(unsafe { IsWindowVisible(hwnd(new)) }, 0);
}

#[test]
fn rejected_show_remains_an_exit_failure_until_a_successful_retry() {
    let fixture = Fixture::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let id = fixture.register(&backend.registry);
    unsafe { ShowWindow(hwnd(id), SW_SHOWNOACTIVATE) };
    backend.run(vec![id], Action::Hide, false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("hide")
    };
    assert!(outcomes[0].result.is_ok());
    unsafe { SetWindowLongPtrW(hwnd(id), GWLP_USERDATA, REJECT_SHOW) };
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("exit")
    };
    assert!(exiting);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].result.as_ref().unwrap_err().contains("显示"));
    assert_eq!(unsafe { IsWindowVisible(hwnd(id)) }, 0);
    unsafe { SetWindowLongPtrW(hwnd(id), GWLP_USERDATA, 0) };
    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("retry")
    };
    assert_eq!(outcomes[0].result.as_ref().unwrap(), &AppliedChange::Shown);
    assert_ne!(unsafe { IsWindowVisible(hwnd(id)) }, 0);
    assert_eq!(unsafe { IsZoomed(hwnd(id)) }, 0);
}

#[test]
fn pending_visibility_request_blocks_exit_until_its_result_is_confirmed() {
    for action in [Action::Hide, Action::Show] {
        let fixture = Fixture::new();
        let (tx, rx) = async_channel::unbounded();
        let backend = Backend::new(tx);
        let id = fixture.register(&backend.registry);
        unsafe { ShowWindow(hwnd(id), SW_SHOWNOACTIVATE) };
        if matches!(action, Action::Show) {
            backend.run(vec![id], Action::Hide, false);
            let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
                panic!("setup hide")
            };
            assert!(outcomes[0].result.is_ok());
        }
        BLOCK_STARTED.store(false, Ordering::SeqCst);
        unsafe { PostMessageW(hwnd(id), BLOCK_FIXTURE, 0, 0) };
        let deadline = Instant::now() + Duration::from_secs(2);
        while !BLOCK_STARTED.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        backend.run(vec![id], action, false);
        let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
            panic!("visibility request")
        };
        assert!(outcomes[0].result.as_ref().unwrap_err().contains("超时"));
        backend.run(Vec::new(), Action::Maximize, true);
        let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
            panic!("pending exit")
        };
        assert_eq!(outcomes.len(), 1);
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
            panic!("retry exit")
        };
        assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
        assert_ne!(unsafe { IsWindowVisible(hwnd(id)) }, 0);
        assert_eq!(unsafe { IsZoomed(hwnd(id)) }, 0);
    }
}

#[test]
fn repeated_hide_keeps_the_first_maximized_or_minimized_restore_state() {
    let _dpi = DpiGuard::new();
    for command in [SW_MAXIMIZE, SW_MINIMIZE] {
        let fixture = Fixture::new();
        let registry = Registry::new();
        let id = fixture.register(&registry);
        unsafe { ShowWindow(hwnd(id), command) };
        let frame = window_frame(hwnd(id)).unwrap();
        let state = unsafe { (IsIconic(hwnd(id)), IsZoomed(hwnd(id))) };
        registry.hide(id).unwrap();
        // A target can show or restore itself before the user hides it again.
        unsafe { ShowWindow(hwnd(id), SW_RESTORE) };
        registry.hide(id).unwrap();
        registry.show(id).unwrap();
        assert_eq!(unsafe { (IsIconic(hwnd(id)), IsZoomed(hwnd(id))) }, state);
        assert_eq!(window_frame(hwnd(id)).unwrap(), frame);
    }
}

#[test]
fn exit_shows_hidden_targets_and_only_maximizes_shrunken_ones() {
    let _dpi = DpiGuard::new();
    let small = Fixture::new();
    let untouched = Fixture::new();
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let ids = [
        small.register(&backend.registry),
        untouched.register(&backend.registry),
    ];
    for id in ids {
        unsafe { ShowWindow(hwnd(id), SW_SHOWNOACTIVATE) };
    }
    let untouched_frame = window_frame(untouched.handle as HWND).unwrap();
    backend.run(vec![ids[0]], Action::Shrink(240), false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("shrink batch")
    };
    assert!(outcomes[0].result.is_ok());
    backend.run(ids.to_vec(), Action::Hide, false);
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        panic!("hide batch")
    };
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
    for id in ids {
        assert_eq!(unsafe { IsWindowVisible(hwnd(id)) }, 0);
    }

    backend.run(Vec::new(), Action::Maximize, true);
    let Event::Batch { outcomes, exiting } = rx.recv_blocking().unwrap() else {
        panic!("exit batch")
    };
    assert!(exiting);
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
    for id in ids {
        assert_ne!(unsafe { IsWindowVisible(hwnd(id)) }, 0);
    }
    assert_ne!(unsafe { IsZoomed(small.handle as HWND) }, 0);
    assert_eq!(unsafe { IsZoomed(untouched.handle as HWND) }, 0);
    assert_eq!(
        window_frame(untouched.handle as HWND).unwrap(),
        untouched_frame
    );
}

#[test]
fn hide_show_preserves_shrunken_geometry_without_minimizing() {
    let _dpi = DpiGuard::new();
    let fixture = Fixture::new();
    let registry = Registry::new();
    let id = fixture.register(&registry);
    unsafe { ShowWindow(fixture.handle as HWND, SW_SHOWNOACTIVATE) };
    registry
        .resize(id, 240, &mut None, &AtomicBool::new(false))
        .unwrap();
    let before = window_geometry(fixture.handle as HWND).unwrap();

    registry.hide(id).unwrap();
    registry.hide(id).unwrap();
    assert_eq!(unsafe { IsWindowVisible(fixture.handle as HWND) }, 0);
    assert_eq!(unsafe { IsIconic(fixture.handle as HWND) }, 0);

    registry.show(id).unwrap();
    registry.show(id).unwrap();
    assert_ne!(unsafe { IsWindowVisible(fixture.handle as HWND) }, 0);
    assert_eq!(unsafe { IsIconic(fixture.handle as HWND) }, 0);
    let after = window_geometry(fixture.handle as HWND).unwrap();
    assert_eq!(after.frame, before.frame);
    assert_eq!(after.client, before.client);
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
        let actual = outcome.result.unwrap().size().unwrap();
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
