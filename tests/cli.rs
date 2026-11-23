use std::{fs, os::unix::fs::PermissionsExt, process::Command};

const PICKER: &str = env!("CARGO_BIN_EXE_wsl-com-picker");

#[test]
fn lists_windows_ports_without_starting_minicom() {
    let bin = tempfile::tempdir().unwrap();
    let usbipd = bin.path().join("usbipd.exe");
    fs::write(
        &usbipd,
        "#!/bin/sh\n\
         [ \"$1\" = state ] || exit 1\n\
         printf '%s\\n' '{\"Devices\":[{\"BusId\":\"7-3\",\"Description\":\"Debug probe (COM9)\",\"InstanceId\":\"USB\\\\VID_2E8A&PID_000C\\\\test\",\"PersistedGuid\":\"fixture\"}]}'\n",
    )
    .unwrap();
    fs::set_permissions(&usbipd, fs::Permissions::from_mode(0o700)).unwrap();

    let output = Command::new(PICKER)
        .arg("--list")
        .env("PATH", bin.path())
        .env("WSL_DISTRO_NAME", "Test")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("COM9\tReady to attach\tUSB 7-3"));
}

#[test]
fn reports_missing_interop_and_keeps_list_mode_available() {
    let empty = tempfile::tempdir().unwrap();
    let output = Command::new(PICKER)
        .arg("--list")
        .env("PATH", empty.path())
        .env("WSL_DISTRO_NAME", "Test")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("needs usbipd-win 5+"));
}

#[test]
fn picker_requires_a_terminal() {
    let output = Command::new(PICKER)
        .env_remove("WSL_DISTRO_NAME")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive WSL terminal"));
}
