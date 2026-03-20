use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use std::thread;

static UID_CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn ensure_cache() {
    let mut lock = UID_CACHE.lock().unwrap();
    if lock.is_none() {
        *lock = Some(HashMap::new());
    }
}

fn cache_get(key: &str) -> Option<String> {
    let lock = UID_CACHE.lock().unwrap();
    lock.as_ref().and_then(|c| c.get(key).cloned())
}

fn cache_set(key: &str, val: &str) {
    let mut lock = UID_CACHE.lock().unwrap();
    if let Some(ref mut c) = *lock {
        c.insert(key.to_string(), val.to_string());
    }
}

pub fn run_cmd(cmd: &[&str]) -> String {
    let result = Command::new(cmd[0]).args(&cmd[1..]).output();
    match result {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(e) => format!("Error: {e}"),
    }
}

pub fn run_cmd_cwd(cmd: &[&str], cwd: &str) -> (bool, String, String) {
    let result = Command::new(cmd[0])
        .args(&cmd[1..])
        .current_dir(cwd)
        .output();
    match result {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
            String::from_utf8_lossy(&o.stderr).trim().to_string(),
        ),
        Err(e) => (false, String::new(), format!("Error: {e}")),
    }
}

pub fn resolve_user(name_or_uid: &str, login_node: &str) -> String {
    if name_or_uid.is_empty() || !name_or_uid.chars().all(|c| c.is_ascii_digit()) {
        return name_or_uid.to_string();
    }
    ensure_cache();
    if let Some(cached) = cache_get(name_or_uid) {
        return cached;
    }
    if !login_node.is_empty() {
        let out = run_cmd(&[
            "ssh", "-o", "ConnectTimeout=2", "-o", "BatchMode=yes",
            login_node, "getent", "passwd", name_or_uid,
        ]);
        if !out.starts_with("Error") && !out.is_empty() {
            if let Some(username) = out.split(':').next() {
                cache_set(name_or_uid, username);
                return username.to_string();
            }
        }
    }
    cache_set(name_or_uid, name_or_uid);
    name_or_uid.to_string()
}

/// Parse scontrol UserId field like "name(uid)" into the cache.
pub fn cache_userid(userid_field: &str) {
    if let Some(paren) = userid_field.find('(') {
        let name = &userid_field[..paren];
        let uid = userid_field[paren + 1..].trim_end_matches(')');
        if !uid.is_empty()
            && uid.chars().all(|c| c.is_ascii_digit())
            && !name.is_empty()
            && !name.chars().all(|c| c.is_ascii_digit())
        {
            ensure_cache();
            cache_set(uid, name);
        }
    }
}

// ── Data structures ──

#[derive(Clone, Default)]
pub struct QueueJob {
    pub job_id: String,
    pub user: String,
    pub name: String,
    pub partition: String,
    pub state: String,
    pub time: String,
    pub nodes: String,
    pub nodelist: String,
}

#[derive(Clone, Default)]
pub struct NodeInfo {
    pub partition: String,
    pub avail: String,
    pub nodes: String,
    pub state: String,
    pub nodelist: String,
}

#[derive(Clone, Default)]
pub struct JobDetail {
    pub job_id: String,
    pub name: String,
    pub user: String,
    pub node: String,
    pub elapsed: String,
    pub timelimit: String,
    pub cpus: String,
    pub mem: String,
    pub gpu: String,
    pub stdout: String,
    pub tail: String,
}

#[derive(Clone, Default)]
pub struct SlurmData {
    pub queue_jobs: Vec<QueueJob>,
    pub node_infos: Vec<NodeInfo>,
    pub job_details: Vec<JobDetail>,
    pub running_total: usize,
}

// ── Fetching ──

