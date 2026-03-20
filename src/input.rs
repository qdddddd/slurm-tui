use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};
use std::path::Path;

use crate::palette::Palette;

/// Represents the current modal dialog state.
pub struct Modal {
    pub kind: ModalKind,
    pub title: String,
    pub prompt: String,
    pub buf: String,
    pub cursor: usize,
    pub body_lines: Vec<(String, BodyStyle)>,
    pub message: Option<(String, MsgStyle)>,
    /// Tab completion state
    pub completions: Vec<String>,
    pub comp_index: isize,
    pub comp_prefix: String,
    pub comp_suffix: String,
    /// History navigation
    pub hist_index: usize,
    pub saved_buf: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ModalKind {
    Submit,
    Cancel,
    CancelConfirm,
    Chdir,
}

#[derive(Clone, Copy)]
pub enum BodyStyle {
    Gray,
    Dim,
    Blue,
    Yellow,
    Fg,
    Red,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum MsgStyle {
    Green,
    Red,
    Yellow,
    Gray,
}

impl Modal {
    pub fn new(kind: ModalKind, title: &str, prompt: &str) -> Self {
        Self {
            kind,
            title: title.to_string(),
            prompt: prompt.to_string(),
            buf: String::new(),
            cursor: 0,
            body_lines: Vec::new(),
            message: None,
            completions: Vec::new(),
            comp_index: -1,
            comp_prefix: String::new(),
            comp_suffix: String::new(),
            hist_index: 0,
            saved_buf: String::new(),
        }
    }

    pub fn with_body(mut self, lines: Vec<(String, BodyStyle)>) -> Self {
        self.body_lines = lines;
        self
    }

    pub fn set_message(&mut self, msg: String, style: MsgStyle) {
        self.message = Some((msg, style));
    }

    pub fn clear_completions(&mut self) {
        self.completions.clear();
        self.comp_index = -1;
    }

