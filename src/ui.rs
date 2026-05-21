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
use crate::slurm::{PartitionStats, QueueJob};

pub fn draw(f: &mut Frame, app: &App) {
    let p = &app.palette;
    // Grow the upper section so every cluster row fits (banner + header + rows),
    // but keep at least 6 lines for the lower details section.
    let nodes_required = app.data.partition_stats.len() as u16 + 2;
    let upper_min = 12u16;
    let upper_max = f.area().height.saturating_sub(3 + 1 + 6).max(upper_min);
    let upper_height = nodes_required.max(upper_min).min(upper_max);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),            // summary
            Constraint::Length(upper_height), // upper (jobs + nodes)
            Constraint::Length(1),            // gap
            Constraint::Min(0),               // lower (details / modal)
        ])
        .split(f.area());

    draw_summary(f, chunks[0], app, p);

    // Right 1/3 fixed for Cluster Status; left 2/3 for Job Queue with a 1-col separator.
    let upper_w = chunks[1].width;
    let cluster_w = upper_w / 3;
    let jobs_w = upper_w.saturating_sub(cluster_w + 1);
    let upper = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(jobs_w),
            Constraint::Length(1),
            Constraint::Length(cluster_w),
        ])
        .split(chunks[1]);

    draw_jobs_table(f, upper[0], &app.data.queue_jobs, p);
    let sep_lines: Vec<Line> = (0..upper[1].height).map(|_| Line::from("│")).collect();
    f.render_widget(
        Paragraph::new(sep_lines).style(Style::default().fg(p.dim)),
        upper[1],
    );
    draw_nodes_table(f, upper[2], &app.data.partition_stats, p);

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
    let idle = data.idle_nodes;
    let mixed = data.mix_nodes;
    let alloc = data.alloc_nodes;
    let down = data.down_nodes;
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
        Span::styled(" | ", Style::default().fg(p.gray)),
        Span::styled(format!("{down} down"), Style::default().fg(p.red)),
        Span::styled(format!("    Updated: {now}"), Style::default().fg(p.gray)),
        Span::raw("    "),
        Span::styled("s", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":submit ", Style::default().fg(p.gray)),
        Span::styled("c", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":cancel ", Style::default().fg(p.gray)),
        Span::styled("d", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":chdir ", Style::default().fg(p.gray)),
        Span::styled("l", Style::default().fg(p.orange).add_modifier(Modifier::BOLD)),
        Span::styled(":logs ", Style::default().fg(p.gray)),
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

