use super::*;
use crate::fixture::{self, Fixtures};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

async fn pause(cx: &AsyncApp, milliseconds: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(milliseconds))
        .await;
}

async fn wait(
    view: &Entity<Scaler>,
    cx: &mut AsyncApp,
    condition: impl Fn(&Scaler) -> bool,
    what: &str,
) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        if view.update(cx, |view, _| condition(view)) {
            return Ok(());
        }
        anyhow::ensure!(start.elapsed() < Duration::from_secs(12), "timeout: {what}");
        pause(cx, 50).await;
    }
}

fn menu_exit(view: &Entity<Scaler>, cx: &mut AsyncApp) {
    let handle = view.update(cx, |view, _| view.tray.as_ref().unwrap().window_handle());
    let menu = crate::tray::TEST_MENU.load(Ordering::SeqCst);
    let command = unsafe { GetMenuItemID(menu as _, 1) };
    assert!(command != u32::MAX && command != 0);
    // The same WM_COMMAND produced by choosing the real popup menu's Exit item.
    assert_ne!(
        unsafe { PostMessageW(handle, WM_COMMAND, command as usize, 0) },
        0
    );
}

async fn drive(
    case: &str,
    pid: u32,
    handles: &[usize],
    window: AnyWindowHandle,
    view: &Entity<Scaler>,
    ready: &AtomicBool,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    wait(view, cx, |view| !view.refreshing, "first discovery").await?;
    let hwnd = view.update(cx, |view, cx| {
        view.settings_path = std::env::temp_dir().join(format!(
            "window-scaler-settings-{}.json",
            std::process::id()
        ));
        let ids: Vec<_> = view
            .windows
            .iter()
            .filter(|entry| entry.id.pid == pid)
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids.len(), 2, "real external fixture discovery");
        view.selected.extend(ids);
        cx.notify();
        view.hwnd
    });
    window.update(cx, |_, window, cx| {
        window.draw(cx).clear(cx);
    })?;
    anyhow::ensure!(
        unsafe { IsWindowVisible(hwnd as HWND) } != 0,
        "main window visible"
    );
    if case == "no-tray" {
        view.update(cx, |view, _| {
            assert!(view.tray.is_none());
            assert!(view.tray_error.is_some());
        });
        unsafe {
            PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
        }
        pause(cx, 180).await;
        anyhow::ensure!(
            unsafe { IsWindowVisible(hwnd as HWND) } != 0,
            "tray failure must not hide main window"
        );
        ready.store(true, Ordering::SeqCst);
        view.update(cx, |view, cx| view.begin(Action::Maximize, true, cx));
        return Ok(());
    }
    window.update(cx, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.search.update(cx, |search, cx| {
                search.set_value("Acceptance 0", window, cx)
            });
            view.width
                .update(cx, |width, cx| width.set_value("240", window, cx));
        })
    })?;
    pause(cx, 100).await;
    if case == "visibility" {
        let frames: Vec<_> = handles
            .iter()
            .map(|handle| fixture::frame(*handle))
            .collect();
        view.update(cx, |view, cx| view.begin(Action::Hide, false, cx));
        wait(view, cx, |view| !view.busy, "hide completion").await?;
        for handle in handles {
            assert_eq!(unsafe { IsWindowVisible(*handle as HWND) }, 0);
            assert_eq!(unsafe { IsIconic(*handle as HWND) }, 0);
        }
        view.update(cx, |view, cx| view.refresh(cx));
        wait(view, cx, |view| !view.refreshing, "hidden target discovery").await?;
        view.update(cx, |view, cx| {
            assert_eq!(
                view.selected.len(),
                2,
                "hidden targets remain selectable after refresh"
            );
            assert_eq!(
                hidden_selected(&view.windows, &view.selected, &view.search.read(cx).value()),
                1
            );
            for id in &view.selected {
                assert_eq!(view.results[id], ("已隐藏".into(), false));
            }
            view.begin(Action::Show, false, cx);
        });
        wait(view, cx, |view| !view.busy, "show completion").await?;
        for (handle, before) in handles.iter().zip(&frames) {
            assert_ne!(unsafe { IsWindowVisible(*handle as HWND) }, 0);
            assert_eq!(fixture::frame(*handle), *before);
        }
        // One target is only hidden, the other also needs exit's existing resize recovery.
        view.update(cx, |view, cx| {
            view.selected.retain(|id| id.hwnd == handles[0]);
            view.begin(Action::Shrink(240), false, cx);
        });
        wait(view, cx, |view| !view.busy, "visibility case shrink").await?;
        view.update(cx, |view, cx| {
            view.selected.extend(
                view.windows
                    .iter()
                    .filter(|w| w.id.pid == pid)
                    .map(|w| w.id),
            );
            view.begin(Action::Hide, false, cx);
        });
        wait(view, cx, |view| !view.busy, "hide before exit").await?;
        view.update(cx, |view, cx| {
            view.selected.clear();
            view.handle_event(Event::Open, cx);
        });
        wait(
            view,
            cx,
            |view| !view.refreshing,
            "open with hidden targets",
        )
        .await?;
        view.update(cx, |view, _| {
            assert!(view.selected.is_empty());
            assert_eq!(view.windows.iter().filter(|w| w.id.pid == pid).count(), 2);
        });
        ready.store(true, Ordering::SeqCst);
        menu_exit(view, cx);
        return Ok(());
    }
    if case == "busy-retry" {
        unsafe {
            PostMessageW(handles[0] as HWND, fixture::BLOCK, 0, 0);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !fixture::block_started(handles[0]) {
            anyhow::ensure!(
                Instant::now() < deadline,
                "fixture did not acknowledge blocking"
            );
            pause(cx, 10).await;
        }
    }
    if case == "closed" {
        unsafe {
            PostMessageW(handles[0] as HWND, WM_CLOSE, 0, 0);
        }
        pause(cx, 150).await;
        anyhow::ensure!(
            unsafe { IsWindow(handles[0] as HWND) } == 0,
            "fixture must really be destroyed"
        );
    }
    view.update(cx, |view, cx| {
        assert_eq!(view.selected.len(), 2);
        assert_eq!(
            hidden_selected(&view.windows, &view.selected, &view.search.read(cx).value()),
            1
        );
        view.begin(Action::Shrink(240), false, cx);
    });
    if case == "busy-retry" {
        menu_exit(view, cx);
        pause(cx, 150).await;
        view.update(cx, |view, _| {
            assert!(view.busy);
            assert!(view.exit_requested);
            assert!(view.status.contains("等待当前操作"));
        });
        wait(view, cx, |view| view.exit_failed, "busy exit failure").await?;
        anyhow::ensure!(
            unsafe { IsWindowVisible(hwnd as HWND) } != 0,
            "failure remains visible"
        );
        view.update(cx, |view, _| {
            assert!(
                view.exit_errors
                    .iter()
                    .any(|error| error.contains("尚未完成"))
            )
        });
        pause(cx, 2000).await;
        assert_eq!(
            fixture::size(handles[0]).width,
            240,
            "old shrink must finish before retry"
        );
        ready.store(true, Ordering::SeqCst);
        view.update(cx, |view, cx| view.begin(Action::Maximize, true, cx));
        return Ok(());
    }
    wait(view, cx, |view| !view.busy, "shrink completion").await?;
    if case == "closed" {
        view.update(cx, |view, cx| {
            assert!(
                view.results
                    .values()
                    .any(|(error, failed)| *failed && error.contains("关闭"))
            );
            assert_eq!(
                fixture::size(handles[1]).width,
                240,
                "other target still completes"
            );
            view.refresh(cx);
        });
        wait(view, cx, |view| !view.refreshing, "closed target refresh").await?;
        view.update(cx, |view, _| assert_eq!(view.selected.len(), 1));
        ready.store(true, Ordering::SeqCst);
        menu_exit(view, cx);
        return Ok(());
    }
    for handle in handles {
        anyhow::ensure!(
            fixture::size(*handle).width == 240,
            "hidden selection must also shrink"
        );
    }
    view.update(cx, |view, cx| {
        view.selected.clear();
        cx.notify();
    });
    if case == "force" {
        unsafe {
            PostMessageW(handles[0] as HWND, fixture::REJECT, 0, 0);
        }
        pause(cx, 80).await;
        menu_exit(view, cx);
        wait(view, cx, |view| view.exit_failed, "refused restore failure").await?;
        view.update(cx, |view, _| {
            assert!(!view.exit_errors.is_empty());
            assert!(view.results.values().any(|(_, failed)| *failed));
        });
        ready.store(true, Ordering::SeqCst);
        view.update(cx, |view, cx| view.quit(cx));
        return Ok(());
    }
    let miniature_sizes: Vec<_> = handles
        .iter()
        .map(|handle| fixture::size(*handle))
        .collect();
    unsafe {
        PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
    }
    pause(cx, 180).await;
    anyhow::ensure!(
        unsafe { IsWindowVisible(hwnd as HWND) } == 0,
        "close must hide main window"
    );
    for (handle, size) in handles.iter().zip(&miniature_sizes) {
        assert_eq!(
            fixture::size(*handle),
            *size,
            "hiding must not change target size"
        );
    }
    let tray_handle = view.update(cx, |view, _| {
        let tray = view.tray.as_ref().unwrap();
        assert!(
            tray.rect().is_some(),
            "real shell tray icon must be registered"
        );
        tray.window_handle()
    });
    // tray-icon 0.26.1's native shell callback (dependency is pinned).
    unsafe {
        PostMessageW(tray_handle, 6002, 0, WM_LBUTTONUP as isize);
    }
    wait(
        view,
        cx,
        |view| unsafe { IsWindowVisible(view.hwnd as HWND) != 0 } && !view.refreshing,
        "tray restore",
    )
    .await?;
    for (handle, size) in handles.iter().zip(&miniature_sizes) {
        assert_eq!(
            fixture::size(*handle),
            *size,
            "tray opening must not change target size"
        );
    }
    view.update(cx, |view, _| assert!(view.selected.is_empty()));
    ready.store(true, Ordering::SeqCst);
    menu_exit(view, cx);
    Ok(())
}

