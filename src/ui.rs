use crate::app::App;
use crate::mounts;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

const ACCENT: Color = Color::Rgb(122, 162, 247);
const MUTED: Color = Color::Rgb(86, 95, 137);
const OK: Color = Color::Rgb(158, 206, 106);
const WARN: Color = Color::Rgb(224, 175, 104);
const ERR: Color = Color::Rgb(247, 118, 142);
const BG: Color = Color::Rgb(26, 27, 38);

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Min(6),
            Constraint::Length(1),
        ])
        .split(area);

    draw_title(f, chunks[0], app);
    draw_devices(f, chunks[1], app);
    draw_logs(f, chunks[2], app);
    draw_footer(f, chunks[3], app);

    if app.show_help {
        draw_help(f, area);
    }
    if app.confirm {
        draw_confirm(f, area, app);
    }
}

fn draw_title(f: &mut Frame, area: Rect, app: &App) {
    let version = match &app.latest_version {
        Some(v) => format!(" · Rockbox {v}"),
        None => "".to_string(),
    };
    let line = Line::from(vec![
        Span::styled(
            " rockbox-tui ",
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" install Rockbox onto iPods ", Style::default().fg(MUTED)),
        Span::styled(version, Style::default().fg(OK)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_devices(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(Span::styled(" iPods ", Style::default().fg(ACCENT)));

    if app.devices.is_empty() {
        let msg = if app.denied > 0 {
            format!(
                "No iPods found. {} disk(s) need root access — run with sudo. (r = rescan)",
                app.denied
            )
        } else {
            "No iPods found. Plug one in and press r to rescan.".to_string()
        };
        let p = Paragraph::new(msg).block(block).style(Style::default().fg(WARN));
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = app
        .devices
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let model = d.ipod.model.as_ref().map(|m| m.modelstr).unwrap_or("unknown");
            let target = d.ipod.build_target().unwrap_or("unknown");
            let mount = mounts::find_data_mount(&d.path)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not mounted".into());
            let ram = if d.ipod.ramsize_mb > 0 {
                format!("{} MiB", d.ipod.ramsize_mb)
            } else {
                "RAM?".into()
            };

            let sel = i == app.selected;
            let key_fg = if sel { ACCENT } else { Color::White };
            let line = Line::from(vec![
                Span::styled(
                    format!(" {} {} ", if sel { "›" } else { " " }, d.path),
                    Style::default().fg(key_fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{model} "), Style::default().fg(Color::White)),
                Span::styled(format!("({ram}) "), Style::default().fg(MUTED)),
                Span::styled(format!("target={target} "), Style::default().fg(ACCENT)),
                Span::styled(mount, Style::default().fg(OK)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_logs(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(Span::styled(" Log ", Style::default().fg(ACCENT)));

    let inner = block.inner(area);

    let progress_h = if app.running { 2 } else { 0 };
    let logs_area = Rect {
        y: inner.y + progress_h,
        height: inner.height.saturating_sub(progress_h),
        ..inner
    };

    if app.running {
        let step = app.step.as_deref().unwrap_or("Working…");
        let pct = app
            .progress
            .and_then(|(d, t)| t.filter(|&t| t > 0).map(|t| ((d as f64 / t as f64) * 100.0) as u16))
            .unwrap_or(0);
        let head = Line::from(vec![
            Span::styled(app.spinner(), Style::default().fg(ACCENT)),
            Span::raw(" "),
            Span::styled(step, Style::default().fg(Color::White)),
        ]);
        f.render_widget(Paragraph::new(head), Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 });
        f.render_widget(
            Gauge::default().gauge_style(Style::default().fg(ACCENT).bg(BG)).percent(pct),
            Rect { x: inner.x, y: inner.y + 1, width: inner.width, height: 1 },
        );
    }

    let shown: Vec<Line> = app
        .logs
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(Color::Gray))))
        .collect();
    f.render_widget(Paragraph::new(shown).wrap(Wrap { trim: false }), logs_area);
    f.render_widget(block, area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &mut App) {
    let (status, color) = if let Some(s) = &app.status_msg {
        let c = if s.contains("failed") { ERR } else { OK };
        (s.clone(), c)
    } else if app.running {
        ("installing…".to_string(), ACCENT)
    } else {
        ("ready".to_string(), OK)
    };

    let line = Line::from(vec![
        Span::styled(format!(" {status} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(
            " s:install  r:rescan  j/k:select  f:follow  u/d:scroll  x:clear  ?:help  q:quit ",
            Style::default().fg(MUTED),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(64, 72, area);
    let text = vec![
        Line::from(Span::styled(
            " rockbox-tui ",
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(" s / Enter   install Rockbox on the selected iPod"),
        Line::from(" j / k / ↑ / ↓   move selection"),
        Line::from(" r           rescan for iPods"),
        Line::from(" f           toggle log follow"),
        Line::from(" u / d       scroll logs"),
        Line::from(" x           clear logs"),
        Line::from(" ? / h       toggle this help"),
        Line::from(" q / Esc     quit"),
        Line::from(""),
        Line::from("Needs raw-device access — the TUI elevates via pkexec."),
        Line::from("Install backs up the firmware, flashes the bootloader,"),
        Line::from("and copies .rockbox onto the mounted data partition."),
    ];
    let p = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(ACCENT)))
        .wrap(Wrap { trim: true });
    f.render_widget(Clear, popup);
    f.render_widget(p, popup);
}

fn draw_confirm(f: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(64, 62, area);
    let mut lines = vec![
        Line::from(Span::styled(
            " Install Rockbox? ",
            Style::default().fg(Color::Black).bg(ERR).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if let Some(d) = app.selected_device() {
        let model = d.ipod.model.as_ref().map(|m| m.modelstr).unwrap_or("unknown");
        let target = d.ipod.build_target().unwrap_or("unknown");
        lines.push(Line::from(vec![
            Span::styled("Device: ", Style::default().fg(MUTED)),
            Span::styled(format!("{} ({model})", d.path), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Build target: ", Style::default().fg(MUTED)),
            Span::styled(target, Style::default().fg(ACCENT)),
        ]));
        lines.push(Line::from(""));
    }

    lines.extend_from_slice(&[
        Line::from("This will back up the firmware partition, then flash"),
        Line::from("the Rockbox bootloader and copy .rockbox onto the"),
        Line::from("mounted data partition."),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y / Enter  ", Style::default().fg(OK).add_modifier(Modifier::BOLD)),
            Span::styled("install   ", Style::default().fg(Color::White)),
            Span::styled(" n / Esc ", Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
            Span::styled("cancel", Style::default().fg(Color::White)),
        ]),
    ]);

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(ERR)))
        .wrap(Wrap { trim: true });
    f.render_widget(Clear, popup);
    f.render_widget(p, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
