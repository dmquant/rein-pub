//! The four screens (§10): Mission Control, Live Attempt, Recovery Console,
//! Compare. Headless-renderable — every function takes a Frame and data,
//! no terminal required.
//!
//! Visual rules: committed evidence panes carry `[committed]` titles with
//! double borders (the "solid rule"); live reads carry `[live]` with plain
//! borders — visually separate, always. Absence is words, never blank.

use super::data::{ActionState, AttemptDetail, CompareReport, UiSnapshot};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

/// The organizing sentence — stays on-screen (§10 screen 2).
pub const ORGANIZING_SENTENCE: &str =
    "Process exit is evidence only. Terminal classification waits for all required validators.";

fn committed_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(format!("{title} [committed]"))
}

fn live_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .title(format!("{title} [live]"))
}

pub fn render_mission_control(f: &mut Frame<'_>, area: Rect, snap: &UiSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(6),
            Constraint::Length(6),
        ])
        .split(area);

    // Current Truth panel (§10): pack/lock hashes, epoch, PIT mode.
    let truth = Paragraph::new(vec![
        Line::from(format!("workspace      {}", snap.workspace)),
        Line::from(format!("epoch          {}", snap.truth.epoch)),
        Line::from(format!(
            "source cutoff  {}   pit mode {}",
            snap.truth.source_cutoff, snap.truth.pit_mode
        )),
        Line::from(format!("providers.lock {}", snap.truth.providers_lock)),
    ])
    .block(committed_block("current truth"));
    f.render_widget(truth, chunks[0]);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let task_rows: Vec<Row> = if snap.tasks.is_empty() {
        vec![Row::new(vec![Cell::from(
            "no tasks — nothing planned yet (stated, not blank)",
        )])]
    } else {
        snap.tasks
            .iter()
            .map(|t| {
                Row::new(vec![
                    Cell::from(t.task_ref.clone()),
                    Cell::from(t.task_type.clone()),
                    Cell::from(if t.satisfied {
                        "satisfied"
                    } else {
                        "unsatisfied"
                    }),
                ])
            })
            .collect()
    };
    let tasks = Table::new(
        task_rows,
        [
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
    .header(
        Row::new(vec!["task", "type", "adjudication"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(committed_block("tasks"));
    f.render_widget(tasks, mid[0]);

    let attempt_rows: Vec<Row> = if snap.attempts.is_empty() {
        vec![Row::new(vec![Cell::from(
            "no attempts — nothing has run (stated, not blank)",
        )])]
    } else {
        snap.attempts
            .iter()
            .map(|a| {
                let outcome = a
                    .outcome
                    .as_ref()
                    .map(|(o, rcpt)| format!("{o} per {rcpt}"))
                    .unwrap_or_else(|| "no terminal receipt yet".to_string());
                Row::new(vec![
                    Cell::from(a.attempt_id.clone()),
                    Cell::from(a.state.clone()),
                    Cell::from(outcome),
                ])
            })
            .collect()
    };
    let attempts = Table::new(
        attempt_rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(25),
            Constraint::Percentage(45),
        ],
    )
    .header(
        Row::new(vec!["attempt", "state", "outcome (per receipt)"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(committed_block("attempts"));
    f.render_widget(attempts, mid[1]);

    let failures = if snap.validator_failures.is_empty() {
        "validator failures: none recorded".to_string()
    } else {
        format!(
            "validator failures: {}",
            snap.validator_failures.join(" · ")
        )
    };
    let queue_line = format!(
        "recovery queue: {} — press 3 for the console",
        if snap.queue.is_empty() {
            "empty (a statement, not a blank)".to_string()
        } else {
            format!("{} typed anomalies", snap.queue.len())
        }
    );
    let foot = Paragraph::new(vec![Line::from(failures), Line::from(queue_line)])
        .wrap(Wrap { trim: true })
        .block(live_block("signals"));
    f.render_widget(foot, chunks[2]);
}

pub fn render_live_attempt(
    f: &mut Frame<'_>,
    area: Rect,
    detail: Option<&AttemptDetail>,
    propose_state: Option<&ActionState>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(12),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);

    f.render_widget(
        Paragraph::new(ORGANIZING_SENTENCE).style(Style::default().add_modifier(Modifier::ITALIC)),
        chunks[0],
    );

    let Some(d) = detail else {
        f.render_widget(
            Paragraph::new("no attempt selected — pick one on Mission Control (stated, not blank)")
                .block(live_block("state panel")),
            chunks[1],
        );
        return;
    };

    // The axes as separate fields (§10 screen 2, invariant 1): child process
    // and HarnessRun are projections of the process axis; validation is the
    // outcome derivation; the two external axes render recorded state.
    let axes = &d.axes;
    let process = format!("{}", axes.process);
    let lines = vec![
        Line::from(format!(
            "attempt            {}   ({})",
            d.attempt_id, d.task_ref
        )),
        Line::from(format!("context hash       {}", d.context_hash)),
        Line::from(format!("child process      {process}")),
        Line::from(format!("harness run        {process}")),
        Line::from(format!("artifact           {}", axes.artifact)),
        Line::from(format!(
            "validation         {} recorded verdicts",
            d.validations.len()
        )),
        Line::from(format!("attempt outcome    {}", axes.outcome)),
        Line::from(format!("task satisfaction  {}", axes.satisfaction)),
        Line::from(format!("research acceptance {}", axes.research_acceptance)),
        Line::from(format!("system admission    {}", axes.system_admission)),
    ];
    f.render_widget(
        Paragraph::new(lines).block(committed_block(
            "state panel — six vocabularies, never one badge",
        )),
        chunks[1],
    );

    let val_rows: Vec<Row> = if d.validations.is_empty() {
        vec![Row::new(vec![Cell::from(
            "no validation receipts — validators have not run (stated)",
        )])]
    } else {
        d.validations
            .iter()
            .map(|(a, v, verdict)| {
                let style = if verdict == "passed" {
                    Style::default()
                } else {
                    Style::default().fg(Color::Red)
                };
                Row::new(vec![
                    Cell::from(a.clone()),
                    Cell::from(v.clone()),
                    Cell::from(verdict.clone()).style(style),
                ])
            })
            .collect()
    };
    f.render_widget(
        Table::new(
            val_rows,
            [
                Constraint::Percentage(25),
                Constraint::Percentage(30),
                Constraint::Percentage(45),
            ],
        )
        .header(
            Row::new(vec!["artifact", "validator", "verdict"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(committed_block("validation receipts")),
        chunks[2],
    );

    // Action bar: disabled actions explain themselves (invariant 32).
    let action_line = match propose_state {
        Some(ActionState::Enabled) => "p propose-to-gate [enabled]".to_string(),
        Some(ActionState::Disabled { explain }) => {
            format!("p propose-to-gate [disabled] — {explain}")
        }
        None => "p propose-to-gate [disabled] — no attempt selected".to_string(),
    };
    f.render_widget(
        Paragraph::new(action_line).block(live_block("actions")),
        chunks[3],
    );
}

pub fn render_recovery(f: &mut Frame<'_>, area: Rect, snap: &UiSnapshot, selected: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(6)])
        .split(area);

    if snap.queue.is_empty() {
        f.render_widget(
            Paragraph::new(
                "queue empty — nothing awaiting recovery\n(an empty panel and a failed one mean opposite things)",
            )
            .wrap(Wrap { trim: true })
            .block(live_block("recovery queue — typed anomalies")),
            chunks[0],
        );
    } else {
        let rows: Vec<Row> = snap
            .queue
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let style = if i == selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    Cell::from(r.attempt_id.clone()),
                    Cell::from(format!("{:?}", r.anomaly)),
                    Cell::from(r.diagnosis.clone()),
                ])
                .style(style)
            })
            .collect();
        f.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Percentage(20),
                    Constraint::Percentage(20),
                    Constraint::Percentage(60),
                ],
            )
            .header(
                Row::new(vec!["attempt", "typed anomaly", "diagnosis"])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(live_block("recovery queue — typed anomalies")),
            chunks[0],
        );
    }

    // Exactly three safe actions; each requires a confirmation popup —
    // authority-changing actions never happen on a single keystroke. There
    // is no force-success keybinding by construction (the keymap has no such
    // entry to disable).
    let actions = Paragraph::new(vec![
        Line::from("m  resume-commit  (new fence generation; old generations may not commit)"),
        Line::from("r  retry          (same ContextPack, byte-identical; new attempt)"),
        Line::from("u  close-as-unknown (explicit — unknown never defaults to anything)"),
        Line::from(Span::styled(
            "forbidden: force success — no keybinding exists",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(committed_block("the three safe actions"));
    f.render_widget(actions, chunks[1]);
}

pub fn render_compare(f: &mut Frame<'_>, area: Rect, report: Option<&CompareReport>) {
    let Some(r) = report else {
        f.render_widget(
            Paragraph::new(
                "no pair selected — mark attempts with a and b on Mission Control (stated, not blank)",
            )
            .block(live_block("compare")),
            area,
        );
        return;
    };
    let rows: Vec<Row> = r
        .rows
        .iter()
        .map(|row| {
            let differs = row.a != row.b;
            let style = if differs {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(row.subject.clone()),
                Cell::from(row.a.clone()),
                Cell::from(row.b.clone()),
                Cell::from(row.class.label()),
            ])
            .style(style)
        })
        .collect();
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(22),
                Constraint::Percentage(29),
                Constraint::Percentage(29),
                Constraint::Percentage(20),
            ],
        )
        .header(
            Row::new(vec!["subject", &r.a as &str, &r.b as &str, "class"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(committed_block(
            "compare — differences classified (6 classes, complete)",
        )),
        area,
    );
}

pub fn render_help(f: &mut Frame<'_>, area: Rect) {
    let text = vec![
        Line::from("1/2/3/4  screens: mission control · live attempt · recovery · compare"),
        Line::from("g then 1-4  goto screen   j/k  move   a/b  mark compare pair"),
        Line::from(":  palette (screen N | quit)   F2  mouse capture toggle"),
        Line::from("Esc  unwind (popup → selection → quit)   ?  this help"),
        Line::from("recovery: m/r/u then y to confirm — never a single keystroke"),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title("help (?)");
    let popup = centered(area, 70, 9);
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(text).block(block), popup);
}

pub fn centered(area: Rect, pct_x: u16, height: u16) -> Rect {
    let w = area.width * pct_x / 100;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: height.min(area.height),
    }
}
