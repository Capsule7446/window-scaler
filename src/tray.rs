use crate::backend::Event;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

// Instrument only test executables; the shipped application has no test hooks.
#[cfg(test)]
pub static TEST_MENU: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
pub static TEST_FAIL_CREATION: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn create(events: async_channel::Sender<Event>) -> anyhow::Result<TrayIcon> {
    #[cfg(test)]
    if TEST_FAIL_CREATION.load(std::sync::atomic::Ordering::SeqCst) {
        anyhow::bail!("injected native tray creation failure");
    }
    let menu = Menu::new();
    let open = MenuItem::new("显示主界面", true, None);
    let exit = MenuItem::new("退出（先显示／放大）", true, None);
    menu.append_items(&[&open, &exit])?;
    #[cfg(test)]
    TEST_MENU.store(
        tray_icon::menu::ContextMenu::hpopupmenu(&menu) as usize,
        std::sync::atomic::Ordering::SeqCst,
    );
    let open_id = open.id().clone();
    let exit_id = exit.id().clone();
    let menu_events = events.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            let _ = menu_events.try_send(Event::Open);
        }
        if event.id == exit_id {
            let _ = menu_events.try_send(Event::Exit);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            let _ = events.try_send(Event::Open);
        }
    }));
    // A small window glyph drawn directly; no external asset is needed at runtime.
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 2..30usize {
        for x in 2..30usize {
            let p = (y * 32 + x) * 4;
            let window = (7..25).contains(&x) && (8..23).contains(&y);
            let border = window && (!(9..23).contains(&x) || !(12..21).contains(&y));
            let mini = (15..23).contains(&x) && (15..21).contains(&y);
            rgba[p..p + 4].copy_from_slice(if border || mini {
                &[255, 255, 255, 255]
            } else {
                &[44, 101, 225, 255]
            });
        }
    }
    let icon = Icon::from_rgba(rgba, 32, 32)?;
    Ok(TrayIconBuilder::new()
        .with_tooltip("窗口缩放器 · 点击打开")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()?)
}