pub fn fetch_all(max_jobs: usize, login_node: &str) -> SlurmData {
    // Run squeue and sinfo in parallel
    let login1 = login_node.to_string();
    let squeue_handle = thread::spawn(move || {
        run_cmd(&["squeue", "-h", "-o", "%i|%u|%j|%P|%T|%M|%D|%R"])
    });
    let sinfo_handle = thread::spawn(|| {
        run_cmd(&["sinfo", "-h", "-o", "%P|%a|%D|%t|%N"])
    });

    let squeue_out = squeue_handle.join().unwrap_or_default();
    let sinfo_out = sinfo_handle.join().unwrap_or_default();

    // Parse queue jobs
    let mut queue_jobs = Vec::new();
    let mut running_jobs: Vec<(String, String)> = Vec::new();
    if !squeue_out.is_empty() && !squeue_out.starts_with("Error") {
        for line in squeue_out.lines() {
            let parts: Vec<&str> = line.trim().split('|').collect();
            if parts.len() >= 8 {
                let user = resolve_user(parts[1], &login1);
                let state = parts[4].to_string();
                if state == "RUNNING" {
                    running_jobs.push((parts[0].to_string(), user.clone()));
                }
                queue_jobs.push(QueueJob {
                    job_id: parts[0].to_string(),
                    user,
                    name: parts[2].to_string(),
                    partition: parts[3].to_string(),
                    state,
                    time: parts[5].to_string(),
                    nodes: parts[6].to_string(),
                    nodelist: shorten_reason(parts[7]),
                });
            }
        }
    }

    // Parse node info
    let mut node_infos = Vec::new();
    if !sinfo_out.is_empty() && !sinfo_out.starts_with("Error") {
        for line in sinfo_out.lines() {
            let parts: Vec<&str> = line.trim().split('|').collect();
            if parts.len() >= 5 {
                node_infos.push(NodeInfo {
                    partition: parts[0].to_string(),
                    avail: parts[1].to_string(),
                    nodes: parts[2].to_string(),
                    state: parts[3].to_string(),
                    nodelist: parts[4].to_string(),
                });
            }
        }
    }

    // Sort running jobs: current user first
    let current_user = std::env::var("USER").unwrap_or_default();
    running_jobs.sort_by_key(|(_, u)| if *u == current_user { 0 } else { 1 });
    let running_total = running_jobs.len();

    // Fetch scontrol details in parallel
    let job_ids: Vec<String> = running_jobs.iter().take(max_jobs).map(|(id, _)| id.clone()).collect();
    let job_user_map: HashMap<String, String> = running_jobs.iter().take(max_jobs).cloned().collect();

    let detail_handles: Vec<_> = job_ids
        .iter()
        .map(|jid| {
            let jid = jid.clone();
            thread::spawn(move || {
                let out = run_cmd(&["scontrol", "show", "job", &jid]);
                (jid, out)
            })
        })
        .collect();

    let mut details_raw: HashMap<String, String> = HashMap::new();
    for h in detail_handles {
        if let Ok((jid, out)) = h.join() {
            details_raw.insert(jid, out);
        }
    }

    // Parse details and fetch tails in parallel
    let mut parsed: Vec<(JobDetail, Option<String>)> = Vec::new();
    for jid in &job_ids {
        let raw = match details_raw.get(jid) {
            Some(r) if !r.starts_with("Error") => r,
            _ => continue,
        };
        let fields = parse_scontrol(raw);
        let name = fields.get("JobName").cloned().unwrap_or_else(|| "N/A".into());
        let raw_userid = fields.get("UserId").cloned().unwrap_or_else(|| "N/A".into());
        cache_userid(&raw_userid);
        let user = resolve_user(raw_userid.split('(').next().unwrap_or("N/A"), &login1);
        let node = fields.get("NodeList").cloned().unwrap_or_else(|| "N/A".into());
        let elapsed = fields.get("RunTime").cloned().unwrap_or_else(|| "N/A".into());
        let timelimit = fields.get("TimeLimit").cloned().unwrap_or_else(|| "N/A".into());
        let cpus = fields.get("NumCPUs").cloned().unwrap_or_else(|| "N/A".into());
        let mem = fields
            .get("MinMemoryNode")
            .or_else(|| fields.get("MinMemoryCPU"))
            .cloned()
            .unwrap_or_else(|| "N/A".into());
        let gpu = extract_gpu(&fields);
        let stdout = fields.get("StdOut").cloned().unwrap_or_default();

        parsed.push((
            JobDetail {
                job_id: jid.clone(),
                name,
                user,
                node,
                elapsed,
                timelimit,
                cpus,
                mem,
                gpu,
                stdout: stdout.clone(),
                tail: String::new(),
            },
            if stdout.is_empty() || stdout == "N/A" {
                None
            } else {
                Some(stdout)
            },
        ));
    }

    // Tail log files in parallel
    let tail_handles: Vec<_> = parsed
        .iter()
        .enumerate()
        .filter_map(|(i, (detail, stdout_path))| {
            let path = stdout_path.as_ref()?;
            let n = if job_user_map.get(&detail.job_id).map_or(false, |u| *u == current_user) {
                "6"
            } else {
                "3"
            };
            let path = path.clone();
            let n = n.to_string();
            Some((i, thread::spawn(move || run_cmd(&["tail", &format!("-{n}"), &path]))))
        })
        .collect();

    for (i, h) in tail_handles {
        if let Ok(tail) = h.join() {
            if !tail.starts_with("Error") {
                parsed[i].0.tail = tail;
            }
        }
    }

    let job_details: Vec<JobDetail> = parsed.into_iter().map(|(d, _)| d).collect();

    SlurmData {
        queue_jobs,
        node_infos,
        job_details,
        running_total,
    }
}

