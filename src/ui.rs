use chrono::Local;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Padding, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::App;
use crate::palette::Palette;
use crate::slurm::{NodeInfo, QueueJob};

pub fn draw(f: &mut Frame, app: &App) {
    let p = &app.palette;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // summary
            Constraint::Length(12), // upper (jobs + nodes)
            Constraint::Length(1),  // gap
            Constraint::Min(0),    // lower (details / modal)
        ])
        .split(f.area());

    draw_summary(f, chunks[0], app, p);

    let upper = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Length(1),
            Constraint::Percentage(40),
        ])
        .split(chunks[1]);

    draw_jobs_table(f, upper[0], &app.data.queue_jobs, p);
    let sep_lines: Vec<Line> = (0..upper[1].height).map(|_| Line::from("│")).collect();
    f.render_widget(
        Paragraph::new(sep_lines).style(Style::default().fg(p.dim)),
        upper[1],
    );
    draw_nodes_table(f, upper[2], &app.data.node_infos, p);

    // Lower area: modal overlay or job details
    if let Some(ref modal) = app.modal {
        crate::input::draw_modal(f, chunks[3], modal, p);
    } else {
        draw_job_details(f, chunks[3], app, p);
    }
}

fn draw_summary(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let data = &app.data;
    let running = data.queue_jobs.iter().filter(|j| j.state == "RUNNING").count();
    let pending = data.queue_jobs.iter().filter(|j| j.state == "PENDING").count();
    let idle = data.node_infos.iter().filter(|n| n.state == "idle").count();
    let mixed = data.node_infos.iter().filter(|n| n.state == "mix").count();
    let alloc = data.node_infos.iter().filter(|n| n.state == "alloc").count();
    let now = Local::now().format("%H:%M:%S");

    let spans = vec![
        Span::styled("Jobs: ", Style::default().fg(p.fg).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{running} running"), Style::default().fg(p.blue)),
        Span::styled(" | ", Style::default().fg(p.gray)),
        Span::styled(format!("{pending} pending"), Style::default().fg(p.yellow)),
        Span::styled("    Nodes: ", Style::default().fg(p.fg).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{idle} idle"), Style::default().fg(p.aqua)),
        Span::styled(" | ", Style::default().fg(p.gray)),
        Span::styled(format!("{mixed} mixed"), Style::default().fg(p.purple)),
        Span::styled(" | ", Style::default().fg(p.gray)),
        Span::styled(format!("{alloc} alloc"), Style::default().fg(p.yellow)),
        Span::styled(format!("    Updated: {now}"), Style::default().fg(p.gray)),
        Span::raw("    "),
        Span::styled("s", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":submit ", Style::default().fg(p.gray)),
        Span::styled("c", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":cancel ", Style::default().fg(p.gray)),
        Span::styled("d", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":chdir ", Style::default().fg(p.gray)),
        Span::styled("q", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":quit", Style::default().fg(p.gray)),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.dim))
        .padding(Padding::horizontal(1))
        .title_alignment(Alignment::Center)
        .title(format!("cwd: {}", app.cwd));
    let paragraph = Paragraph::new(Line::from(spans)).block(block);
    f.render_widget(paragraph, area);
}

fn draw_jobs_table(f: &mut Frame, area: Rect, jobs: &[QueueJob], p: &Palette) {
    let title = " Job Queue ";

    let outer = inset_left(area, 1);
    let area_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(outer);

    let banner = Paragraph::new(title)
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Black)
                .bg(p.blue)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(banner, area_chunks[0]);

    let header = Row::new(vec![
        Cell::from("JobID"),
        Cell::from("User"),
        Cell::from("Name"),
        Cell::from("Part"),
        Cell::from("State"),
        Cell::from("Time"),
        Cell::from("N"),
        Cell::from("NodeList"),
    ])
    .style(Style::default().fg(p.fg).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = if jobs.is_empty() {
        vec![Row::new(vec![
            Cell::from(""),
            Cell::from(""),
            Cell::from(Span::styled("No jobs in queue", Style::default().fg(p.gray))),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ])]
    } else {
        jobs.iter()
            .map(|j| {
                let state_color = match j.state.as_str() {
                    "RUNNING" => p.blue,
                    "PENDING" => p.yellow,
                    "COMPLETING" => p.aqua,
                    "FAILED" | "CANCELLED" => p.red,
                    _ => p.fg,
                };
                Row::new(vec![
                    Cell::from(Span::styled(&j.job_id, Style::default().fg(p.blue))),
                    Cell::from(Span::styled(&j.user, Style::default().fg(p.aqua))),
                    Cell::from(Span::styled(&j.name, Style::default().fg(p.fg))),
                    Cell::from(Span::styled(&j.partition, Style::default().fg(p.yellow))),
                    Cell::from(Span::styled(&j.state, Style::default().fg(state_color))),
                    Cell::from(Span::styled(&j.time, Style::default().fg(p.purple))),
                    Cell::from(Span::styled(&j.nodes, Style::default().fg(p.fg))),
                    Cell::from(Span::styled(&j.nodelist, Style::default().fg(p.gray))),
                ])
            })
            .collect()
    };

    let widths = [
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(16),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(3),
        Constraint::Min(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(Block::default().padding(Padding::horizontal(1)));
    f.render_widget(table, area_chunks[1]);
}

fn draw_nodes_table(f: &mut Frame, area: Rect, nodes: &[NodeInfo], p: &Palette) {
    let title = " Cluster Status ";

    let outer = inset_right(area, 1);
    let area_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(outer);

    let banner = Paragraph::new(title)
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Black)
                .bg(p.blue)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(banner, area_chunks[0]);

    let header = Row::new(vec![
        Cell::from("Part"),
        Cell::from("Av"),
        Cell::from("N"),
        Cell::from("State"),
        Cell::from("NodeList"),
    ])
    .style(Style::default().fg(p.fg).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = nodes
        .iter()
        .map(|n| {
            let state_color = match n.state.as_str() {
                "idle" => p.aqua,
                "mix" => p.purple,
                "alloc" => p.yellow,
                s if s.starts_with("drain") || s.starts_with("down") => p.red,
                _ => p.fg,
            };
            Row::new(vec![
                Cell::from(Span::styled(&n.partition, Style::default().fg(p.blue))),
                Cell::from(Span::styled(&n.avail, Style::default().fg(p.aqua))),
                Cell::from(Span::styled(&n.nodes, Style::default().fg(p.fg))),
                Cell::from(Span::styled(&n.state, Style::default().fg(state_color))),
                Cell::from(Span::styled(&n.nodelist, Style::default().fg(p.gray))),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Min(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(Block::default().padding(Padding::horizontal(1)));
    f.render_widget(table, area_chunks[1]);
}

fn draw_job_details(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let details = &app.data.job_details;
    let total = app.data.running_total;
    let max_jobs = app.max_jobs;

    let title = if total > max_jobs {
        format!("Running Job Details ({total}) - showing {max_jobs} of {total}")
    } else {
        format!("Running Job Details ({total})")
    };

    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.blue))
        .title_alignment(Alignment::Center)
        .title(title.clone());

    let inner = details_block.inner(area);
    let content = inset_horizontal(inner, 2);

    if details.is_empty() {
        let text = Paragraph::new(Span::styled("No running jobs", Style::default().fg(p.gray)));
        f.render_widget(text, content);
        f.render_widget(details_block, area);
        return;
    }

    let current_user = std::env::var("USER").unwrap_or_default();

    // Build lines for each job
    let mut job_blocks: Vec<Vec<Line>> = Vec::new();
    for d in details {
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled(
                format!("Job {}", d.job_id),
                Style::default().fg(p.blue).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" - {}", truncate(&d.name, 20)),
                Style::default().fg(p.fg),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("User: ", Style::default().fg(p.gray)),
            Span::styled(&d.user, Style::default().fg(p.aqua)),
            Span::styled("  Node: ", Style::default().fg(p.gray)),
            Span::styled(&d.node, Style::default().fg(p.yellow)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Time: ", Style::default().fg(p.gray)),
            Span::styled(&d.elapsed, Style::default().fg(p.purple)),
            Span::styled(format!(" / {}", d.timelimit), Style::default().fg(p.dim)),
        ]));
        let mut res_spans = vec![
            Span::styled("CPUs: ", Style::default().fg(p.gray)),
            Span::styled(&d.cpus, Style::default().fg(p.orange)),
            Span::styled("  Mem: ", Style::default().fg(p.gray)),
            Span::styled(&d.mem, Style::default().fg(p.orange)),
        ];
        if !d.gpu.is_empty() {
            res_spans.push(Span::styled("  GPU: ", Style::default().fg(p.gray)));
            res_spans.push(Span::styled(&d.gpu, Style::default().fg(p.orange)));
        }
        lines.push(Line::from(res_spans));

        if !d.stdout.is_empty() && d.stdout != "N/A" {
            lines.push(Line::from(vec![
                Span::styled("Log: ", Style::default().fg(p.gray)),
                Span::styled(&d.stdout, Style::default().fg(p.dim)),
            ]));
            if !d.tail.is_empty() {
                let n_lines = if d.user == current_user { 5 } else { 2 };
                let tail_lines: Vec<&str> = d.tail.lines().collect();
                let start = tail_lines.len().saturating_sub(n_lines);
                for tl in &tail_lines[start..] {
                    lines.push(Line::from(Span::styled(
                        format!("  {tl}"),
                        Style::default().fg(p.dim),
                    )));
                }
            }
        }
        lines.push(Line::from("")); // blank separator
        job_blocks.push(lines);
    }

    let total_lines: usize = job_blocks.iter().map(Vec::len).sum();
    let can_use_single_column = total_lines as u16 <= content.height;

    if can_use_single_column || job_blocks.len() < 2 {
        let all_lines: Vec<Line> = job_blocks.into_iter().flatten().collect();
        let paragraph = Paragraph::new(all_lines).wrap(Wrap { trim: false });
        f.render_widget(paragraph, content);
    } else {
        let mid = (job_blocks.len() + 1) / 2;
        let left_lines: Vec<Line> = job_blocks[..mid].iter().flatten().cloned().collect();
        let right_lines: Vec<Line> = job_blocks[mid..].iter().flatten().cloned().collect();

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Length(6), Constraint::Percentage(50)])
            .split(content);

        f.render_widget(Paragraph::new(left_lines), cols[0]);
        f.render_widget(Paragraph::new(right_lines), cols[2]);
    }

    f.render_widget(details_block, area);
}

fn inset_left(area: Rect, inset: u16) -> Rect {
    if area.width <= inset {
        area
    } else {
        Rect {
            x: area.x + inset,
            y: area.y,
            width: area.width - inset,
            height: area.height,
        }
    }
}

fn inset_right(area: Rect, inset: u16) -> Rect {
    if area.width <= inset {
        area
    } else {
        Rect {
            x: area.x,
            y: area.y,
            width: area.width - inset,
            height: area.height,
        }
    }
}

fn inset_horizontal(area: Rect, inset: u16) -> Rect {
    if area.width <= inset * 2 {
        area
    } else {
        Rect {
            x: area.x + inset,
            y: area.y,
            width: area.width - inset * 2,
            height: area.height,
        }
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
