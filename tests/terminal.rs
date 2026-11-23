use std::{
    ffi::CStr,
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    process::{Child, Command},
    ptr, thread,
    time::{Duration, Instant},
};

const PICKER: &str = env!("CARGO_BIN_EXE_wsl-com-picker");

struct Pty {
    master: File,
    slave: File,
    path: String,
}

impl Pty {
    fn open() -> Self {
        let mut master = -1;
        let mut slave = -1;
        let dimensions = libc::winsize {
            ws_row: 24,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // openpty owns the output descriptors on success. Leaving name null avoids
        // using its unsized pathname buffer.
        let result = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                ptr::null_mut(),
                ptr::null(),
                &dimensions,
            )
        };
        assert_eq!(result, 0, "openpty: {}", io::Error::last_os_error());
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        for file in [&master, &slave] {
            assert_eq!(
                unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
                0,
                "FD_CLOEXEC: {}",
                io::Error::last_os_error()
            );
        }
        let mut path = [0; 256];
        let result = unsafe { libc::ttyname_r(slave.as_raw_fd(), path.as_mut_ptr(), path.len()) };
        assert_eq!(
            result,
            0,
            "ttyname_r: {}",
            io::Error::from_raw_os_error(result)
        );
        let path = unsafe { CStr::from_ptr(path.as_ptr()) }
            .to_str()
            .unwrap()
            .to_owned();
        Self {
            master,
            slave,
            path,
        }
    }

    fn spawn_picker(&self, command: &mut Command) -> PickerProcess {
        command
            .env("TERM", "xterm")
            .env("LC_ALL", "C.UTF-8")
            .stdin(self.slave.try_clone().unwrap())
            .stdout(self.slave.try_clone().unwrap())
            .stderr(self.slave.try_clone().unwrap());
        // Minicom's keyboard handler requires a controlling terminal.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        PickerProcess(command.spawn().unwrap())
    }

    fn wait_for(&mut self, expected: &str) {
        let mut collected = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut chunk = [0; 8192];
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let mut fd = libc::pollfd {
                fd: self.master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut fd, 1, remaining.as_millis() as i32) };
            if ready == 0 {
                break;
            }
            if ready < 0 {
                let error = io::Error::last_os_error();
                assert_eq!(error.kind(), io::ErrorKind::Interrupted, "{error}");
                continue;
            }
            let count = self.master.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            collected.extend_from_slice(&chunk[..count]);
            if collected
                .windows(expected.len())
                .any(|window| window == expected.as_bytes())
            {
                return;
            }
        }
        let tail = &collected[collected.len().saturating_sub(3000)..];
        panic!(
            "Did not see {expected:?}. Terminal output ended with {:?}",
            String::from_utf8_lossy(tail)
        );
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }
}

struct PickerProcess(Child);

impl PickerProcess {
    fn expect_success(mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "{status}");
                break;
            }
            assert!(Instant::now() < deadline, "picker did not quit");
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for PickerProcess {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // The test starts the picker in a dedicated session and process group.
            unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
            let _ = self.0.wait();
        }
    }
}

#[test]
fn attaches_selected_com_port_and_refreshes_its_state() {
    let mut terminal = Pty::open();
    let serial = Pty::open();
    let fixture = tempfile::tempdir().unwrap();
    let usbipd = fixture.path().join("usbipd.exe");
    let trace = fixture.path().join("attached");
    fs::write(
        &usbipd,
        r#"#!/bin/sh
case "$*" in
    state)
        client=null
        [ ! -f "$MOCK_USBIPD_TRACE" ] || client='"127.0.0.1"'
        printf '{"Devices":[{"BusId":"7-3","Description":"Debug probe (COM9)","InstanceId":"fixture","PersistedGuid":"fixture","ClientIPAddress":%s}]}\n' "$client"
        ;;
    'attach --wsl --busid 7-3')
        printf '%s\n' "$*" > "$MOCK_USBIPD_TRACE"
        ;;
    *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&usbipd, fs::Permissions::from_mode(0o700)).unwrap();
    let child = terminal.spawn_picker(
        Command::new(PICKER)
            .args(["--device", &serial.path])
            .env("PATH", fixture.path())
            .env("MOCK_USBIPD_TRACE", &trace)
            .env("WSL_DISTRO_NAME", "Test"),
    );

    terminal.wait_for("Enter Open");
    terminal.send(b"j");
    terminal.wait_for("Attach");
    terminal.send(b"\r");
    terminal.wait_for("Find port");
    assert_eq!(
        fs::read_to_string(trace).unwrap(),
        "attach --wsl --busid 7-3\n"
    );

    terminal.send(b"q");
    child.expect_success();
}

#[test]
#[ignore = "requires installed Minicom 2.9+ and xterm terminal data"]
fn opens_real_minicom_transfers_bytes_and_returns_to_picker() {
    let mut terminal = Pty::open();
    let mut serial = Pty::open();
    let test_home = tempfile::tempdir().unwrap();
    let mut picker = Command::new(PICKER);
    picker
        .args(["--device", &serial.path])
        .env("HOME", test_home.path())
        .env_remove("WSL_DISTRO_NAME");
    let child = terminal.spawn_picker(&mut picker);

    terminal.wait_for("Enter Open");
    terminal.send(b"\r");
    terminal.wait_for("Welcome to minicom");
    let mut settings = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(serial.slave.as_raw_fd(), settings.as_mut_ptr()) },
        0
    );
    let settings = unsafe { settings.assume_init() };
    assert_eq!(unsafe { libc::cfgetispeed(&settings) }, libc::B115200);
    assert_eq!(
        settings.c_cflag & (libc::CRTSCTS | libc::PARENB | libc::CSTOPB),
        0
    );
    assert_eq!(settings.c_cflag & libc::CSIZE, libc::CS8);
    assert_eq!(settings.c_iflag & (libc::IXON | libc::IXOFF), 0);

    serial.send(b"\r\nserial-probe\r\n");
    terminal.wait_for("serial-probe");
    terminal.send(b"picker-probe");
    serial.wait_for("picker-probe");

    terminal.send(b"\x01x");
    terminal.wait_for("Leave Minicom?");
    terminal.send(b"\r");
    terminal.wait_for("Closed");
    terminal.send(b"q");

    child.expect_success();
}