    pub fn insert_char(&mut self, ch: char) {
        self.buf.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.clear_completions();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.buf[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.buf.drain(prev..self.cursor);
            self.cursor = prev;
            self.clear_completions();
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.buf.len() {
            let next = self.buf[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.buf.len());
            self.buf.drain(self.cursor..next);
            self.clear_completions();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.buf[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.buf.len() {
            self.cursor = self.buf[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.buf.len());
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.buf.len();
    }

    pub fn kill_to_end(&mut self) {
        self.buf.truncate(self.cursor);
        self.clear_completions();
    }

    pub fn kill_to_start(&mut self) {
        self.buf = self.buf[self.cursor..].to_string();
        self.cursor = 0;
        self.clear_completions();
    }

    pub fn kill_word(&mut self) {
        let left = self.buf[..self.cursor].trim_end();
        let idx = left.rfind(|c: char| c == ' ' || c == '/').map(|i| i + 1).unwrap_or(0);
        self.buf = format!("{}{}", &self.buf[..idx], &self.buf[self.cursor..]);
        self.cursor = idx;
        self.clear_completions();
    }

    pub fn submit(&self) -> Option<String> {
        let trimmed = self.buf.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

// ── Path completion ──

pub fn path_complete(text: &str, cwd: Option<&str>) -> Vec<String> {
    let expanded = if text.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            home.to_string_lossy().to_string() + &text[1..]
        } else {
            text.to_string()
        }
    } else {
        text.to_string()
    };

    let full = if !Path::new(&expanded).is_absolute() {
        if let Some(cwd) = cwd {
            format!("{cwd}/{expanded}")
        } else {
            expanded.clone()
        }
    } else {
        expanded.clone()
    };

    let pattern = format!("{full}*");
    let mut results = Vec::new();
    for entry in glob::glob(&pattern).into_iter().flatten().flatten() {
        let is_dir = entry.is_dir();
        let display = if !Path::new(text).is_absolute() {
            if let Some(cwd) = cwd {
                entry
                    .strip_prefix(cwd)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| entry.to_string_lossy().to_string())
            } else {
                entry.to_string_lossy().to_string()
            }
        } else {
            entry.to_string_lossy().to_string()
        };
        if is_dir {
            results.push(format!("{display}/"));
        } else {
            results.push(display);
        }
    }
    results.sort();
    results
}

pub fn handle_tab(modal: &mut Modal, cwd: &str) {
    if modal.completions.is_empty() {
        // Find word around cursor
        let left = &modal.buf[..modal.cursor];
        let right = &modal.buf[modal.cursor..];
        let word_start = left.rfind(' ').map(|i| i + 1).unwrap_or(0);
        let word_end = right.find(' ').map(|i| modal.cursor + i).unwrap_or(modal.buf.len());
        let word = &modal.buf[word_start..word_end];
        modal.comp_prefix = modal.buf[..word_start].to_string();
        modal.comp_suffix = modal.buf[word_end..].to_string();
        modal.completions = path_complete(word, Some(cwd));
        if !modal.completions.is_empty() {
            modal.comp_index = 0;
            modal.buf = format!("{}{}{}", modal.comp_prefix, modal.completions[0], modal.comp_suffix);
            modal.cursor = modal.comp_prefix.len() + modal.completions[0].len();
        }
    } else {
        modal.comp_index = (modal.comp_index + 1) % modal.completions.len() as isize;
        let idx = modal.comp_index as usize;
        modal.buf = format!("{}{}{}", modal.comp_prefix, modal.completions[idx], modal.comp_suffix);
        modal.cursor = modal.comp_prefix.len() + modal.completions[idx].len();
    }
}

// ── Drawing ──

pub fn draw_modal(f: &mut Frame, area: Rect, modal: &Modal, p: &Palette) {
    let mut lines: Vec<Line> = Vec::new();

    // Body lines
    for (text, style) in &modal.body_lines {
        let color = match style {
            BodyStyle::Gray => p.gray,
            BodyStyle::Dim => p.dim,
            BodyStyle::Blue => p.blue,
            BodyStyle::Yellow => p.yellow,
            BodyStyle::Fg => p.fg,
            BodyStyle::Red => p.red,
        };
        lines.push(Line::from(Span::styled(text.as_str(), Style::default().fg(color))));
    }
    if !modal.body_lines.is_empty() {
        lines.push(Line::from(""));
    }

    // Message or input line
    if let Some((ref msg, ref style)) = modal.message {
        let color = match style {
            MsgStyle::Green => p.green,
            MsgStyle::Red => p.red,
            MsgStyle::Yellow => p.yellow,
            MsgStyle::Gray => p.gray,
        };
        lines.push(Line::from(Span::styled(msg.as_str(), Style::default().fg(color))));
    } else {
        let mut spans = vec![Span::styled(
            &modal.prompt,
            Style::default().fg(p.yellow).add_modifier(Modifier::BOLD),
        )];
        let pos = modal.cursor;
        let buf = &modal.buf;
        spans.push(Span::styled(&buf[..pos], Style::default().fg(p.fg)));
        if pos < buf.len() {
            let next_end = buf[pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| pos + i)
                .unwrap_or(buf.len());
            spans.push(Span::styled(
                &buf[pos..next_end],
                Style::default().fg(p.fg).add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::styled(&buf[next_end..], Style::default().fg(p.fg)));
        } else {
            spans.push(Span::styled("█", Style::default().fg(p.fg)));
        }
        lines.push(Line::from(spans));

        // Completions
        if !modal.completions.is_empty() {
            for (i, c) in modal.completions.iter().enumerate() {
                if i as isize == modal.comp_index {
                    lines.push(Line::from(Span::styled(
                        format!(" > {c}"),
                        Style::default().fg(p.aqua).add_modifier(Modifier::BOLD),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        format!("   {c}"),
                        Style::default().fg(p.dim),
                    )));
                }
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.yellow))
        .title_alignment(Alignment::Center)
        .title(modal.title.as_str())
        .padding(Padding::horizontal(1));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}
