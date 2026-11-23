use std::{
    io::{self, Stdout},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    minicom,
    ports::{self, Port, UsbId},
    windows::{self, Device, State},
};

const BAUDS: [u32; 8] = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const ACCENT: Color = Color::Cyan;

#[derive(Clone, Debug)]
enum Row {
    Linux(Port),
    Windows(Device),
}

impl Row {
    fn same_device(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Linux(a), Self::Linux(b)) => a.path == b.path,
            (Self::Windows(a), Self::Windows(b)) => a.bus == b.bus && a.com == b.com,
            _ => false,
        }
    }
}

struct Picker {
    rows: Vec<Row>,
    cursor: ListState,
    baud: u32,
    notice: String,
    scan_errors: Vec<String>,
}

impl Picker {
    fn new(baud: u32) -> Self {
        Self {
            rows: Vec::new(),
            cursor: ListState::default(),
            baud,
            notice: String::new(),
            scan_errors: Vec::new(),
        }
    }

    fn reload(&mut self, extra: Option<&Path>) {
        let selected = self.selected().cloned();
        self.scan_errors.clear();
        self.rows = match ports::scan(extra) {
            Ok(ports) => ports.into_iter().map(Row::Linux).collect(),
            Err(error) => {
                self.scan_errors
                    .push(format!("Could not scan /dev: {error}"));
                Vec::new()
            }
        };
        match windows::scan() {
            Ok(devices) => self.rows.extend(devices.into_iter().map(Row::Windows)),
            Err(error) => self.scan_errors.push(error),
        }
        let index = selected.and_then(|old| self.rows.iter().position(|row| row.same_device(&old)));
        self.cursor
            .select(index.or_else(|| (!self.rows.is_empty()).then_some(0)));
    }

    fn selected(&self) -> Option<&Row> {
        self.cursor
            .selected()
            .and_then(|index| self.rows.get(index))
    }

    fn move_cursor(&mut self, delta: isize) {
        if let Some(index) = self.cursor.selected() {
            self.cursor.select(Some(
                index.saturating_add_signed(delta).min(self.rows.len() - 1),
            ));
        }
    }

    fn change_baud(&mut self, next: bool) {
        let candidate = if next {
            BAUDS.iter().find(|&&value| value > self.baud)
        } else {
            BAUDS.iter().rev().find(|&&value| value < self.baud)
        };
        if let Some(&value) = candidate {
            self.baud = value;
        }
    }

    fn linux_ports(&self) -> impl Iterator<Item = &Port> {
        self.rows.iter().filter_map(|row| match row {
            Row::Linux(port) => Some(port),
            Row::Windows(_) => None,
        })
    }

    fn suggest_port(&mut self, device: &Device, exclude: &[PathBuf]) {
        let Some(path) =
            unique_match(self.linux_ports(), device.usb_id, exclude).map(|port| port.path.clone())
        else {
            self.notice = format!(
                "{} is attached. Select its Linux port, or press R if it has yet to appear.",
                device.com
            );
            return;
        };
        let index = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Linux(port) if port.path == path));
        self.cursor.select(index);
        self.notice = format!(
            "Possible {} match at {}. Verify the path, then press Enter.",
            device.com,
            path.display()
        );
    }
}

fn unique_match<'a>(
    ports: impl Iterator<Item = &'a Port>,
    id: Option<UsbId>,
    exclude: &[PathBuf],
) -> Option<&'a Port> {
    let id = id?;
    let mut matches = ports.filter(|port| port.usb_id == Some(id) && !exclude.contains(&port.path));
    let candidate = matches.next()?;
    matches.next().is_none().then_some(candidate)
}

struct Screen(Terminal<CrosstermBackend<Stdout>>);

impl Screen {
    fn open() -> io::Result<Self> {
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        terminal::enable_raw_mode()?;
        let mut screen = Self(terminal);
        execute!(
            screen.0.backend_mut(),
            EnterAlternateScreen,
            crossterm::cursor::Hide
        )?;
        Ok(screen)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            self.0.backend_mut(),
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        let _ = terminal::disable_raw_mode();
    }
}

