use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const HISTORY_MAX: usize = 100;

pub struct InputHistory {
    path: PathBuf,
    sections: HashMap<String, Vec<String>>,
}

impl InputHistory {
    pub fn new() -> Self {
        let path = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/cache"))
            .join("slurm-tui-history");
        let mut h = Self {
            path,
            sections: HashMap::new(),
        };
        h.load();
        h
    }

    fn load(&mut self) {
        let Ok(content) = fs::read_to_string(&self.path) else {
            return;
        };
        let mut section: Option<String> = None;
        for line in content.lines() {
            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].to_string();
                self.sections.entry(name.clone()).or_default();
                section = Some(name);
            } else if let Some(ref sec) = section {
                if !line.is_empty() {
                    self.sections.entry(sec.clone()).or_default().push(line.to_string());
                }
            }
        }
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut out = String::new();
        for (section, entries) in &self.sections {
            out.push_str(&format!("[{section}]\n"));
            let start = entries.len().saturating_sub(HISTORY_MAX);
            for entry in &entries[start..] {
                out.push_str(entry);
                out.push('\n');
            }
        }
        let _ = fs::write(&self.path, out);
    }

    pub fn get(&self, section: &str) -> &[String] {
        self.sections.get(section).map_or(&[], |v| v.as_slice())
    }

    pub fn push(&mut self, section: &str, entry: String) {
        self.sections.entry(section.to_string()).or_default().push(entry);
    }
}
