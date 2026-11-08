//! Opt-in tests against real GPUI, Win32 windows and the installed shell tray.
//! Each case runs in a bounded child. No keyboard/mouse input reaches other apps.
#![allow(dead_code, unused_imports)]

mod app;
#[path = "../../src/backend.rs"]
mod backend;
mod fixture;
#[path = "../../src/model.rs"]
mod model;
#[path = "../../src/native.rs"]
mod native;
mod permissions;
#[path = "../../src/settings.rs"]
mod settings;
#[path = "../../src/tray.rs"]
mod tray;

use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--fixture") {
        fixture::serve(&args[2]);
    } else if args.get(1).is_some_and(|arg| arg == "--permission-probe") {
        let code = permissions::probe(args[2].parse().unwrap());
        std::process::exit(code as i32);
    } else if args.get(1).is_some_and(|arg| arg == "--case") {
        if args[2] == "permissions" {
            permissions::run();
        } else {
            app::driver::run(args[2].clone());
        }
    } else {
        for case in [
            "normal",
            "busy-retry",
            "force",
            "no-tray",
            "closed",
            "visibility",
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--case", case])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let start = Instant::now();
            loop {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                if start.elapsed() > Duration::from_secs(35) {
                    child.kill().unwrap();
                    let output = child.wait_with_output().unwrap();
                    panic!(
                        "{case} timed out: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
            let output = child.wait_with_output().unwrap();
            println!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(output.status.success(), "{case}: {}", output.status);
            println!("PASS {case} ({:.2}s)", start.elapsed().as_secs_f32());
        }
    }
}
