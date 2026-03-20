use crate::history::InputHistory;
use crate::input::{BodyStyle, Modal, ModalKind, MsgStyle};
use crate::palette::Palette;
use crate::slurm::{self, SlurmData};

pub struct App {
    pub palette: Palette,
    pub cwd: String,
    pub login_node: String,
    pub data: SlurmData,
    pub max_jobs: usize,
    pub modal: Option<Modal>,
    pub history: InputHistory,
    pub should_quit: bool,
    /// When set, modal shows a timed message then auto-dismisses
    pub msg_deadline: Option<std::time::Instant>,
}

impl App {
    pub fn new(dark: bool, login_node: String) -> Self {
        Self {
            palette: if dark { Palette::dark() } else { Palette::light() },
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".into()),
            login_node,
            data: SlurmData::default(),
            max_jobs: 20,
            modal: None,
            history: InputHistory::new(),
            should_quit: false,
            msg_deadline: None,
        }
    }

    pub fn calc_max_jobs(&mut self, height: u16) {
        let available = height.saturating_sub(3 + 12 + 1 + 2) as usize;
        let per_column = (available / 7).max(1);
        self.max_jobs = per_column * 2;
    }

    pub fn fetch_data(&mut self) {
        self.data = slurm::fetch_all(self.max_jobs, &self.login_node);
    }

    // ── Modal openers ──

    pub fn open_submit(&mut self) {
        let body = vec![
            (format!("cwd: {}", self.cwd), BodyStyle::Gray),
            ("Enter sbatch args (Esc to cancel)".into(), BodyStyle::Dim),
        ];
        let mut modal = Modal::new(ModalKind::Submit, " Submit Job ", ":sbatch ").with_body(body);
        let hist = self.history.get(" Submit Job ");
        modal.hist_index = hist.len();
        self.modal = Some(modal);
    }

    pub fn open_cancel(&mut self) {
        let output = slurm::fetch_user_jobs();
        if output.is_empty() || output.starts_with("Error") {
            self.flash_message(" Cancel Job ", "No jobs to cancel", MsgStyle::Yellow);
            return;
        }
        let mut body: Vec<(String, BodyStyle)> =
            vec![("Enter JobID or 'all' (Esc to cancel)".into(), BodyStyle::Dim)];
        for line in output.lines() {
            let parts: Vec<&str> = line.trim().split('|').collect();
            if parts.len() >= 3 {
                let style = match parts[2] {
                    "RUNNING" => BodyStyle::Blue,
                    "PENDING" => BodyStyle::Yellow,
                    _ => BodyStyle::Fg,
                };
                body.push((format!("  {:>8}  {:<20} {}", parts[0], parts[1], parts[2]), style));
            }
        }
        let mut modal = Modal::new(ModalKind::Cancel, " Cancel Job ", ":cancel ").with_body(body);
        let hist = self.history.get(" Cancel Job ");
        modal.hist_index = hist.len();
        self.modal = Some(modal);

    }

    pub fn open_chdir(&mut self) {
        let body = vec![
            (format!("Current: {}", self.cwd), BodyStyle::Gray),
            ("Enter path (Tab to complete, Esc to cancel)".into(), BodyStyle::Dim),
        ];
        let mut modal = Modal::new(ModalKind::Chdir, " Change Directory ", ":cd ").with_body(body);
        let hist = self.history.get(" Change Directory ");
        modal.hist_index = hist.len();
        self.modal = Some(modal);

    }

    // ── Modal submission handlers ──

