use std::{env, process::Command};

use serde::Deserialize;

use crate::ports::UsbId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    NeedsSharing,
    Shared,
    Attached,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::NeedsSharing => "Needs sharing",
            Self::Shared => "Ready to attach",
            Self::Attached => "Attached",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub com: String,
    pub bus: String,
    pub description: String,
    pub usb_id: Option<UsbId>,
    pub state: State,
    instance: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UsbipdState {
    devices: Vec<UsbipdDevice>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UsbipdDevice {
    bus_id: Option<String>,
    description: String,
    instance_id: String,
    persisted_guid: Option<String>,
    #[serde(rename = "ClientIPAddress")]
    client_ip: Option<String>,
}

pub fn scan() -> Result<Vec<Device>, String> {
    if env::var_os("WSL_DISTRO_NAME").is_none() {
        return Ok(Vec::new());
    }
    parse_state(&usbipd(&["state"])?)
}

pub fn attach(selected: &Device) -> Result<(), String> {
    let device = scan()?
        .into_iter()
        .find(|device| device.bus == selected.bus && device.instance == selected.instance)
        .ok_or("The Windows device changed or was unplugged. Refresh and select it again.")?;
    match device.state {
        State::NeedsSharing => Err(format!(
            "Share {} in Administrator PowerShell: usbipd bind --busid {}",
            device.com, device.bus
        )),
        State::Attached => Ok(()),
        State::Shared => usbipd(&["attach", "--wsl", "--busid", &device.bus]).map(|_| ()),
    }
}

fn usbipd(args: &[&str]) -> Result<String, String> {
    let output = Command::new("usbipd.exe")
        .args(args)
        .output()
        .map_err(|error| {
            format!("Windows COM discovery needs usbipd-win 5+ on the WSL PATH ({error}).")
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = stderr
        .lines()
        .chain(stdout.lines())
        .find(|line| line.to_lowercase().contains("error:"))
        .or_else(|| stderr.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or_else(|| stdout.trim());
    Err(format!(
        "usbipd {} failed ({}). {message}",
        args[0], output.status
    ))
}

fn parse_state(output: &str) -> Result<Vec<Device>, String> {
    let state: UsbipdState = serde_json::from_str(output.trim_start_matches('\u{feff}'))
        .map_err(|error| format!("Cannot read usbipd state JSON: {error}"))?;
    let mut devices = Vec::new();
    for row in state.devices {
        let Some(bus) = row.bus_id.filter(|value| valid_bus(value)) else {
            continue;
        };
        let ports = com_numbers(&row.description);
        if ports.is_empty() {
            continue;
        }
        let com = ports
            .iter()
            .map(|number| format!("COM{number}"))
            .collect::<Vec<_>>()
            .join(", ");
        let state = if row.client_ip.is_some() {
            State::Attached
        } else if row.persisted_guid.is_some() {
            State::Shared
        } else {
            State::NeedsSharing
        };
        devices.push((
            ports[0],
            Device {
                com,
                bus,
                description: row.description,
                usb_id: usb_id(&row.instance_id),
                state,
                instance: row.instance_id,
            },
        ));
    }
    devices.sort_by_key(|(com, _)| *com);
    Ok(devices.into_iter().map(|(_, device)| device).collect())
}

fn com_numbers(description: &str) -> Vec<u32> {
    let mut numbers: Vec<_> = description
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter_map(|word| {
            let digits = word.get(3..)?;
            if !word.get(..3)?.eq_ignore_ascii_case("COM")
                || !digits.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            digits.parse::<u32>().ok().filter(|&number| number > 0)
        })
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

fn valid_bus(bus: &str) -> bool {
    fn digits(value: &str) -> bool {
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
    }
    bus.split_once('-')
        .is_some_and(|(root, path)| digits(root) && path.split('.').all(digits))
}

fn usb_id(instance: &str) -> Option<UsbId> {
    let mut parts = instance.split('\\');
    if !parts.next()?.eq_ignore_ascii_case("USB") {
        return None;
    }
    let hardware = parts.next()?.to_ascii_uppercase();
    let mut ids = hardware.split('&');
    let vendor = ids.next()?.strip_prefix("VID_")?;
    let product = ids.next()?.strip_prefix("PID_")?;
    Some(UsbId(
        u16::from_str_radix(vendor, 16).ok()?,
        u16::from_str_radix(product, 16).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connected_serial_devices_and_sorts_by_numeric_com() {
        let report = r#"{"Devices":[
            {"BusId":"6-1","Description":"CMSIS-DAP, USB Serial (COM12)",
             "InstanceId":"USB\\VID_2E8A&PID_000C\\123","PersistedGuid":"guid"},
            {"BusId":"2-3","Description":"USB Serial (COM3), (COM17)",
             "InstanceId":"USB\\VID_0403&PID_6010\\A", "ClientIPAddress":"172.31.0.2"},
            {"BusId":"4-2","Description":"USB Serial (COM20)",
             "InstanceId":"USB\\VID_0403&PID_6001\\B"},
            {"BusId":null,"Description":"USB Serial (COM5)","InstanceId":"unplugged"},
            {"BusId":"6-2","Description":"USB Keyboard","InstanceId":"keyboard"}
        ]}"#;
        let devices = parse_state(report).unwrap();
        assert_eq!(
            devices
                .iter()
                .map(|device| (device.com.as_str(), device.state))
                .collect::<Vec<_>>(),
            [
                ("COM3, COM17", State::Attached),
                ("COM12", State::Shared),
                ("COM20", State::NeedsSharing)
            ]
        );
        assert_eq!(devices[1].usb_id, Some(UsbId(0x2e8a, 0x000c)));
    }

    #[test]
    fn recognizes_com_tokens_and_rejects_malformed_state() {
        assert_eq!(
            com_numbers("COM, XCOM1, com2, (COM10) (COM2), COM0, COM3x"),
            [2, 10]
        );
        assert!(parse_state("not JSON").is_err());
        assert!(parse_state("{}").is_err());
        assert!(valid_bus("6-1.3"));
        for bad in ["", "2", "-1", "1-", "2-3.", "2-3;command"] {
            assert!(!valid_bus(bad), "{bad}");
        }
    }
}