fn jobs_column_widths(jobs: &[QueueJob]) -> [u16; 8] {
    let fit = |get: &dyn Fn(&QueueJob) -> &str, header: &str, cap: usize| -> u16 {
        let m = jobs.iter().map(|j| get(j).len()).max().unwrap_or(0);
        m.max(header.len()).min(cap) as u16
    };
    // Name falls back to placeholder length when no jobs are queued.
    let name_max = if jobs.is_empty() {
        "No jobs in queue".len()
    } else {
        jobs.iter().map(|j| j.name.len()).max().unwrap_or(0)
    };
    [
        fit(&|j| &j.job_id, "JobID", 30),
        fit(&|j| &j.user, "User", 20),
        name_max.max("Name".len()).min(30) as u16,
        fit(&|j| &j.partition, "Part", 20),
        fit(&|j| &j.state, "State", 12),
        fit(&|j| &j.time, "Time", 12),
        fit(&|j| &j.nodes, "N", 6),
        fit(&|j| &j.nodelist, "NodeList", 40),
    ]
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

    let cw = jobs_column_widths(jobs);
    let widths = [
        Constraint::Length(cw[0]), // JobID
        Constraint::Length(cw[1]), // User
        Constraint::Min(8),        // Name (takes leftover space)
        Constraint::Length(cw[3]), // Part
        Constraint::Length(cw[4]), // State
        Constraint::Length(cw[5]), // Time
        Constraint::Length(cw[6]), // N
        Constraint::Length(cw[7]), // NodeList
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(Block::default().padding(Padding::horizontal(1)));
    f.render_widget(table, area_chunks[1]);
}

fn draw_nodes_table(f: &mut Frame, area: Rect, stats: &[PartitionStats], p: &Palette) {
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

    let headers = ["Part", "Idle", "States", "CPU", "Mem", "GPU"];
    let header = Row::new(headers.iter().map(|h| Cell::from(*h)).collect::<Vec<_>>())
        .style(Style::default().fg(p.fg).add_modifier(Modifier::BOLD));

    let ratio_cell = |s: &str, free_color: Color| -> Cell<'static> {
        match s.split_once('/') {
            Some((num, den)) => Cell::from(Line::from(vec![
                Span::styled(num.to_string(), Style::default().fg(free_color)),
                Span::styled(format!("/{den}"), Style::default().fg(p.fg)),
            ])),
            None => Cell::from(Span::styled(s.to_string(), Style::default().fg(free_color))),
        }
    };
    let rows: Vec<Row> = stats
        .iter()
        .map(|s| {
            let idle_color = if s.idle_nodes > 0 { p.aqua } else { p.gray };
            let cpu_color = if s.cpu_idle > 0 { p.aqua } else { p.gray };
            let mem_color = if s.mem_free_mb > 0 { p.aqua } else { p.gray };
            let gpu_color = if s.gpu_total == 0 {
                p.gray
            } else if s.gpu_free > 0 {
                p.aqua
            } else {
                p.yellow
            };

            // States column: each segment colored to match the summary line.
            let mut state_spans: Vec<Span> = Vec::new();
            for (count, letter, color) in [
                (s.idle_nodes, "i", p.aqua),
                (s.mix_nodes, "m", p.purple),
                (s.alloc_nodes, "a", p.yellow),
                (s.down_nodes, "d", p.red),
                (s.other_nodes, "o", p.fg),
            ] {
                if count == 0 {
                    continue;
                }
                if !state_spans.is_empty() {
                    state_spans.push(Span::raw(" "));
                }
                state_spans.push(Span::styled(
                    format!("{count}{letter}"),
                    Style::default().fg(color),
                ));
            }

            let idle = format!("{}/{}", s.idle_nodes, s.total_nodes);
            let cpu = format!("{}/{}", s.cpu_idle, s.cpu_total);
            let mem = format!("{}/{}", fmt_mem(s.mem_free_mb), fmt_mem(s.mem_total_mb));
            let gpu = if s.gpu_total > 0 {
                format!("{}/{}", s.gpu_free, s.gpu_total)
            } else {
                "-".into()
            };

            Row::new(vec![
                Cell::from(Span::styled(s.partition.clone(), Style::default().fg(p.blue))),
                ratio_cell(&idle, idle_color),
                Cell::from(Line::from(state_spans)),
                ratio_cell(&cpu, cpu_color),
                ratio_cell(&mem, mem_color),
                ratio_cell(&gpu, gpu_color),
            ])
        })
        .collect();

    let part_w = stats
        .iter()
        .map(|s| s.partition.len())
        .max()
        .unwrap_or(0)
        .max("Part".len()) as u16;
    let idle_w = stats
        .iter()
        .map(|s| format!("{}/{}", s.idle_nodes, s.total_nodes).len())
        .max()
        .unwrap_or(0)
        .max("Idle".len()) as u16;
    let states_w = stats
        .iter()
        .map(|s| {
            let counts = [
                s.idle_nodes,
                s.mix_nodes,
                s.alloc_nodes,
                s.down_nodes,
                s.other_nodes,
            ];
            let n_segs = counts.iter().filter(|&&n| n > 0).count();
            let chars: usize = counts
                .iter()
                .filter(|&&n| n > 0)
                .map(|n| n.to_string().len() + 1) // digits + state letter
                .sum();
            chars + n_segs.saturating_sub(1) // spaces between segments
        })
        .max()
        .unwrap_or(0)
        .max("States".len()) as u16;
    let widths = [
        Constraint::Length(part_w),
        Constraint::Length(idle_w),
        Constraint::Length(states_w),
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(Block::default().padding(Padding::horizontal(1)));
    f.render_widget(table, area_chunks[1]);
}

fn fmt_mem(mb: u64) -> String {
    if mb < 1024 {
        format!("{mb}M")
    } else if mb < 1024 * 1024 {
        format!("{:.1}G", mb as f64 / 1024.0)
    } else {
        format!("{:.1}T", mb as f64 / 1024.0 / 1024.0)
    }
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
    clear_area(f, content);

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

        clear_area(f, cols[0]);
        clear_area(f, cols[2]);
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

fn clear_area(f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let blank_line = " ".repeat(area.width as usize);
    let lines: Vec<Line> = (0..area.height).map(|_| Line::from(blank_line.as_str())).collect();
    let paragraph = Paragraph::new(lines).style(Style::reset());
    f.render_widget(paragraph, area);
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