enum Action {
    Quit,
    Open(Port),
}

pub fn run(extra: Option<&Path>, baud: u32) -> Result<(), String> {
    let mut picker = Picker::new(baud);
    picker.reload(extra);
    loop {
        let action =
            choose(&mut picker, extra).map_err(|error| format!("Terminal error: {error}"))?;
        let Action::Open(port) = action else {
            return Ok(());
        };
        picker.notice = match minicom::launch(&port.path, picker.baud) {
            Ok(status) if status.success() => format!("Closed {}", port.label),
            Ok(status) => format!("Minicom ended with {status} on {}", port.label),
            Err(error) => error,
        };
        picker.reload(extra);
    }
}

fn choose(picker: &mut Picker, extra: Option<&Path>) -> io::Result<Action> {
    let mut screen = Screen::open()?;
    loop {
        screen.0.draw(|frame| render(frame, picker))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if key.code == KeyCode::Char('c') {
                return Ok(Action::Quit);
            }
            continue;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Ok(Action::Quit),
            KeyCode::Up | KeyCode::Char('k') => picker.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => picker.move_cursor(1),
            KeyCode::Left => picker.change_baud(false),
            KeyCode::Right => picker.change_baud(true),
            KeyCode::Char('r') => {
                picker.notice.clear();
                picker.reload(extra);
                if picker.scan_errors.is_empty() {
                    picker.notice = "Refreshed ports.".into();
                }
            }
            KeyCode::Enter => match picker.selected().cloned() {
                Some(Row::Linux(port)) if !ports::is_character_device(&port.path) => {
                    picker.notice = "Device disappeared. Refreshed the port list.".into();
                    picker.reload(extra);
                }
                Some(Row::Linux(port)) if !ports::can_access(&port.path) => {
                    picker.notice =
                        "Permission denied. Check the device group and your dialout membership."
                            .into();
                }
                Some(Row::Linux(port)) => return Ok(Action::Open(port)),
                Some(Row::Windows(device)) => choose_windows(&mut screen, picker, extra, &device)?,
                None => {}
            },
            _ => {}
        }
    }
}

fn choose_windows(
    screen: &mut Screen,
    picker: &mut Picker,
    extra: Option<&Path>,
    device: &Device,
) -> io::Result<()> {
    match device.state {
        State::NeedsSharing => {
            picker.notice = format!(
                "In Administrator PowerShell run usbipd bind --busid {}. Then press R here.",
                device.bus
            );
        }
        State::Attached => {
            picker.reload(extra);
            picker.suggest_port(device, &[]);
        }
        State::Shared => {
            let before: Vec<_> = ports::scan(extra)?
                .into_iter()
                .map(|port| port.path)
                .collect();
            picker.notice = format!("Attaching {} to WSL…", device.com);
            screen.0.draw(|frame| render(frame, picker))?;
            let attached = windows::attach(device);
            if attached.is_ok() && device.usb_id.is_some() {
                // The USB/IP command can finish before the kernel creates its TTY nodes.
                let deadline = Instant::now() + Duration::from_secs(4);
                while Instant::now() < deadline {
                    let ports = ports::scan(extra).unwrap_or_default();
                    if ports
                        .iter()
                        .any(|port| port.usb_id == device.usb_id && !before.contains(&port.path))
                    {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            picker.reload(extra);
            match attached {
                Ok(()) => picker.suggest_port(device, &before),
                Err(error) => picker.notice = error,
            }
        }
    }
    Ok(())
}

fn render(frame: &mut Frame, picker: &mut Picker) {
    if frame.area().width < 36 || frame.area().height < 13 {
        frame.render_widget(
            Paragraph::new("Resize to at least 36×13. Q quits.").wrap(Wrap { trim: true }),
            frame.area(),
        );
        return;
    }

    let [header, table, details, notice, keys] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(4),
        Constraint::Length(2),
        Constraint::Length(2),
    ])
    .areas(frame.area());

    let title = Line::from(vec![
        Span::styled(
            " SERIAL PORTS ",
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  Minicom", Style::default().fg(Color::Gray)),
    ]);
    frame.render_widget(title, header);
    let block = Block::bordered().title(" Available ports ");
    if picker.rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No serial ports detected. Connect a device and press R.")
                .block(block)
                .wrap(Wrap { trim: true }),
            table,
        );
    } else {
        let rows = picker.rows.iter().map(render_row);
        let list = List::new(rows)
            .block(block)
            .highlight_symbol("› ")
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_stateful_widget(list, table, &mut picker.cursor);
    }

    let (detail, action) = match picker.selected() {
        Some(Row::Linux(port)) => (plain(&port.detail), "Open"),
        Some(Row::Windows(device)) => (
            plain(&device.description),
            match device.state {
                State::NeedsSharing => "Share help",
                State::Shared => "Attach",
                State::Attached => "Find port",
            },
        ),
        None => (String::new(), "Open"),
    };
    let lines = vec![
        Line::styled(
            format!("{} baud  ·  8N1  ·  No flow control", picker.baud),
            Style::default().fg(ACCENT),
        ),
        Line::from(detail),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Connection ")),
        details,
    );
    let message = if picker.notice.is_empty() {
        picker.scan_errors.join(" ")
    } else {
        picker.notice.clone()
    };
    frame.render_widget(
        Paragraph::new(plain(&message)).wrap(Wrap { trim: true }),
        notice,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "↑↓ Select   ←→ Baud   Enter {action}   R Refresh   Q Quit"
        ))
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: true }),
        keys,
    );
}

