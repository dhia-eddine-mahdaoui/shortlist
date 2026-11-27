//! Everything Shortlist keeps: jobs, the profile, saved searches, and the live scan
//! that the Chrome extension is working on. Saved as one JSON file in the user's app data
//! folder (%APPDATA%\Shortlist on Windows, ~/Library/Application Support/Shortlist on macOS).

use crate::digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const STATUSES: &[&str] = &["new", "saved", "applying", "applied", "interview", "offer", "rejected", "archived"];

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Strength {
    pub kind: String,
    pub text: String,
    pub action: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Job {
    pub id: String,
    pub url: String,
    pub title: String,
    pub company: String,
    pub company_url: String,
    pub logo: String,
    pub location: String,
    pub workplace: String,
    pub salary: String,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    pub currency: String,
    pub posted: String,
    pub applicants: String,
    pub seniority: String,
    pub employment: String,
    pub description: String,
    pub source: String,
    pub found_at: u64,
    pub details: bool,
    // Filled by the AI.
    pub score: Option<u32>,
    pub fit: String,
    pub strengths: Vec<Strength>,
    pub gaps: Vec<String>,
    pub scored_at: u64,
    // Managed by the user.
    pub status: String,
    pub starred: bool,
    pub notes: String,
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Search {
    pub name: String,
    pub keywords: String,
    pub location: String,
    pub remote: bool,
    pub days: u32,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Data {
    pub jobs: Vec<Job>,
    pub summary: String,
    pub summary_at: u64,
    pub preferences: String,
    pub searches: Vec<Search>,
    pub scan_limit: u32,
}

#[derive(Clone)]
pub struct Scan {
    pub id: u64,
    pub label: String,
    pub state: String, // queued | running | done | error | cancelled
    pub message: String,
    pub found: u32,
    pub added: u32,
    pub done: u32,
    pub total: u32,
    pub cancel: bool,
    pub last_event: u64,
}

impl Scan {
    pub fn active(&self) -> bool {
        (self.state == "queued" || self.state == "running") && now().saturating_sub(self.last_event) < 180
    }
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "label": self.label, "state": self.state, "message": self.message,
                "found": self.found, "added": self.added, "done": self.done, "total": self.total })
    }
}

#[derive(Default)]
pub struct Bridge {
    pub tasks: VecDeque<Value>,
    pub scan: Option<Scan>,
    pub last_seen: u64,
    pub seq: u64,
}

pub struct Shared {
    pub data: Mutex<Data>,
    pub bridge: Mutex<Bridge>,
    pub cv: Condvar,
    pub dir: PathBuf,
    pub app: AppHandle,
}

static S: OnceLock<Shared> = OnceLock::new();

pub fn init(app: AppHandle, dir: PathBuf) {
    let _ = std::fs::create_dir_all(dir.join("profile"));
    let mut data: Data = std::fs::read(dir.join("data.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if data.scan_limit == 0 {
        data.scan_limit = 30;
    }
    let _ = S.set(Shared { data: Mutex::new(data), bridge: Mutex::new(Bridge::default()), cv: Condvar::new(), dir, app });
}

pub fn s() -> &'static Shared {
    S.get().expect("store not initialised")
}

pub fn data() -> MutexGuard<'static, Data> {
    s().data.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn bridge() -> MutexGuard<'static, Bridge> {
    s().bridge.lock().unwrap_or_else(|e| e.into_inner())
}

/// Tells the window to refresh.
pub fn notify() {
    let _ = s().app.emit("changed", ());
}

/// Writes the data file (atomically) and refreshes the window.
pub fn save(d: &Data) {
    let path = s().dir.join("data.json");
    let tmp = s().dir.join("data.json.tmp");
    if let Ok(bytes) = serde_json::to_vec(d) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    notify();
}

pub fn docs_dir() -> PathBuf {
    s().dir.join("profile")
}

pub struct Doc {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified: u64,
}

