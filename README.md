# WSL COM Picker

Pick a Windows USB serial adapter or an existing Linux serial device from a Rust TUI, then open it in Minicom. Run the picker from inside WSL.

## Install and run

Inside Ubuntu WSL, install Minicom and the build tools, then build with the locked dependencies. Install Rust with [rustup](https://rustup.rs/) if `cargo` is missing.

```sh
sudo apt update
sudo apt install minicom build-essential ncurses-base
minicom --version
cd ~/workspace/wsl-com-picker
cargo build --release --locked
./target/release/wsl-com-picker
```

Use ↑/↓ or `j`/`k` to choose a row, ←/→ to change baud, and Enter to act on the selected row. Press `R` to refresh and `Q` or Esc to quit. Inside Minicom, press Ctrl+A followed by `X` and confirm to return to the picker.

```sh
./target/release/wsl-com-picker --list
./target/release/wsl-com-picker --baud 57600
./target/release/wsl-com-picker --device /dev/pts/4 --baud 9600
```

`--list` prints the ports and their status without requiring a terminal or launching Minicom. `--device` adds an absolute Linux device path and highlights it on startup. Use your actual device path. A symlink such as `/dev/serial/by-id/…` is resolved to its device node.

The default connection is 115200 baud, 8N1, with hardware and software flow control off. The picker starts Minicom with a temporary connection profile. Changes to that profile in Minicom last for the session.

## Attach a Windows USB serial adapter

1. Install `usbipd-win` on Windows, for example with `winget install --interactive --exact dorssel.usbipd-win` in PowerShell. In WSL, run `usbipd.exe --version` to verify that Windows interop can reach it.
2. Plug in the adapter and find its bus ID with `usbipd list` in PowerShell. When the picker shows **Needs sharing**, press Enter to see the command for the selected device. Run `usbipd bind --busid <BUSID>` in Administrator PowerShell once for that device. Press `R` back in the picker.
3. On the **Ready to attach** row, press Enter to attach the USB device to WSL. The picker looks for the Linux TTY and highlights it if it finds one new match by USB vendor and product ID. Verify the highlighted path and press Enter again to open Minicom. If several ports match, select the intended Linux row manually. On an **Attached** row, Enter searches the current Linux ports again.
4. To release the device back to Windows, quit Minicom and run `usbipd detach --busid <BUSID>` in Windows PowerShell. Closing Minicom or quitting the picker leaves USB attachment active. While attached, WSL 2 distributions have access to the adapter and Windows loses access.

Windows rows come from USB devices with a COM number in their `usbipd` description. Built-in, Bluetooth, and virtual Windows COM ports require a separate bridge that exposes them as a Linux device. You can pass the resulting device to the picker with `--device`. [Microsoft WSL USB guide](https://learn.microsoft.com/en-us/windows/wsl/connect-usb), [usbipd-win JSON interface](https://github.com/dorssel/usbipd-win/wiki/Automation).

## Requirements and verified versions

| Dependency | Required version or capability | Verified in Ubuntu 24.04.3 LTS on WSL 2 |
| --- | --- | --- |
| Rust and Cargo | 1.88.0 or newer to build the Rust 2024 project | 1.88.0 and 1.97.1 |
| Minicom | 2.9 or newer on the WSL `PATH` to open a session | 2.9, Ubuntu package `2.9-4` |
| System build tools | A Linux C linker such as `cc` and standard system libraries. Ubuntu provides these through `build-essential` | `build-essential` `12.10ubuntu1` |
| Terminal | An interactive terminal with a valid `TERM` and its terminfo entry for Minicom. The picker layout needs at least 36 columns by 13 rows | `xterm` terminfo, Ubuntu `ncurses-base` `6.4+20240113-1ubuntu2.2` |
| Windows USB attachment | WSL 2 with a USB/IP-enabled kernel of at least 5.10.60.1, a Linux driver for the device, Windows interop, and `usbipd-win` 5.0.0 or newer with `usbipd.exe` on the WSL `PATH` | WSL kernel `6.6.87.2-microsoft-standard-WSL2` and `usbipd-win` 5.3.0 for live discovery |
| Git | Optional, to work with the repository and its feature history | 2.43.0 |

For USB attachment Microsoft documents Windows 11 build 22000 or later on x64 or ARM64. Windows 10 can use the Microsoft Store version of WSL. [Microsoft WSL USB guide](https://learn.microsoft.com/en-us/windows/wsl/connect-usb).

Direct Rust dependencies locked in [Cargo.lock](Cargo.lock) are Ratatui 0.30.2, Crossterm 0.29.0, libc 0.2.191, Serde 1.0.229, serde_json 1.0.154, and tempfile 3.27.0. There are no separate Rust dependencies to install by hand. [Minicom 2.9 release](https://salsa.debian.org/minicom-team/minicom/-/releases/2.9), [Minicom manual](https://www.man7.org/linux/man-pages/man1/minicom.1.html), [Microsoft WSL USB requirements](https://learn.microsoft.com/en-us/windows/wsl/connect-usb).

## Troubleshoot

- **Permission denied**. Check the device's owner and group with `ls -l /dev/ttyACM0`, substituting the path shown. If the device belongs to `dialout` on Ubuntu, run `sudo usermod -aG dialout "$USER"` and open a new WSL login session. Check the active groups with `id -nG`.
- **Attached with no Linux device**. Press `R` and check for `/dev/ttyACM*` or `/dev/ttyUSB*`. The device needs a Linux driver to create the serial node. The picker also finds active `/dev/ttyS*` ports. Use `--device` if the driver or bridge creates a different serial device path.
- **Windows ports are missing**. Check `usbipd.exe state` from WSL and `usbipd list` from PowerShell. If Windows discovery is unavailable, you can still open Linux serial ports.
- **Minicom cannot open the port**. Verify the baud rate, device permissions, and whether another serial terminal is using it. Minicom uses its regular port locking.

## Related tools

- **com2tty** provides a Windows TUI and a serial-to-PTY bridge. Minicom can open the resulting PTY. [com2tty](https://github.com/Yi-Cheng-Wang/com2tty).
- **PicoLynx** provides a Windows TUI for sharing and attaching USB serial and storage devices to WSL. [PicoLynx](https://pypi.org/project/picolynx/).
- **Scope** provides a Rust picker and its own serial console. [Scope](https://github.com/matheuswhite/scope-rs).

See [TESTING.md](TESTING.md) for automated and hardware test cases.
