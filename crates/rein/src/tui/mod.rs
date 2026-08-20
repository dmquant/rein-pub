//! The TUI (§10): four screens over the same domain core the CLI uses —
//! never parsing CLI output, never shelling out for domain operations.
//!
//! Keyboard model (§10 / PDF §37.1 + earned additions from a sibling TUI): `?` help,
//! `:` palette, `g` goto, vim motions, F2 mouse-capture toggle, Esc unwind
//! (popup → selection → quit), toasts that decay. Destructive or
//! authority-changing actions always confirm — and there is no force-success
//! keybinding by construction: [`KEYMAP`] is the complete list.

pub mod data;
pub mod screens;
pub mod theme;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use data::{load_snapshot, ActionState, AttemptDetail, CompareReport, UiSnapshot};
use ratatui::Frame;
use rein_core::ids::AttemptId;
use rein_runtime::store::Store;
use rein_runtime::workspace::Workspace;

/// The complete key → action table. A binding that is not here does not
/// exist; the recovery test asserts nothing in this table can spell
/// force-success.
#[allow(dead_code)] // the complete-keymap contract; asserted by the M4 tests
pub const KEYMAP: &[(&str, &str)] = &[
    ("1", "screen: mission control"),
    ("2", "screen: live attempt"),
    ("3", "screen: recovery console"),
    ("4", "screen: compare"),
    ("g", "goto prefix (g then 1-4)"),
    ("j", "select next"),
    ("k", "select previous"),
    ("a", "mark compare A"),
    ("b", "mark compare B"),
    ("m", "recovery: resume-commit (confirm required)"),
    ("r", "recovery: retry (confirm required)"),
    ("u", "recovery: close-as-unknown (confirm required)"),
    ("y", "confirm pending action"),
    ("n", "dismiss pending action"),
    ("?", "help"),
    (":", "palette"),
    ("F2", "mouse capture toggle"),
    ("Esc", "unwind: popup → selection → quit"),
    ("q", "quit"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    MissionControl,
    LiveAttempt,
    Recovery,
    Compare,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Popup {
    Help,
    Palette(String),
    Confirm {
        action: &'static str,
        attempt: String,
    },
}

pub struct App {
    pub screen: Screen,
    pub selected: usize,
    pub compare_a: Option<AttemptId>,
    pub compare_b: Option<AttemptId>,
    pub popup: Option<Popup>,
    pub goto_pending: bool,
    pub mouse_capture: bool,
    pub toasts: Vec<(String, u8)>,
    pub quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            screen: Screen::MissionControl,
            selected: 0,
            compare_a: None,
            compare_b: None,
            popup: None,
            goto_pending: false,
            mouse_capture: false,
            toasts: Vec::new(),
            quit: false,
        }
    }
}

impl App {
    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toasts.push((msg.into(), 6));
    }

    pub fn tick(&mut self) {
        for t in &mut self.toasts {
            t.1 = t.1.saturating_sub(1);
        }
        self.toasts.retain(|t| t.1 > 0);
    }

    /// Esc unwinds: popup → selection → quit (§10 keyboard model).
    pub fn unwind(&mut self) {
        if self.popup.is_some() {
            self.popup = None;
        } else if self.selected != 0 {
            self.selected = 0;
        } else {
            self.quit = true;
        }
    }

    pub fn handle_key(&mut self, code: KeyCode, snap: &UiSnapshot) -> Option<PendingAction> {
        if let Some(Popup::Palette(input)) = &mut self.popup {
            match code {
                KeyCode::Esc => self.popup = None,
                KeyCode::Enter => {
                    let cmd = input.trim().to_string();
                    self.popup = None;
                    match cmd.as_str() {
                        "quit" | "q" => self.quit = true,
                        "screen 1" => self.screen = Screen::MissionControl,
                        "screen 2" => self.screen = Screen::LiveAttempt,
                        "screen 3" => self.screen = Screen::Recovery,
                        "screen 4" => self.screen = Screen::Compare,
                        other => self.toast(format!("palette: unknown command `{other}`")),
                    }
                }
                KeyCode::Char(c) => input.push(c),
                KeyCode::Backspace => {
                    input.pop();
                }
                _ => {}
            }
            return None;
        }
        if let Some(Popup::Confirm { action, attempt }) = self.popup.clone() {
            match code {
                KeyCode::Char('y') => {
                    self.popup = None;
                    return Some(PendingAction {
                        action,
                        attempt: attempt.clone(),
                    });
                }
                KeyCode::Char('n') | KeyCode::Esc => self.popup = None,
                _ => {}
            }
            return None;
        }

        if self.goto_pending {
            self.goto_pending = false;
            match code {
                KeyCode::Char('1') => self.screen = Screen::MissionControl,
                KeyCode::Char('2') => self.screen = Screen::LiveAttempt,
                KeyCode::Char('3') => self.screen = Screen::Recovery,
                KeyCode::Char('4') => self.screen = Screen::Compare,
                _ => {}
            }
            return None;
        }

        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.unwind(),
            KeyCode::Char('?') => self.popup = Some(Popup::Help),
            KeyCode::Char(':') => self.popup = Some(Popup::Palette(String::new())),
            KeyCode::Char('g') => self.goto_pending = true,
            KeyCode::F(2) => {
                self.mouse_capture = !self.mouse_capture;
                self.toast(format!(
                    "mouse capture {}",
                    if self.mouse_capture { "on" } else { "off" }
                ));
            }
            KeyCode::Char('1') => self.screen = Screen::MissionControl,
            KeyCode::Char('2') => self.screen = Screen::LiveAttempt,
            KeyCode::Char('3') => self.screen = Screen::Recovery,
            KeyCode::Char('4') => self.screen = Screen::Compare,
            KeyCode::Char('j') | KeyCode::Down => self.selected = self.selected.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('a') => {
                if let Some(row) = snap.attempts.get(self.selected) {
                    self.compare_a = AttemptId::parse(&row.attempt_id).ok();
                    self.toast(format!("compare A = {}", row.attempt_id));
                }
            }
            KeyCode::Char('b') => {
                if let Some(row) = snap.attempts.get(self.selected) {
                    self.compare_b = AttemptId::parse(&row.attempt_id).ok();
                    self.toast(format!("compare B = {}", row.attempt_id));
                }
            }
            // The three recovery actions — confirm required, never a single
            // keystroke (§10 rules).
            KeyCode::Char('m') | KeyCode::Char('r') | KeyCode::Char('u')
                if self.screen == Screen::Recovery =>
            {
                if let Some(anomaly) = snap.queue.get(self.selected) {
                    let action = match code {
                        KeyCode::Char('m') => "resume-commit",
                        KeyCode::Char('r') => "retry",
                        _ => "close-unknown",
                    };
                    self.popup = Some(Popup::Confirm {
                        action,
                        attempt: anomaly.attempt_id.clone(),
                    });
                } else {
                    self.toast("recovery queue is empty — nothing to act on");
                }
            }
            _ => {}
        }
        None
    }
}