pub fn run(case: String) {
    let fixtures = Fixtures::start();
    let original_frames: Vec<_> = fixtures
        .handles
        .iter()
        .map(|handle| fixture::frame(*handle))
        .collect();
    let handles = fixtures.handles.clone();
    let pid = fixtures.pid;
    let ready = Arc::new(AtomicBool::new(false));
    let failure = Arc::new(Mutex::new(None::<String>));
    crate::tray::TEST_FAIL_CREATION.store(case == "no-tray", Ordering::SeqCst);
    let run_case = case.clone();
    let run_ready = ready.clone();
    let run_failure = failure.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::component::set_locale("zh-CN");
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(920.), px(680.)),
                        cx,
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(format!("窗口缩放器验收 · {run_case}").into()),
                        ..Default::default()
                    }),
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Scaler::new(window, cx)),
            )
            .unwrap();
            cx.spawn(async move |cx| {
                let result = drive(&run_case, pid, &handles, window, &view, &run_ready, cx).await;
                if let Err(error) = result {
                    *run_failure.lock().unwrap() = Some(format!("{error:#}"));
                    view.update(cx, |view, cx| view.quit(cx));
                }
            })
            .detach();
        });
    assert!(
        ready.load(Ordering::SeqCst),
        "{}",
        failure
            .lock()
            .unwrap()
            .as_deref()
            .unwrap_or("application exited early")
    );
    assert!(failure.lock().unwrap().is_none());
    for (index, handle) in fixtures.handles.iter().enumerate() {
        if case == "visibility" {
            assert_ne!(
                unsafe { IsWindowVisible(*handle as HWND) },
                0,
                "exit must show every hidden target"
            );
            if index == 1 {
                assert_eq!(
                    fixture::frame(*handle),
                    original_frames[index],
                    "hide-only target must retain its geometry"
                );
            }
        }
        if case == "closed" && index == 0 {
            assert_eq!(unsafe { IsWindow(*handle as HWND) }, 0);
            continue;
        }
        let size = fixture::size(*handle);
        if case == "force" && index == 0 {
            assert_eq!(size.width, 240);
        } else {
            assert!(
                size.width > 240,
                "{case}: fixture {index} left miniature: {size:?}"
            );
        }
        println!("{case} final HWND {handle}: {size:?}");
    }
    let settings = std::env::temp_dir().join(format!(
        "window-scaler-settings-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(settings);
}
