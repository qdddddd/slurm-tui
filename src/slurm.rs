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
pub struct PartitionStats {
    pub partition: String,
    pub total_nodes: u32,
    pub idle_nodes: u32,
    pub mix_nodes: u32,
    pub alloc_nodes: u32,
    pub down_nodes: u32,
    pub other_nodes: u32,
    pub cpu_idle: u64,
    pub cpu_total: u64,
    pub mem_free_mb: u64,
    pub mem_total_mb: u64,
    pub gpu_free: u64,
    pub gpu_total: u64,
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
    pub partition_stats: Vec<PartitionStats>,
    pub idle_nodes: usize,
    pub mix_nodes: usize,
    pub alloc_nodes: usize,
    pub down_nodes: usize,
    pub job_details: Vec<JobDetail>,
    pub running_total: usize,
}

// ── Fetching ──

pub fn fetch_all(max_jobs: usize, login_node: &str) -> SlurmData {
    // Run squeue and scontrol in parallel
    let login1 = login_node.to_string();
    let squeue_handle = thread::spawn(move || {
        run_cmd(&["squeue", "-h", "-o", "%i|%u|%j|%P|%T|%M|%D|%R"])
    });
    let scontrol_handle = thread::spawn(|| {
        run_cmd(&["scontrol", "-d", "-o", "show", "node"])
    });

    let squeue_out = squeue_handle.join().unwrap_or_default();
    let scontrol_out = scontrol_handle.join().unwrap_or_default();

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

    let (partition_stats, idle_nodes, mix_nodes, alloc_nodes, down_nodes) =
        parse_node_stats(&scontrol_out);

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
        let fields = parse_scontrol(&raw);
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
                parsed[i].0.tail = resolve_cr(&tail);
            }
        }
    }

    let job_details: Vec<JobDetail> = parsed.into_iter().map(|(d, _)| d).collect();

    SlurmData {
        queue_jobs,
        partition_stats,
        idle_nodes,
        mix_nodes,
        alloc_nodes,
        down_nodes,
        job_details,
        running_total,
    }
}

#[derive(Default)]
struct PartitionStatsAccum {
    total_nodes: u32,
    idle_nodes: u32,
    mix_nodes: u32,
    alloc_nodes: u32,
    down_nodes: u32,
    other_nodes: u32,
    cpu_idle: u64,
    cpu_total: u64,
    mem_free_mb: u64,
    mem_total_mb: u64,
    gpu_free: u64,
    gpu_total: u64,
}

enum StateCat {
    Idle,
    Mix,
    Alloc,
    Down,
    Other,
}

fn categorize_state(state: &str) -> StateCat {
    let st = state.to_uppercase();
    if st.contains("DRAIN")
        || st.contains("DOWN")
        || st.contains("FAIL")
        || st.contains("NOT_RESPOND")
        || st.contains("MAINT")
        || st.contains("POWER_DOWN")
        || st.contains("POWERED_DOWN")
    {
        StateCat::Down
    } else if st.contains("MIX") {
        StateCat::Mix
    } else if st.contains("ALLOCATED") {
        StateCat::Alloc
    } else if st.contains("IDLE") {
        StateCat::Idle
    } else {
        StateCat::Other
    }
}

fn parse_gres_count(gres: &str, kind: &str) -> u64 {
    if gres.is_empty() || gres == "(null)" {
        return 0;
    }
    let mut total = 0u64;
    for item in gres.split(',') {
        let item = item.split('(').next().unwrap_or(item).trim();
        if item.is_empty() {
            continue;
        }
        let parts: Vec<&str> = item.split(':').collect();
        if parts.is_empty() || parts[0] != kind {
            continue;
        }
        if let Some(last) = parts.last() {
            if let Ok(n) = last.parse::<u64>() {
                total += n;
            }
        }
    }
    total
}