fn render_row(row: &Row) -> ListItem<'static> {
    let (label, status, color) = match row {
        Row::Linux(port) => {
            let (access, color) = if port.readable_writable {
                ("Ready", Color::Green)
            } else {
                ("Permission denied", Color::Yellow)
            };
            (plain(&port.label), access, color)
        }
        Row::Windows(device) => {
            let color = match device.state {
                State::NeedsSharing => Color::Yellow,
                State::Shared => ACCENT,
                State::Attached => Color::Green,
            };
            (
                format!("{}  ·  USB {}", device.com, device.bus),
                device.state.label(),
                color,
            )
        }
    };
    ListItem::new(Line::from(vec![
        Span::raw(label),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(color)),
    ]))
}

pub fn plain(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn selects_adjacent_baud_presets_from_custom_rate() {
        let mut picker = Picker::new(250000);
        picker.change_baud(false);
        assert_eq!(picker.baud, 230400);
        picker.change_baud(true);
        assert_eq!(picker.baud, 460800);
        picker.baud = 921600;
        picker.change_baud(true);
        assert_eq!(picker.baud, 921600);
    }

    #[test]
    fn renders_no_devices_and_controls() {
        let mut picker = Picker::new(115200);
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        terminal.draw(|frame| render(frame, &mut picker)).unwrap();
        let buffer = terminal.backend().buffer();
        let contents: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(contents.contains("No serial ports detected"));
        assert!(contents.contains("115200 baud"));
        assert!(contents.contains("Enter Open"));
    }

    #[test]
    fn replaces_control_characters_in_device_text() {
        assert_eq!(plain("A\x1b[31m\nB\t"), "A [31m B ");
    }

    #[test]
    fn suggests_only_unique_usb_matches() {
        let id = UsbId(0x1234, 0x5678);
        let ports = [
            Port {
                path: "/dev/ttyUSB0".into(),
                usb_id: Some(id),
                label: String::new(),
                detail: String::new(),
                readable_writable: true,
            },
            Port {
                path: "/dev/ttyUSB1".into(),
                usb_id: Some(id),
                label: String::new(),
                detail: String::new(),
                readable_writable: true,
            },
        ];
        assert_eq!(unique_match(ports.iter(), Some(id), &[]), None);
        assert_eq!(unique_match(ports.iter(), None, &[]), None);
        assert_eq!(
            unique_match(ports.iter(), Some(id), &[ports[0].path.clone()]),
            Some(&ports[1])
        );
    }
}
