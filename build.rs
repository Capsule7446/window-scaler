use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = out.join("app.ico");
    // GPUI already embeds its PerMonitorV2/asInvoker manifest. Only add our icon;
    // a second manifest with resource ID 1 causes CVTRES duplicate-resource errors.
    let mut bytes = Vec::new();
    for n in [0u16, 1, 1] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend([32, 32, 0, 0]);
    for n in [1u16, 32] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend(4264u32.to_le_bytes());
    bytes.extend(22u32.to_le_bytes());
    for n in [40u32, 32, 64] {
        bytes.extend(n.to_le_bytes());
    }
    for n in [1u16, 32] {
        bytes.extend(n.to_le_bytes());
    }
    for n in [0u32, 4096, 0, 0, 0, 0] {
        bytes.extend(n.to_le_bytes());
    }
    for y in (0..32).rev() {
        for x in 0..32 {
            let visible = (2..30).contains(&x) && (2..30).contains(&y);
            let border = (7..25).contains(&x)
                && (8..23).contains(&y)
                && (!(9..23).contains(&x) || !(12..21).contains(&y));
            let mini = (15..23).contains(&x) && (15..21).contains(&y);
            bytes.extend(if !visible {
                [0, 0, 0, 0]
            } else if border || mini {
                [255, 255, 255, 255]
            } else {
                [225, 101, 44, 255]
            });
        }
    }
    bytes.extend([0u8; 128]);
    fs::write(&icon, bytes).unwrap();
    let escaped = |path: &std::path::Path| path.to_string_lossy().replace('\\', "\\\\");
    let source = out.join("app.rc");
    let resource = out.join("app.res");
    fs::write(
        &source,
        format!("#pragma code_page(65001)\n1 ICON \"{}\"\n", escaped(&icon)),
    )
    .unwrap();
    let status = Command::new("rc.exe")
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&source)
        .status()
        .expect("无法运行rc.exe，请使用build.ps1加载Windows SDK环境");
    assert!(status.success(), "Windows资源编译失败");
    println!("cargo:rustc-link-arg={}", resource.display());
}
