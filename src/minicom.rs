use std::{
    fs, io,
    path::Path,
    process::{Command, ExitStatus, Stdio},
};

const PROFILE_NAME: &str = "wsl-com-picker";
const PROFILE: &str = "\
pu bits 8
pu parity N
pu stopbits 1
pu rtscts No
pu xonxoff No
pu minit
pu mreset
pu mhangup
";

pub fn launch(device: &Path, baud: u32) -> Result<ExitStatus, String> {
    let version = Command::new("minicom")
        .arg("--version")
        .env_remove("MINICOM")
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| format!("Cannot run Minicom 2.9+ from PATH: {error}"))?;
    if !version.status.success() || !supported_version(&String::from_utf8_lossy(&version.stdout)) {
        return Err("Minicom 2.9 or newer is required. Check minicom --version.".into());
    }

    let home = tempfile::tempdir().map_err(|error| format!("Creating Minicom profile: {error}"))?;
    fs::write(home.path().join(format!(".minirc.{PROFILE_NAME}")), PROFILE)
        .map_err(|error| format!("Writing Minicom profile: {error}"))?;
    command(device, baud, home.path())
        .status()
        .map_err(|error| format!("Launching Minicom: {error}"))
}

fn command(device: &Path, baud: u32, home: &Path) -> Command {
    let mut command = Command::new("minicom");
    command
        .env("HOME", home)
        .env_remove("MINICOM")
        .arg("--device")
        .arg(device)
        .arg("--baudrate")
        .arg(baud.to_string())
        .args(["--color=on", "--wrap", PROFILE_NAME])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

fn supported_version(output: &str) -> bool {
    let Some(version) = output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("minicom version "))
    else {
        return false;
    };
    let mut parts = version.trim().split('.');
    let Some(major) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    let Some(minor) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    (major, minor) >= (2, 9)
}

pub fn require_tty() -> io::Result<()> {
    use std::io::IsTerminal;

    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        Ok(())
    } else {
        Err(io::Error::other(
            "Open the picker in an interactive WSL terminal, or use --list.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_supported_version() {
        for version in ["2.9", "2.11.1", "3.0"] {
            assert!(supported_version(&format!(
                "minicom version {version}\nCopyright"
            )));
        }
        for version in ["2.8", "2.10x", "2", "", "unknown"] {
            assert!(!supported_version(&format!("minicom version {version}")));
        }
    }

    #[test]
    fn minicom_receives_literal_device_and_baud_arguments() {
        let command = command(
            Path::new("/dev/serial path"),
            9600,
            Path::new("/tmp/example"),
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "--device",
                "/dev/serial path",
                "--baudrate",
                "9600",
                "--color=on",
                "--wrap",
                PROFILE_NAME
            ]
        );
    }
}
