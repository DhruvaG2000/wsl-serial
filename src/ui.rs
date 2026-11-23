use std::{
    io::{self, Stdout},
    path::Path,
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
    ports::{self, Port},
};

const BAUDS: [u32; 8] = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const ACCENT: Color = Color::Cyan;

struct Picker {
    ports: Vec<Port>,
    cursor: ListState,
    baud: u32,
    notice: String,
}

impl Picker {
    fn new(baud: u32) -> Self {
        Self {
            ports: Vec::new(),
            cursor: ListState::default(),
            baud,
            notice: String::new(),
        }
    }

    fn reload(&mut self, extra: Option<&Path>) {
        let selected = self.selected().map(|port| port.path.clone());
        match ports::scan(extra) {
            Ok(ports) => self.ports = ports,
            Err(error) => {
                self.ports.clear();
                self.notice = format!("Could not scan /dev: {error}");
            }
        }
        let index = selected.and_then(|path| self.ports.iter().position(|port| port.path == path));
        self.cursor
            .select(index.or_else(|| (!self.ports.is_empty()).then_some(0)));
    }

    fn selected(&self) -> Option<&Port> {
        self.cursor
            .selected()
            .and_then(|index| self.ports.get(index))
    }

    fn move_cursor(&mut self, delta: isize) {
        if let Some(index) = self.cursor.selected() {
            self.cursor.select(Some(
                index.saturating_add_signed(delta).min(self.ports.len() - 1),
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
                picker.notice = "Refreshed ports.".into();
                picker.reload(extra);
            }
            KeyCode::Enter => {
                let Some(port) = picker.selected().cloned() else {
                    continue;
                };
                if !ports::is_character_device(&port.path) {
                    picker.notice = "Device disappeared. Refreshed the port list.".into();
                    picker.reload(extra);
                } else if !ports::can_access(&port.path) {
                    picker.notice =
                        "Permission denied. Check the device group and your dialout membership."
                            .into();
                } else {
                    return Ok(Action::Open(port));
                }
            }
            _ => {}
        }
    }
}

fn render(frame: &mut Frame, picker: &mut Picker) {
    let [header, table, details, keys] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(5),
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
    let block = Block::bordered().title(" Devices in WSL ");
    if picker.ports.is_empty() {
        frame.render_widget(
            Paragraph::new("No serial ports detected. Connect a device and press R.")
                .block(block)
                .wrap(Wrap { trim: true }),
            table,
        );
    } else {
        let rows = picker.ports.iter().map(|port| {
            let (access, color) = if port.readable_writable {
                ("Ready", Color::Green)
            } else {
                ("Permission denied", Color::Yellow)
            };
            ListItem::new(Line::from(vec![
                Span::raw(plain(&port.label)),
                Span::raw("  "),
                Span::styled(access, Style::default().fg(color)),
            ]))
        });
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

    let detail = picker
        .selected()
        .map_or_else(String::new, |port| plain(&port.detail));
    let lines = vec![
        Line::from(detail),
        Line::styled(
            format!("{} baud  ·  8N1  ·  No flow control", picker.baud),
            Style::default().fg(ACCENT),
        ),
        Line::from(plain(&picker.notice)),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Connection "))
            .wrap(Wrap { trim: true }),
        details,
    );

    frame.render_widget(
        Paragraph::new("↑↓ Select   ←→ Baud   Enter Open   R Refresh   Q Quit")
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: true }),
        keys,
    );
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
}
