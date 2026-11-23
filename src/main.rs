mod minicom;
mod ports;
mod ui;
mod windows;

use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

const HELP: &str = "\
wsl-com-picker — choose a serial port and open Minicom

Usage: wsl-com-picker [--list] [--device PATH] [--baud RATE]

  --list         Print Linux and Windows ports without opening the picker
  --device PATH  Include an explicit Linux serial or PTY device
  --baud RATE    Set initial baud rate (default 115200)
  --version      Print the picker version
  --help         Show this help
";

#[derive(Debug, PartialEq, Eq)]
enum Options {
    Help,
    Version,
    Run {
        list: bool,
        device: Option<PathBuf>,
        baud: u32,
    },
}

impl Options {
    fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let mut list = false;
        let mut device = None;
        let mut baud = 115200;
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--help" | "-h") => return Ok(Self::Help),
                Some("--version" | "-V") => return Ok(Self::Version),
                Some("--list") => list = true,
                Some("--device") => {
                    let path = args
                        .next()
                        .map(PathBuf::from)
                        .ok_or("--device needs a path")?;
                    if !path.is_absolute() {
                        return Err("--device needs an absolute Linux device path".into());
                    }
                    device = Some(path);
                }
                Some("--baud") => {
                    baud = args
                        .next()
                        .and_then(|value| value.to_str()?.parse::<u32>().ok())
                        .filter(|&value| value > 0)
                        .ok_or("--baud needs a positive integer")?;
                }
                _ => {
                    return Err(format!(
                        "Unknown argument {}. See --help.",
                        arg.to_string_lossy()
                    ));
                }
            }
        }
        Ok(Self::Run { list, device, baud })
    }
}

fn run() -> Result<(), String> {
    match Options::parse(env::args_os().skip(1))? {
        Options::Help => print!("{HELP}"),
        Options::Version => println!("wsl-com-picker {}", env!("CARGO_PKG_VERSION")),
        Options::Run { list, device, baud } => {
            if let Some(path) = &device
                && !ports::is_character_device(path)
            {
                return Err(format!(
                    "{} is not an available character device",
                    path.display()
                ));
            }
            if list {
                for port in ports::scan(device.as_deref()).map_err(|error| error.to_string())? {
                    let access = if port.readable_writable {
                        "Ready"
                    } else {
                        "Permission denied"
                    };
                    println!(
                        "{}\t{access}\t{}",
                        ui::plain(&port.label),
                        ui::plain(&port.detail)
                    );
                }
                match windows::scan() {
                    Ok(devices) => {
                        for device in devices {
                            println!(
                                "{}\t{}\tUSB {}\t{}",
                                device.com,
                                device.state.label(),
                                device.bus,
                                ui::plain(&device.description)
                            );
                        }
                    }
                    Err(error) => eprintln!("wsl-com-picker: {}", ui::plain(&error)),
                }
            } else {
                minicom::require_tty().map_err(|error| error.to_string())?;
                ui::run(device.as_deref(), baud)?;
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    if let Err(error) = run() {
        eprintln!("wsl-com-picker: {error}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn list_accepts_an_explicit_device_and_baud() {
        assert_eq!(
            parse(&["--list", "--device", "/dev/pts/1", "--baud", "57600"]),
            Ok(Options::Run {
                list: true,
                device: Some("/dev/pts/1".into()),
                baud: 57600
            })
        );
    }

    #[test]
    fn errors_on_invalid_arguments() {
        for args in [
            &["--baud", "0"][..],
            &["--baud"],
            &["--device", "ttyUSB0"],
            &["--wat"],
        ] {
            assert!(parse(args).is_err());
        }
    }
}