fn parse_node_stats(
    raw: &str,
) -> (Vec<PartitionStats>, usize, usize, usize, usize) {
    if raw.is_empty() || raw.starts_with("Error") {
        return (Vec::new(), 0, 0, 0, 0);
    }

    let mut per_partition: HashMap<String, PartitionStatsAccum> = HashMap::new();
    let mut idle_nodes = 0usize;
    let mut mix_nodes = 0usize;
    let mut alloc_nodes = 0usize;
    let mut down_nodes = 0usize;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields = parse_scontrol(line);
        let partitions = match fields.get("Partitions") {
            Some(p) if !p.is_empty() => p.clone(),
            _ => continue,
        };
        let state = fields.get("State").cloned().unwrap_or_default();
        let cpu_alloc: u64 = fields.get("CPUAlloc").and_then(|s| s.parse().ok()).unwrap_or(0);
        let cpu_tot: u64 = fields.get("CPUTot").and_then(|s| s.parse().ok()).unwrap_or(0);
        let mem_tot: u64 = fields.get("RealMemory").and_then(|s| s.parse().ok()).unwrap_or(0);
        let mem_alloc: u64 = fields.get("AllocMem").and_then(|s| s.parse().ok()).unwrap_or(0);
        let gres = fields.get("Gres").cloned().unwrap_or_default();
        let gres_used = fields.get("GresUsed").cloned().unwrap_or_default();

        let cpu_idle = cpu_tot.saturating_sub(cpu_alloc);
        let mem_free_mb = mem_tot.saturating_sub(mem_alloc);
        let gpu_total = parse_gres_count(&gres, "gpu");
        let gpu_used = parse_gres_count(&gres_used, "gpu");
        let gpu_free = gpu_total.saturating_sub(gpu_used);

        let category = categorize_state(&state);
        match category {
            StateCat::Idle => idle_nodes += 1,
            StateCat::Mix => mix_nodes += 1,
            StateCat::Alloc => alloc_nodes += 1,
            StateCat::Down => down_nodes += 1,
            StateCat::Other => {}
        }
        // Down nodes are unusable; don't count their resources as free.
        let counts_as_free = !matches!(category, StateCat::Down);

        for partition in partitions.split(',').filter(|s| !s.is_empty()) {
            let entry = per_partition
                .entry(partition.to_string())
                .or_default();
            entry.total_nodes += 1;
            match category {
                StateCat::Idle => entry.idle_nodes += 1,
                StateCat::Mix => entry.mix_nodes += 1,
                StateCat::Alloc => entry.alloc_nodes += 1,
                StateCat::Down => entry.down_nodes += 1,
                StateCat::Other => entry.other_nodes += 1,
            }
            entry.cpu_total += cpu_tot;
            entry.mem_total_mb += mem_tot;
            entry.gpu_total += gpu_total;
            if counts_as_free {
                entry.cpu_idle += cpu_idle;
                entry.mem_free_mb += mem_free_mb;
                entry.gpu_free += gpu_free;
            }
        }
    }

    let mut stats: Vec<PartitionStats> = per_partition
        .into_iter()
        .map(|(name, a)| PartitionStats {
            partition: name,
            total_nodes: a.total_nodes,
            idle_nodes: a.idle_nodes,
            mix_nodes: a.mix_nodes,
            alloc_nodes: a.alloc_nodes,
            down_nodes: a.down_nodes,
            other_nodes: a.other_nodes,
            cpu_idle: a.cpu_idle,
            cpu_total: a.cpu_total,
            mem_free_mb: a.mem_free_mb,
            mem_total_mb: a.mem_total_mb,
            gpu_free: a.gpu_free,
            gpu_total: a.gpu_total,
        })
        .collect();
    stats.sort_by(|a, b| a.partition.cmp(&b.partition));

    (stats, idle_nodes, mix_nodes, alloc_nodes, down_nodes)
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