pub struct PendingAction {
    pub action: &'static str,
    pub attempt: String,
}

/// Render the whole app — pure, headless-testable.
pub fn render_app(
    f: &mut Frame<'_>,
    app: &App,
    snap: &UiSnapshot,
    detail: Option<&AttemptDetail>,
    publish_state: Option<&ActionState>,
    compare: Option<&CompareReport>,
) {
    let area = f.size();
    // Shell chrome: tab bar above, keybar below, the screen between.
    let shell = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(area);
    screens::render_tabs(f, shell[0], app.screen, &snap.workspace);
    screens::render_keybar(f, shell[2], app.screen);
    let body = shell[1];
    match app.screen {
        Screen::MissionControl => screens::render_mission_control(f, body, snap, app.selected),
        Screen::LiveAttempt => screens::render_live_attempt(f, body, detail, publish_state),
        Screen::Recovery => screens::render_recovery(f, body, snap, app.selected),
        Screen::Compare => screens::render_compare(f, body, compare),
    }
    match &app.popup {
        Some(Popup::Help) => screens::render_help(f, area),
        Some(Popup::Palette(input)) => {
            let popup = screens::centered(area, 60, 3);
            f.render_widget(ratatui::widgets::Clear, popup);
            f.render_widget(
                ratatui::widgets::Paragraph::new(format!(":{input}")).block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .border_style(theme::accent())
                        .title(ratatui::text::Span::styled("palette", theme::heading())),
                ),
                popup,
            );
        }
        Some(Popup::Confirm { action, attempt }) => {
            let popup = screens::centered(area, 64, 4);
            f.render_widget(ratatui::widgets::Clear, popup);
            f.render_widget(
                ratatui::widgets::Paragraph::new(vec![
                    ratatui::text::Line::from(vec![
                        ratatui::text::Span::styled(
                            action.to_string(),
                            theme::warn().add_modifier(ratatui::style::Modifier::BOLD),
                        ),
                        ratatui::text::Span::raw(format!(" on {attempt}?  ")),
                        theme::key("y"),
                        ratatui::text::Span::raw(" to confirm · "),
                        theme::key("n"),
                        ratatui::text::Span::raw(" to dismiss"),
                    ]),
                    ratatui::text::Line::from(ratatui::text::Span::styled(
                        "(no semantic inputs change; no prior event is rewritten)",
                        theme::muted(),
                    )),
                ])
                .block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .border_style(theme::warn())
                        .title(ratatui::text::Span::styled(
                            "confirm — never a single keystroke",
                            theme::heading(),
                        )),
                ),
                popup,
            );
        }
        _ => {}
    }
    if let Some((msg, _)) = app.toasts.last() {
        let popup = ratatui::layout::Rect {
            x: area.x + 1,
            y: area.height.saturating_sub(2),
            width: area.width.saturating_sub(2).min(msg.len() as u16 + 2),
            height: 1,
        };
        f.render_widget(
            ratatui::widgets::Paragraph::new(msg.as_str()).style(
                ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
            ),
            popup,
        );
    }
}

/// The interactive loop (`rein tui` / `attach`). Selection of attempt for the
/// Live screen follows the Mission Control cursor.
pub fn run_tui(ws: &Workspace, store: &mut Store) -> std::io::Result<()> {
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let mut app = App::default();

    let result = loop {
        let snap = match load_snapshot(ws, store) {
            Ok(s) => s,
            Err(e) => break Err(std::io::Error::other(e)),
        };
        let detail = snap
            .attempts
            .get(app.selected.min(snap.attempts.len().saturating_sub(1)))
            .and_then(|row| AttemptId::parse(&row.attempt_id).ok())
            .and_then(|aid| data::attempt_detail(store, &aid).ok());
        let publish_state = detail.as_ref().map(data::publish_action_state);
        let compare = match (&app.compare_a, &app.compare_b) {
            (Some(a), Some(b)) => data::compare_attempts(store, a, b).ok(),
            _ => None,
        };
        terminal.draw(|f| {
            render_app(
                f,
                &app,
                &snap,
                detail.as_ref(),
                publish_state.as_ref(),
                compare.as_ref(),
            )
        })?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let Some(pending) = app.handle_key(key.code, &snap) {
                        app.toast(format!(
                            "{} requested for {} — run `rein attempt recover {} --action {}`",
                            pending.action, pending.attempt, pending.attempt, pending.action
                        ));
                    }
                }
            }
        }
        app.tick();
        if app.quit {
            break Ok(());
        }
    };

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    result
}