    pub fn handle_modal_submit(&mut self) {
        let modal = match self.modal.take() {
            Some(m) => m,
            None => return,
        };
        let val = match modal.submit() {
            Some(v) => v,
            None => return,
        };

        match modal.kind {
            ModalKind::Submit => {
                self.history.push(" Submit Job ", val.clone());
                let (ok, stdout, stderr) = slurm::submit_job(&val, &self.cwd);
                if ok {
                    self.flash_message(" Submit Job ", &stdout, MsgStyle::Green);
                } else {
                    self.flash_message(" Submit Job ", &format!("Error: {stderr}"), MsgStyle::Red);
                }
            }
            ModalKind::Cancel => {
                if val.eq_ignore_ascii_case("all") {
                    // Open confirmation
                    let body = vec![("This will cancel every job you own.".into(), BodyStyle::Red)];
                    let modal = Modal::new(ModalKind::CancelConfirm, " Cancel Job ", ":cancel ALL jobs? [y/N] ")
                        .with_body(body);
                    self.modal = Some(modal);
            
                    return;
                }
                self.history.push(" Cancel Job ", val.clone());
                let (ok, _, stderr) = slurm::cancel_job(&val);
                if ok {
                    self.flash_message(" Cancel Job ", "Job(s) cancelled", MsgStyle::Green);
                } else {
                    self.flash_message(" Cancel Job ", &format!("Error: {stderr}"), MsgStyle::Red);
                }
            }
            ModalKind::CancelConfirm => {
                if val.trim().eq_ignore_ascii_case("y") {
                    let (ok, _, stderr) = slurm::cancel_job("all");
                    if ok {
                        self.flash_message(" Cancel Job ", "Job(s) cancelled", MsgStyle::Green);
                    } else {
                        self.flash_message(" Cancel Job ", &format!("Error: {stderr}"), MsgStyle::Red);
                    }
                }
                // else: dismissed
            }
            ModalKind::Chdir => {
                self.history.push(" Change Directory ", val.clone());
                let new_dir = if val.starts_with('~') {
                    if let Some(home) = dirs::home_dir() {
                        home.to_string_lossy().to_string() + &val[1..]
                    } else {
                        val.clone()
                    }
                } else {
                    val.clone()
                };
                let path = if std::path::Path::new(&new_dir).is_absolute() {
                    std::path::PathBuf::from(&new_dir)
                } else {
                    std::path::PathBuf::from(&self.cwd).join(&new_dir)
                };
                let path = match path.canonicalize() {
                    Ok(p) => p,
                    Err(_) => path,
                };
                if path.is_dir() {
                    self.cwd = path.to_string_lossy().to_string();
                    self.flash_message(" Change Directory ", &format!("cwd: {}", self.cwd), MsgStyle::Green);
                } else {
                    self.flash_message(
                        " Change Directory ",
                        &format!("Not a directory: {}", path.display()),
                        MsgStyle::Red,
                    );
                }
            }
        }
    }

    fn flash_message(&mut self, title: &str, msg: &str, style: MsgStyle) {
        let mut modal = Modal::new(ModalKind::Submit, title, ""); // kind doesn't matter for message
        modal.set_message(msg.to_string(), style);
        self.modal = Some(modal);
        self.msg_deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));

    }

    pub fn dismiss_modal(&mut self) {
        self.modal = None;
        self.msg_deadline = None;

    }

    pub fn has_timed_message(&self) -> bool {
        self.msg_deadline.is_some()
    }

    pub fn check_message_timeout(&mut self) -> bool {
        if let Some(deadline) = self.msg_deadline {
            if std::time::Instant::now() >= deadline {
                self.dismiss_modal();
                return true;
            }
        }
        false
    }

    // ── History navigation ──

    pub fn history_up(&mut self) {
        if let Some(ref mut modal) = self.modal {
            let hist = self.history.get(&modal.title);
            if hist.is_empty() || modal.hist_index == 0 {
                return;
            }
            if modal.hist_index == hist.len() {
                modal.saved_buf = modal.buf.clone();
            }
            modal.hist_index -= 1;
            modal.buf = hist[modal.hist_index].clone();
            modal.cursor = modal.buf.len();
        }
    }

    pub fn history_down(&mut self) {
        if let Some(ref mut modal) = self.modal {
            let hist = self.history.get(&modal.title);
            if modal.hist_index >= hist.len() {
                return;
            }
            modal.hist_index += 1;
            if modal.hist_index < hist.len() {
                modal.buf = hist[modal.hist_index].clone();
            } else {
                modal.buf = modal.saved_buf.clone();
            }
            modal.cursor = modal.buf.len();
        }
    }
}