pub fn docs() -> Vec<Doc> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(docs_dir()) {
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            out.push(Doc { name: e.file_name().to_string_lossy().into_owned(), path: e.path(), size: meta.len(), modified });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// True when the documents changed after the AI last summarised them.
pub fn summary_stale(d: &Data) -> bool {
    let docs = docs();
    if docs.is_empty() {
        return false;
    }
    d.summary.trim().is_empty() || docs.iter().any(|x| x.modified > d.summary_at)
}

pub fn ext_connected() -> bool {
    now().saturating_sub(bridge().last_seen) < 45
}

fn text(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.split_whitespace().collect::<Vec<_>>().join(" "),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Adds or refreshes jobs sent by the extension. Keeps the user's status, notes and the AI's score.
pub fn merge_jobs(list: &[Value], source: &str) -> (u32, u32) {
    let mut d = data();
    let (mut added, mut updated) = (0, 0);
    for v in list {
        let id = text(v, "id");
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let idx = match d.jobs.iter().position(|j| j.id == id) {
            Some(i) => {
                updated += 1;
                i
            }
            None => {
                added += 1;
                d.jobs.push(Job {
                    url: format!("https://www.linkedin.com/jobs/view/{id}/"),
                    id: id.clone(),
                    status: "new".into(),
                    source: source.to_string(),
                    found_at: now(),
                    ..Default::default()
                });
                d.jobs.len() - 1
            }
        };
        let j = &mut d.jobs[idx];
        macro_rules! take {
            ($field:ident, $key:expr) => {{
                let t = text(v, $key);
                if !t.is_empty() {
                    j.$field = t;
                }
            }};
        }
        take!(title, "title");
        take!(company, "company");
        take!(company_url, "company_url");
        take!(logo, "logo");
        take!(location, "location");
        take!(workplace, "workplace");
        take!(salary, "salary");
        take!(posted, "posted");
        take!(applicants, "applicants");
        take!(seniority, "seniority");
        take!(employment, "employment");
        if let Some(desc) = v.get("description").and_then(|x| x.as_str()) {
            if desc.trim().len() > 40 {
                j.description = desc.trim().to_string();
            }
        }
        if v.get("details").and_then(|x| x.as_bool()) == Some(true) && !j.description.is_empty() {
            j.details = true;
        }
        // "London (Hybrid)" -> location "London", workplace "Hybrid".
        for w in ["On-site", "Remote", "Hybrid"] {
            let tag = format!("({w})");
            if j.location.contains(&tag) {
                j.location = j.location.replace(&tag, "").trim().to_string();
                if j.workplace.is_empty() {
                    j.workplace = w.to_string();
                }
            }
        }
        if j.workplace.is_empty() {
            j.workplace = digest::guess_workplace(&j.location, &j.title, &j.description);
        }
        if j.salary.is_empty() {
            if let Some(s) = digest::salary_in_text(&j.description) {
                j.salary = s;
            }
        }
        match digest::parse_salary(&j.salary) {
            Some((lo, hi, cur)) => {
                j.salary_min = Some(lo);
                j.salary_max = Some(hi);
                j.currency = cur;
            }
            None => {
                j.salary_min = None;
                j.salary_max = None;
            }
        }
        j.updated_at = now();
    }
    save(&d);
    (added, updated)
}

/* ------------------------------------------------------------------ scans */

pub fn start_scan(params: &Value) -> Result<Value, String> {
    let mode = if params.get("mode").and_then(|v| v.as_str()) == Some("search") { "search" } else { "recommended" };
    let keywords = text(params, "keywords");
    let location = text(params, "location");
    if mode == "search" && keywords.is_empty() {
        return Err("A search needs keywords.".into());
    }
    let (limit, skip) = {
        let d = data();
        let limit = params.get("limit").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(d.scan_limit).clamp(1, 100);
        // Jobs we already have in full are not fetched again.
        let skip: Vec<String> = d.jobs.iter().filter(|j| j.details).map(|j| j.id.clone()).collect();
        (limit, skip)
    };
    let remote = params.get("remote").and_then(|v| v.as_bool()).unwrap_or(false);
    let days = params.get("days").and_then(|v| v.as_u64()).unwrap_or(0).min(60);
    let label = if mode == "search" {
        let mut l = format!("Search: {keywords}");
        if !location.is_empty() {
            l += &format!(" in {location}");
        }
        if remote {
            l += " (remote)";
        }
        l
    } else {
        "LinkedIn preferences".to_string()
    };

    let mut b = bridge();
    if let Some(sc) = &b.scan {
        if sc.active() {
            return Err(format!("A scan is already running ({}).", sc.label));
        }
    }
    b.seq += 1;
    let id = b.seq;
    b.tasks.retain(|t| t["kind"] != "scan");
    b.tasks.push_back(json!({
        "kind": "scan", "id": id, "label": label,
        "params": { "mode": mode, "keywords": keywords, "location": location, "remote": remote, "days": days, "limit": limit },
        "skip": skip,
    }));
    let scan = Scan { id, label, state: "queued".into(), message: "Waiting for Chrome".into(), found: 0, added: 0, done: 0, total: limit, cancel: false, last_event: now() };
    let out = scan.to_json();
    b.scan = Some(scan);
    drop(b);
    s().cv.notify_all();
    notify();
    Ok(out)
}

pub fn cancel_scan() {
    let mut b = bridge();
    b.tasks.retain(|t| t["kind"] != "scan");
    if let Some(sc) = b.scan.as_mut() {
        if sc.state == "queued" || sc.state == "running" {
            sc.cancel = true;
            sc.state = "cancelled".into();
            sc.message = "Cancelled".into();
        }
    }
    drop(b);
    s().cv.notify_all();
    notify();
}

pub fn scan_json() -> Value {
    bridge().scan.as_ref().map(|s| s.to_json()).unwrap_or(Value::Null)
}

/// Waits until the current scan ends or `secs` pass. Returns right away when Chrome isn't connected.
pub fn wait_scan(secs: u64) -> Option<Scan> {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(secs);
    let mut b = bridge();
    loop {
        let sc = b.scan.clone()?;
        if !sc.active() {
            return Some(sc);
        }
        let connected = now().saturating_sub(b.last_seen) < 45;
        // Give a just-woken extension a few seconds to pick the task up.
        if sc.state == "queued" && !connected && start.elapsed() > Duration::from_secs(4) {
            return Some(sc);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Some(sc);
        }
        b = s().cv.wait_timeout(b, left.min(Duration::from_secs(5))).unwrap_or_else(|e| e.into_inner()).0;
    }
}
