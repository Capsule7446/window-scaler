#![windows_subsystem = "windows"]

mod app;
mod backend;
mod layout;
mod model;
mod native;
mod settings;
mod tray;

use gpui_kit::*;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let path = settings::path().with_file_name("last-error.txt");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, info.to_string());
    }));
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::component::set_locale("zh-CN");
            let bounds = Bounds::centered(None, size(px(960.), px(680.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(760.), px(540.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("窗口缩放器".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| app::Scaler::new(window, cx))
            }) {
                native::fatal_message(&format!("无法创建主界面：{error}"));
                cx.quit();
            }
        });
}
