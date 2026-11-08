//! Opt-in UIPI acceptance. Only a disposable child receives a restricted token.
use crate::{
    backend::{Action, Backend, Event},
    fixture::{self, Fixtures},
};
use std::{
    mem::{size_of, size_of_val},
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    System::Threading::*,
    UI::WindowsAndMessaging::{RemovePropW, SetPropW},
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Numeric diagnostics survive even when the restricted child has no console.
pub fn probe(pid: u32) -> u32 {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return 10;
        }
        let mut buffer = [0usize; 32];
        let mut length = 0;
        let ok = GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buffer.as_mut_ptr() as _,
            size_of_val(&buffer) as u32,
            &mut length,
        );
        CloseHandle(token);
        if ok == 0 {
            return 11;
        }
        let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if *GetSidSubAuthority(label.Label.Sid, count as u32 - 1) != 4096 {
            return 12;
        }
    }
    let (tx, rx) = async_channel::unbounded();
    let backend = Backend::new(tx);
    let targets: Vec<_> = backend
        .registry
        .discover()
        .unwrap()
        .into_iter()
        .filter(|window| window.id.pid == pid)
        .collect();
    if targets.len() != 2 {
        return 20;
    }
    if targets.iter().any(|window| window.id.cookie != 0) {
        return 21;
    }
    if targets.iter().any(|window| {
        !window
            .unavailable
            .as_ref()
            .is_some_and(|error| error.contains("权限"))
    }) {
        return 22;
    }
    let property = wide(&format!(
        "WindowScaler.PermissionProbe.{}",
        std::process::id()
    ));
    for target in &targets {
        unsafe {
            let accepted = SetPropW(target.id.hwnd as HWND, property.as_ptr(), 1usize as _);
            let error = GetLastError();
            if accepted != 0 {
                RemovePropW(target.id.hwnd as HWND, property.as_ptr());
                return 21;
            }
            if error != ERROR_ACCESS_DENIED {
                return 25;
            }
        }
    }
    backend.run(
        targets.iter().map(|window| window.id).collect(),
        Action::Shrink(240),
        false,
    );
    let Event::Batch { outcomes, .. } = rx.recv_blocking().unwrap() else {
        return 23;
    };
    if outcomes.len() != 2 || outcomes.iter().any(|outcome| outcome.result.is_ok()) {
        return 24;
    }
    0
}

pub fn run() {
    let fixtures = Fixtures::start();
    let original: Vec<_> = fixtures
        .handles
        .iter()
        .map(|handle| fixture::size(*handle))
        .collect();
    unsafe {
        let mut current = null_mut();
        assert_ne!(
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
                &mut current
            ),
            0
        );
        let mut restricted = null_mut();
        assert_ne!(
            CreateRestrictedToken(
                current,
                DISABLE_MAX_PRIVILEGE,
                0,
                null_mut(),
                0,
                null_mut(),
                0,
                null_mut(),
                &mut restricted
            ),
            0
        );
        CloseHandle(current);
        let mut sid = null_mut();
        // S-1-16-4096 = Low integrity; 0x20 = SE_GROUP_INTEGRITY.
        assert_ne!(
            AllocateAndInitializeSid(
                &SECURITY_MANDATORY_LABEL_AUTHORITY,
                1,
                4096,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut sid
            ),
            0
        );
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: 0x20,
            },
        };
        let changed = SetTokenInformation(
            restricted,
            TokenIntegrityLevel,
            &label as *const _ as _,
            size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid),
        );
        let label_error = GetLastError();
        FreeSid(sid);
        assert_ne!(
            changed, 0,
            "set child token Low before creation: {label_error}"
        );
        let exe = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut command = wide(&format!("\"{exe}\" --permission-probe {}", fixtures.pid));
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut child = PROCESS_INFORMATION::default();
        let created = CreateProcessAsUserW(
            restricted,
            wide(&exe).as_ptr(),
            command.as_mut_ptr(),
            null_mut(),
            null_mut(),
            0,
            CREATE_NO_WINDOW,
            null_mut(),
            null_mut(),
            &startup,
            &mut child,
        );
        let error = GetLastError();
        CloseHandle(restricted);
        assert_ne!(
            created, 0,
            "CreateProcessAsUserW needs token-launch permissions: {error}"
        );
        CloseHandle(child.hThread);
        if WaitForSingleObject(child.hProcess, 15000) != WAIT_OBJECT_0 {
            TerminateProcess(child.hProcess, 99);
            CloseHandle(child.hProcess);
            panic!("restricted probe timed out");
        }
        let mut code = 999;
        assert_ne!(GetExitCodeProcess(child.hProcess, &mut code), 0);
        CloseHandle(child.hProcess);
        println!(
            "Low-at-creation probe={code}; 0=UIPI denied with error 5 and backend rejected resize; 10..12=invalid token; 20=missing targets; 21=SetProp allowed; 22=wrong UI error; 23..25=wrong rejection"
        );
        for (handle, before) in fixtures.handles.iter().zip(original) {
            assert_eq!(fixture::size(*handle), before);
        }
        assert_eq!(code, 0);
    }
}