/// Strip ANSI escape sequences (CSI codes like `\x1b[0;128;0m`).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Consume CSI sequence: ESC [ ... final_byte
            if let Some(next) = chars.next() {
                if next == '[' {
                    // Skip until we hit a letter (0x40..=0x7E)
                    for ch in chars.by_ref() {
                        if ch.is_ascii_alphabetic() || ch == 'm' {
                            break;
                        }
                    }
                }
                // else: non-CSI escape, just skip the two chars
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Process carriage returns in tail output and strip ANSI codes.
/// Progress bars (tqdm, etc.) use \r to overwrite the same line, so `tail`
/// returns one huge line with all updates concatenated.  Split on \r and
/// keep only the last non-empty segment of each line.
fn resolve_cr(raw: &str) -> String {
    let cleaned = strip_ansi(raw);
    cleaned
        .lines()
        .map(|line| {
            if line.contains('\r') {
                line.rsplit('\r')
                    .find(|s| !s.is_empty())
                    .unwrap_or("")
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fetch user's jobs for cancel dialog.
pub fn fetch_user_jobs() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    run_cmd(&["squeue", "-u", &user, "-h", "-o", "%i|%j|%T"])
}

/// Fetch recent tasks for the current user using `sacct`.
/// Returns lines in the form `JOBID|NAME|STATE`, newest first,
/// limited to `limit` distinct top-level jobs (no steps).
pub fn fetch_recent_tasks(limit: usize) -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let raw = run_cmd(&[
        "sacct",
        "-u",
        &user,
        "-X",
        "-n",
        "-P",
        "-o",
        "JobIDRaw,JobName,State",
    ]);
    if raw.is_empty() || raw.starts_with("Error") {
        return raw;
    }

    let mut lines: Vec<&str> = raw.lines().collect();
    lines.reverse();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out_lines: Vec<String> = Vec::new();

    for line in lines {
        let parts: Vec<&str> = line.trim().split('|').collect();
        if parts.len() < 3 {
            continue;
        }
        let job_id = parts[0].trim();
        let name = parts[1].trim();
        let state = parts[2].trim();

        if job_id.is_empty() || job_id.contains('.') {
            continue;
        }
        if !seen.insert(job_id.to_string()) {
            continue;
        }
        out_lines.push(format!("{}|{}|{}", job_id, name, state));
        if out_lines.len() >= limit {
            break;
        }
    }

    out_lines.join("\n")
}

/// Resolve stdout path for a given job ID.
pub fn resolve_job_stdout(job_id: &str) -> Result<String, String> {
    fetch_job_stdout(job_id)
}

fn fetch_job_stdout(job_id: &str) -> Result<String, String> {
    let raw = run_cmd(&["scontrol", "show", "job", job_id]);
    if !raw.is_empty() && !raw.starts_with("Error") {
        let fields = parse_scontrol(&raw);
        if let Some(stdout) = fields.get("StdOut") {
            if !stdout.is_empty() && stdout != "N/A" {
                let job_name = fields.get("JobName").map(String::as_str).unwrap_or("");
                return Ok(expand_stdout_path(stdout, job_id, job_name));
            }
        }
    }

    let raw = run_cmd(&[
        "sacct",
        "-j",
        job_id,
        "-X",
        "-n",
        "-P",
        "-o",
        "JobIDRaw,JobName,StdOut",
    ]);
    if raw.is_empty() || raw.starts_with("Error") {
        return Err(format!("Unable to resolve StdOut for job {job_id}"));
    }

    for line in raw.lines() {
        let parts: Vec<&str> = line.trim().split('|').collect();
        if parts.len() < 3 {
            continue;
        }
        let id = parts[0].trim();
        let job_name = parts[1].trim();
        let stdout = parts[2].trim();
        if id == job_id && !stdout.is_empty() && stdout != "Unknown" && stdout != "N/A" {
            return Ok(expand_stdout_path(stdout, job_id, job_name));
        }
    }

    Err(format!("No StdOut configured for job {job_id}"))
}

fn expand_stdout_path(stdout: &str, job_id: &str, job_name: &str) -> String {
    stdout
        .replace("%j", job_id)
        .replace("%A", job_id)
        .replace("%x", job_name)
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