fn parse_scontrol(raw: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for token in raw.replace('\n', " ").split_whitespace() {
        if let Some((k, v)) = token.split_once('=') {
            fields.insert(k.to_string(), v.to_string());
        }
    }
    fields
}

fn extract_gpu(fields: &HashMap<String, String>) -> String {
    for key in &["TresPerNode", "TresPerJob", "TresPerSocket", "TresPerTask"] {
        if let Some(val) = fields.get(*key) {
            if val.to_lowercase().contains("gpu") {
                for item in val.split(',') {
                    if item.to_lowercase().contains("gpu") {
                        if let Some(count) = item.rsplit(':').next() {
                            return count.to_string();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

fn shorten_reason(reason: &str) -> String {
    if !reason.starts_with('(') {
        return reason.to_string();
    }
    let inner = reason.trim_start_matches('(').trim_end_matches(')');
    if inner.contains("ReqNodeNotAvail") {
        "NodeNA".into()
    } else if inner.contains("Resources") {
        "Rsrc".into()
    } else if inner.contains("Priority") {
        "Prio".into()
    } else if inner.contains("Dependency") {
        "Dep".into()
    } else if inner.contains("QOSMax") {
        "QOS".into()
    } else {
        inner.chars().take(8).collect()
    }
}

/// Fetch user's jobs for cancel dialog.
pub fn fetch_user_jobs() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    run_cmd(&["squeue", "-u", &user, "-h", "-o", "%i|%j|%T"])
}

/// Submit a job via sbatch.
pub fn submit_job(args: &str, cwd: &str) -> (bool, String, String) {
    let mut parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        return (false, String::new(), "No arguments".into());
    }
    if parts[0] != "sbatch" {
        parts.insert(0, "sbatch");
    }
    run_cmd_cwd(&parts, cwd)
}

/// Cancel a job or all user jobs.
pub fn cancel_job(val: &str) -> (bool, String, String) {
    if val.eq_ignore_ascii_case("all") {
        let user = std::env::var("USER").unwrap_or_default();
        run_cmd_cwd(&["scancel", "-u", &user], ".")
    } else {
        run_cmd_cwd(&["scancel", val], ".")
    }
}
