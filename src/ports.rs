use std::{
    ffi::CString,
    fs, io,
    os::unix::{ffi::OsStrExt, fs::FileTypeExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub path: PathBuf,
    pub label: String,
    pub detail: String,
    pub readable_writable: bool,
}

pub fn scan(extra: Option<&Path>) -> io::Result<Vec<Port>> {
    let sys = Path::new("/sys/class/tty");
    let mut devices = Vec::new();
    for entry in fs::read_dir("/dev")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(key @ (kind, _)) = port_key(name) else {
            continue;
        };
        let tty = sys.join(name);
        if kind == 2 && !active_uart(&tty) {
            continue;
        }
        let path = entry.path();
        if !is_character_device(&path) {
            continue;
        }
        let product = usb_product(&tty).unwrap_or_else(|| match kind {
            0 => "USB CDC serial".into(),
            1 => "USB serial adapter".into(),
            _ => "Hardware UART".into(),
        });
        devices.push((key, new_port(path, product)));
    }
    devices.sort_by_key(|(key, _)| *key);
    let mut ports: Vec<_> = devices.into_iter().map(|(_, port)| port).collect();
    if let Some(extra) = extra
        && let Ok(path) = fs::canonicalize(extra)
        && is_character_device(&path)
        && !ports.iter().any(|port| port.path == path)
    {
        ports.push(new_port(
            path,
            format!("Explicit device {}", extra.display()),
        ));
    }
    Ok(ports)
}

fn new_port(path: PathBuf, detail: String) -> Port {
    let label = path.display().to_string();
    Port {
        label,
        detail,
        readable_writable: can_access(&path),
        path,
    }
}

pub fn is_character_device(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_char_device())
}

pub fn can_access(path: &Path) -> bool {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // A permission check avoids opening the port and changing its modem lines.
    unsafe {
        libc::faccessat(
            libc::AT_FDCWD,
            path.as_ptr(),
            libc::R_OK | libc::W_OK,
            libc::AT_EACCESS,
        ) == 0
    }
}

fn port_key(name: &str) -> Option<(u8, u32)> {
    ["ttyACM", "ttyUSB", "ttyS"]
        .into_iter()
        .enumerate()
        .find_map(|(kind, prefix)| {
            let suffix = name.strip_prefix(prefix)?;
            if !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some((kind as u8, suffix.parse().ok()?))
        })
}

fn active_uart(tty: &Path) -> bool {
    fs::read_to_string(tty.join("type"))
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .is_some_and(|kind| kind != 0)
}

fn usb_product(tty: &Path) -> Option<String> {
    let path = tty.join("device").canonicalize().ok()?;
    path.ancestors().find_map(|parent| {
        let product = fs::read_to_string(parent.join("product")).ok()?;
        let trimmed = product.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_and_sorts_only_numbered_serial_nodes() {
        assert_eq!(port_key("ttyUSB0"), Some((1, 0)));
        assert_eq!(port_key("ttyS3"), Some((2, 3)));
        assert!(port_key("ttyACM2") < port_key("ttyACM10"));
        for name in [
            "tty", "tty1", "ttyUSB", "ttyUSB-1", "ttyACM+2", "ttyS2x", "pts",
        ] {
            assert_eq!(port_key(name), None, "{name}");
        }
    }

    #[test]
    fn hides_unconfigured_uart_placeholders() {
        let tty = tempfile::tempdir().unwrap();
        assert!(!active_uart(tty.path()));
        let path = tty.path().join("type");
        fs::write(&path, "0\n").unwrap();
        assert!(!active_uart(tty.path()));
        fs::write(&path, "4\n").unwrap();
        assert!(active_uart(tty.path()));
    }

    #[test]
    fn requires_character_device() {
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(!is_character_device(file.path()));
        assert!(is_character_device(Path::new("/dev/null")));
    }
}
