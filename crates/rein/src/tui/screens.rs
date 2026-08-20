//! The four screens (§10): Mission Control, Live Attempt, Recovery Console,
//! Compare. Headless-renderable — every function takes a Frame and data,
//! no terminal required.
//!
//! Visual rules: committed evidence panes carry `[committed]` titles with
//! double borders (the "solid rule"); live reads carry `[live]` with plain,
//! muted borders — visually separate, always. Absence is words, never blank.
//! Color is semantic ([`super::theme`]): a hue never says more than the
//! receipt text it decorates.

use super::data::{ActionState, AttemptDetail, CompareReport, UiSnapshot};
use super::theme;
use super::Screen;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
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
        .title(Line::from(vec![
            Span::styled(title.to_string(), theme::heading()),
            Span::styled(" [committed]", theme::muted()),
        ]))
}

fn live_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::muted())
        .title(Line::from(vec![
            Span::styled(title.to_string(), theme::heading()),
            Span::styled(" [live]", theme::muted()),
        ]))
}

fn header_row(cells: Vec<&str>) -> Row<'_> {
    Row::new(cells).style(theme::heading())
}

/// A `label value` line where the value carries a chosen style.
fn field(label: &str, value: String, style: Style) -> Line<'_> {
    Line::from(vec![Span::raw(label), Span::styled(value, style)])
}

/// Top chrome: brand, the four screens as tabs, the workspace.
const SPINNER: [&str; 6] = ["⠋", "⠙", "⠸", "⠴", "⠦", "⠇"];

pub fn render_tabs(
    f: &mut Frame<'_>,
    area: Rect,
    active: Screen,
    workspace: &str,
    running: usize,
    frame: u64,
) {
    let tab = |n: &str, name: &str, s: Screen| -> Vec<Span<'static>> {
        let label = format!(" {n} {name} ");
        if s == active {
            vec![Span::styled(
                label,
                theme::accent().add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )]
        } else {
            vec![Span::styled(label, theme::muted())]
        }
    };
    let mut spans = vec![Span::styled(
        " rein ",
        theme::heading().add_modifier(Modifier::REVERSED),
    )];
    spans.extend(tab("1", "mission", Screen::MissionControl));
    spans.extend(tab("2", "live", Screen::LiveAttempt));
    spans.extend(tab("3", "recovery", Screen::Recovery));
    spans.extend(tab("4", "compare", Screen::Compare));
    spans.push(Span::styled(format!("  {workspace}"), theme::muted()));
    if running > 0 {
        let spin = SPINNER[(frame as usize) % SPINNER.len()];
        spans.push(Span::styled(
            format!("  {spin} {running} running"),
            theme::accent(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Bottom chrome: the keys that matter here, keycaps bright, labels muted.
pub fn render_keybar(f: &mut Frame<'_>, area: Rect, screen: Screen, results_open: bool) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut hint = |k: &'static str, label: &'static str| {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", theme::muted()));
        }
        spans.push(Span::styled(
            k,
            theme::accent().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {label}"), theme::muted()));
    };
    if results_open {
        hint("j/k", "scroll");
        hint("n/p", "next/prev artifact");
        hint("Esc", "back");
        hint("1-4", "screens");
        hint("q", "quit");
    } else {
        hint("j/k", "move");
        hint("Enter", "open results");
        match screen {
            Screen::MissionControl => hint("a/b", "mark compare pair"),
            Screen::LiveAttempt => hint("p", "publish (receipt-gated)"),
            Screen::Recovery => {
                hint("m/r/u", "the three safe actions");
                hint("y/n", "confirm");
            }
            Screen::Compare => hint("a/b", "mark on mission control"),
        }
        hint("1-4", "screens");
        hint("?", "help");
        hint(":", "palette");
        hint("q", "quit");
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The results viewer: artifact list left, verified content right — from a
/// row on any screen straight to what the attempt actually produced, read
/// back through the CAS. Absence is stated; truncation is stated.
pub fn render_results(f: &mut Frame<'_>, area: Rect, rv: &super::data::ResultsView) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(area);

    let title = format!("results — {} ({})", rv.attempt_id, rv.task_ref);
    let rows: Vec<Row> = if rv.artifacts.is_empty() {
        vec![Row::new(vec![Cell::from(Span::styled(
            "no committed artifacts — nothing was produced (stated, not blank)",
            theme::muted(),
        ))])]
    } else {
        rv.artifacts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let verdict = if a.verdicts.iter().all(|(_, v)| v == "passed") {
                    if a.verdicts.is_empty() {
                        Span::styled("unvalidated", theme::muted())
                    } else {
                        Span::styled("passed", theme::ok())
                    }
                } else {
                    Span::styled("flagged", theme::bad())
                };
                let row = Row::new(vec![
                    Cell::from(Span::styled(a.name.clone(), theme::accent())),
                    Cell::from(format!("{}B", a.bytes)),
                    Cell::from(verdict),
                ]);
                if i == rv.selected {
                    row.style(theme::selected())
                } else {
                    row
                }
            })
            .collect()
    };
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(55),
                Constraint::Percentage(20),
                Constraint::Percentage(25),
            ],
        )
        .header(header_row(vec!["artifact", "size", "validators"]))
        .block(committed_block(&title)),
        cols[0],
    );

    let Some(a) = rv.artifacts.get(rv.selected) else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "select an artifact — none on this side (stated, not blank)",
                theme::muted(),
            ))
            .block(live_block("content")),
            cols[1],
        );
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(a.digest.clone(), theme::muted()),
        Span::raw("  "),
    ]));
    for (validator, verdict) in &a.verdicts {
        lines.push(Line::from(vec![
            Span::styled(validator.clone(), theme::accent()),
            Span::raw(" "),
            theme::status_span(verdict),
        ]));
    }
    lines.push(Line::from(""));
    for l in &a.preview {
        lines.push(Line::from(l.clone()));
    }
    if a.truncated {
        lines.push(Line::from(Span::styled(
            "… truncated — full bytes via `rein artifact cat <digest>` (stated)",
            theme::warn(),
        )));
    }
    let content_title = format!("{} — read back through the CAS", a.name);
    f.render_widget(
        Paragraph::new(lines)
            .scroll((rv.scroll, 0))
            .block(committed_block(&content_title)),
        cols[1],
    );
}

