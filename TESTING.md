# Tests

Run these checks in the repository from Linux or WSL. For formatting and linting, install `rustfmt` and `clippy` with `rustup component add rustfmt clippy`.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

To check the minimum Rust and Cargo version, install that toolchain with `rustup toolchain install 1.88.0 --profile minimal` and run `cargo +1.88.0 test --locked`.

## Automated cases

| Case | Expected result |
| --- | --- |
| CLI flags and invalid input | A valid absolute device path and positive baud rate parse. Missing and invalid values produce errors. |
| Linux device discovery | Numbered serial nodes sort numerically. Non-character files and inactive UART placeholders are excluded. |
| USB/IP state | COM ports and their sharing state are parsed and sorted. Unplugged devices, non-serial USB entries, malformed COM names, and malformed bus IDs are rejected or skipped. |
| Linux TTY matching | One eligible USB vendor/product match is suggested. Zero or multiple matches require manual selection. |
| TUI rendering | The empty state and controls remain visible. Device text cannot send terminal control sequences. Baud changes move to the adjacent preset. |
| List mode | The fake `usbipd.exe` produces a COM row without Minicom installed in the fixture's search path. Unavailable interop reports a diagnostic while list mode stays available. |
| Interactive input and attachment | Launching the picker without a terminal reports an error. On a pseudoterminal, selecting a shared COM row runs the exact `usbipd.exe attach --wsl --busid` command for that row and refreshes its state. |
| Minicom version and command | The picker accepts versions starting at 2.9. The selected device and baud are passed as literal process arguments. |

## Real Minicom test

Start with Minicom 2.9 or newer on `PATH` and an installed `xterm` terminfo entry. Run the opt-in test with this command.

```sh
cargo test --locked --test terminal -- --ignored
```

The test creates two pseudoterminals, opens the actual Minicom executable from the picker, and checks the applied serial settings and data in both directions. It exits Minicom with Ctrl+A then `X`, checks that the picker reappears, then quits the picker. It uses no physical serial device or Windows USB attachment.

## Manual WSL hardware cases

Use a USB serial adapter with a known COM number and a device that can send or echo serial data. Do the sharing test with an adapter that is initially unshared. Match the baud rate and framing on both ends.

| Action | Expected result |
| --- | --- |
| Plug in an unshared USB serial adapter and press `R` | The Windows COM row shows **Needs sharing** and its USB bus ID. Enter displays the Administrator PowerShell bind command for that row. |
| Run the displayed bind command and press `R` | The row shows **Ready to attach**. |
| Select the shared COM row and press Enter | The device joins WSL and its Linux serial node appears. A single new matching TTY is highlighted. |
| Connect two identical adapters or use an adapter with multiple serial interfaces | When multiple candidate TTYs are present, the picker asks you to select the path. Enter on a Linux path starts Minicom for that path. |
| Open the Linux row as a user without serial-device permission | The row shows **Permission denied** and Enter explains how to check the device group. After fixing access and logging in again, the row shows **Ready**. |
| Send and receive data in Minicom, then exit | The serial device and Minicom exchange the expected bytes. The picker returns when Minicom closes. |
| Detach the adapter from PowerShell and press `R` | The Linux serial node disappears and the Windows row can be attached again. Unplugging the adapter and pressing `R` removes the COM row. |

Live discovery was checked with COM8 on USB bus `6-1` and an attached `/dev/ttyACM0`. The development user lacked permission to open that physical port. The automated Minicom test verified serial data transfer over a pseudoterminal, and the attachment test exercised a simulated Windows USB/IP command.