pub fn render_mission_control(f: &mut Frame<'_>, area: Rect, snap: &UiSnapshot, selected: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(6),
            Constraint::Length(6),
        ])
        .split(area);

    // Current Truth panel (§10): pack/lock hashes, epoch, PIT mode.
    let pit_style = if snap.truth.pit_mode.contains("production") {
        theme::ok()
    } else {
        theme::warn()
    };
    let truth = Paragraph::new(vec![
        field("workspace      ", snap.workspace.clone(), theme::accent()),
        field("epoch          ", snap.truth.epoch.clone(), theme::accent()),
        Line::from(vec![
            Span::raw("source cutoff  "),
            Span::styled(snap.truth.source_cutoff.clone(), theme::accent()),
            Span::raw("   pit mode "),
            Span::styled(snap.truth.pit_mode.clone(), pit_style),
        ]),
        field(
            "providers.lock ",
            snap.truth.providers_lock.clone(),
            theme::muted(),
        ),
    ])
    .block(committed_block("current truth"));
    f.render_widget(truth, chunks[0]);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let task_rows: Vec<Row> = if snap.tasks.is_empty() {
        vec![Row::new(vec![Cell::from(Span::styled(
            "no tasks — nothing planned yet (stated, not blank)",
            theme::muted(),
        ))])]
    } else {
        snap.tasks
            .iter()
            .map(|t| {
                let adjudication = if t.satisfied {
                    "satisfied"
                } else {
                    "unsatisfied"
                };
                Row::new(vec![
                    Cell::from(Span::styled(t.task_ref.clone(), theme::accent())),
                    Cell::from(t.task_type.clone()),
                    Cell::from(theme::status_span(adjudication)),
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
    .header(header_row(vec!["task", "type", "adjudication"]))
    .block(committed_block("tasks"));
    f.render_widget(tasks, mid[0]);

    let attempt_rows: Vec<Row> = if snap.attempts.is_empty() {
        vec![Row::new(vec![Cell::from(Span::styled(
            "no attempts — nothing has run (stated, not blank)",
            theme::muted(),
        ))])]
    } else {
        snap.attempts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let outcome_cell = match a.outcome.as_ref() {
                    Some((o, rcpt)) => Cell::from(Line::from(vec![
                        theme::status_span(o),
                        Span::styled(format!(" per {rcpt}"), theme::muted()),
                    ])),
                    None => Cell::from(Span::styled("no terminal receipt yet", theme::muted())),
                };
                let row = Row::new(vec![
                    Cell::from(Span::styled(a.attempt_id.clone(), theme::accent())),
                    Cell::from(a.state.clone()),
                    outcome_cell,
                ]);
                // The cursor j/k moves — it drives the Live screen and the
                // a/b compare marks, so it must be visible here.
                if i == selected {
                    row.style(theme::selected())
                } else {
                    row
                }
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
    .header(header_row(vec![
        "attempt",
        "state",
        "outcome (per receipt)",
    ]))
    .block(committed_block("attempts"));
    f.render_widget(attempts, mid[1]);

    let failures = if snap.validator_failures.is_empty() {
        Line::from(Span::styled(
            "validator failures: none recorded",
            theme::muted(),
        ))
    } else {
        Line::from(vec![
            Span::raw("validator failures: "),
            Span::styled(snap.validator_failures.join(" · "), theme::bad()),
        ])
    };
    let queue_line = if snap.queue.is_empty() {
        Line::from(Span::styled(
            "recovery queue: empty (a statement, not a blank) — press 3 for the console",
            theme::muted(),
        ))
    } else {
        Line::from(vec![
            Span::raw("recovery queue: "),
            Span::styled(
                format!("{} typed anomalies", snap.queue.len()),
                theme::warn(),
            ),
            Span::raw(" — press "),
            theme::key("3"),
            Span::raw(" for the console"),
        ])
    };
    let foot = Paragraph::new(vec![failures, queue_line])
        .wrap(Wrap { trim: true })
        .block(live_block("signals"));
    f.render_widget(foot, chunks[2]);
}

pub fn render_live_attempt(
    f: &mut Frame<'_>,
    area: Rect,
    detail: Option<&AttemptDetail>,
    publish_state: Option<&ActionState>,
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
        Paragraph::new(ORGANIZING_SENTENCE).style(theme::muted().add_modifier(Modifier::ITALIC)),
        chunks[0],
    );

    let Some(d) = detail else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "no attempt selected — pick one on Mission Control (stated, not blank)",
                theme::muted(),
            ))
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
    let artifact = format!("{}", axes.artifact);
    let outcome = format!("{}", axes.outcome);
    let satisfaction = format!("{}", axes.satisfaction);
    let research = format!("{}", axes.research_acceptance);
    let admission = format!("{}", axes.system_admission);
    let lines = vec![
        Line::from(vec![
            Span::raw("attempt            "),
            Span::styled(d.attempt_id.clone(), theme::accent()),
            Span::raw("   ("),
            Span::styled(d.task_ref.clone(), theme::accent()),
            Span::raw(")"),
        ]),
        field(
            "context hash       ",
            d.context_hash.clone(),
            theme::muted(),
        ),
        field(
            "child process      ",
            process.clone(),
            theme::status_style(&process),
        ),
        field(
            "harness run        ",
            process.clone(),
            theme::status_style(&process),
        ),
        field(
            "artifact           ",
            artifact.clone(),
            theme::status_style(&artifact),
        ),
        Line::from(format!(
            "validation         {} recorded verdicts",
            d.validations.len()
        )),
        field(
            "attempt outcome    ",
            outcome.clone(),
            theme::status_style(&outcome),
        ),
        field(
            "task satisfaction  ",
            satisfaction.clone(),
            theme::status_style(&satisfaction),
        ),
        field(
            "research acceptance ",
            research.clone(),
            theme::status_style(&research),
        ),
        field(
            "system admission    ",
            admission.clone(),
            theme::status_style(&admission),
        ),
    ];
    f.render_widget(
        Paragraph::new(lines).block(committed_block(
            "state panel — six vocabularies, never one badge",
        )),
        chunks[1],
    );

    let val_rows: Vec<Row> = if d.validations.is_empty() {
        vec![Row::new(vec![Cell::from(Span::styled(
            "no validation receipts — validators have not run (stated)",
            theme::muted(),
        ))])]
    } else {
        d.validations
            .iter()
            .map(|(a, v, verdict)| {
                Row::new(vec![
                    Cell::from(a.clone()),
                    Cell::from(Span::styled(v.clone(), theme::accent())),
                    Cell::from(theme::status_span(verdict)),
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
        .header(header_row(vec!["artifact", "validator", "verdict"]))
        .block(committed_block("validation receipts")),
        chunks[2],
    );

    // Action bar: disabled actions explain themselves (invariant 32).
    let action_line = match publish_state {
        Some(ActionState::Enabled) => Line::from(vec![
            theme::key("p"),
            Span::raw(" publish-evidence "),
            Span::styled("[enabled]", theme::ok()),
        ]),
        Some(ActionState::Disabled { explain }) => Line::from(vec![
            theme::key("p"),
            Span::raw(" publish-evidence "),
            Span::styled("[disabled]", theme::muted()),
            Span::styled(format!(" — {explain}"), theme::muted()),
        ]),
        None => Line::from(vec![
            theme::key("p"),
            Span::raw(" publish-evidence "),
            Span::styled("[disabled]", theme::muted()),
            Span::styled(" — no attempt selected", theme::muted()),
        ]),
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
            Paragraph::new(vec![
                Line::from("queue empty — nothing awaiting recovery"),
                Line::from(Span::styled(
                    "(an empty panel and a failed one mean opposite things)",
                    theme::muted(),
                )),
            ])
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
                let row = Row::new(vec![
                    Cell::from(Span::styled(r.attempt_id.clone(), theme::accent())),
                    Cell::from(Span::styled(format!("{:?}", r.anomaly), theme::unknown())),
                    Cell::from(r.diagnosis.clone()),
                ]);
                if i == selected {
                    row.style(theme::selected())
                } else {
                    row
                }
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
            .header(header_row(vec!["attempt", "typed anomaly", "diagnosis"]))
            .block(live_block("recovery queue — typed anomalies")),
            chunks[0],
        );
    }

    // Exactly three safe actions; each requires a confirmation popup —
    // authority-changing actions never happen on a single keystroke. There
    // is no force-success keybinding by construction (the keymap has no such
    // entry to disable).
    let action = |k: &'static str, name: &'static str, note: &'static str| {
        Line::from(vec![
            theme::key(k),
            Span::styled(format!("  {name}"), theme::heading()),
            Span::styled(format!("  {note}"), theme::muted()),
        ])
    };
    let actions = Paragraph::new(vec![
        action(
            "m",
            "resume-commit",
            "(new fence generation; old generations may not commit)",
        ),
        action(
            "r",
            "retry",
            "(same ContextPack, byte-identical; new attempt)",
        ),
        action(
            "u",
            "close-as-unknown",
            "(explicit — unknown never defaults to anything)",
        ),
        Line::from(Span::styled(
            "forbidden: force success — no keybinding exists",
            theme::muted(),
        )),
    ])
    .block(committed_block("the three safe actions"));
    f.render_widget(actions, chunks[1]);
}

pub fn render_compare(f: &mut Frame<'_>, area: Rect, report: Option<&CompareReport>) {
    let Some(r) = report else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "no pair selected — mark attempts with a and b on Mission Control (stated, not blank)",
                theme::muted(),
            ))
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
            let label = row.class.label();
            // Difference classes carry their own severity: semantic and
            // output differences are the ones that can change a conclusion.
            let class_style = if label.contains("semantic-input")
                || label.contains("output")
                || label.contains("policy")
            {
                theme::bad()
            } else if label.contains("unexplained") {
                theme::unknown()
            } else {
                theme::muted()
            };
            let value_style = if differs {
                theme::warn()
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(row.subject.clone()),
                Cell::from(Span::styled(row.a.clone(), value_style)),
                Cell::from(Span::styled(row.b.clone(), value_style)),
                Cell::from(Span::styled(label, class_style)),
            ])
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
        .header(header_row(vec![
            "subject",
            &r.a as &str,
            &r.b as &str,
            "class",
        ]))
        .block(committed_block(
            "compare — differences classified (6 classes, complete)",
        )),
        area,
    );
}

pub fn render_help(f: &mut Frame<'_>, area: Rect) {
    let key_line = |keys: &'static str, label: &'static str| {
        Line::from(vec![
            Span::styled(
                format!("{keys:<12}"),
                theme::accent().add_modifier(Modifier::BOLD),
            ),
            Span::raw(label),
        ])
    };
    let text = vec![
        key_line(
            "1/2/3/4",
            "screens: mission control · live attempt · recovery · compare",
        ),
        key_line(
            "g then 1-4",
            "goto screen   j/k  move   a/b  mark compare pair",
        ),
        key_line(":", "palette (screen N | quit)   F2  mouse capture toggle"),
        key_line("Esc", "unwind (popup → selection → quit)   ?  this help"),
        key_line(
            "m/r/u",
            "recovery actions, then y to confirm — never a single keystroke",
        ),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled("help (?)", theme::heading()));
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
