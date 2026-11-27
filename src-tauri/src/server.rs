//! Local HTTP server on 127.0.0.1:12731.
//!   /mcp      MCP (streamable HTTP, JSON replies) for Claude Code, Codex and other assistants.
//!   /ext/*    The Chrome extension: picks up scan tasks and sends back the jobs it read.

use crate::digest;
use crate::store::{self, Strength, STATUSES};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

pub const PORT: u16 = 12731;

pub fn port() -> u16 {
    std::env::var("SHORTLIST_PORT").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(PORT)
}

pub fn start() {
    std::thread::spawn(|| {
        let listener = match TcpListener::bind(("127.0.0.1", port())) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Shortlist: port {} unavailable: {e}", port());
                return;
            }
        };
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
}

/// Asks the copy that is already running to bring its window forward.
pub fn ask_to_show() {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port())) else { return };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let request = "POST /show HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.read(&mut [0u8; 64]);
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn loopback_host(h: &str) -> bool {
    let h = h.trim();
    let name = if h.starts_with('[') { h.split(']').next().unwrap_or("").trim_start_matches('[') } else { h.split(':').next().unwrap_or("") };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

/// The request headers the server looks at, in lower case.
#[derive(Default)]
struct Headers {
    len: usize,
    host: String,
    origin: Option<String>,
    content_type: String,
    fetch_mode: String,
}

impl Headers {
    fn read(&mut self, line: &str) {
        let Some((name, value)) = line.split_once(':') else { return };
        let value = value.trim().to_ascii_lowercase();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => self.len = value.parse().unwrap_or(0),
            "host" => self.host = value,
            "origin" => self.origin = Some(value),
            "content-type" => self.content_type = value,
            "sec-fetch-mode" => self.fetch_mode = value,
            _ => {}
        }
    }
}

/// Why a request is turned away, if it is. Web pages must not drive the app: only command-line
/// clients, local pages, and browser extensions (for /ext/ only) get in.
fn refusal(method: &str, path: &str, h: &Headers) -> Option<&'static str> {
    // The Host check blocks DNS rebinding.
    if !loopback_host(&h.host) {
        return Some("403 Forbidden");
    }
    // Command-line clients send no Origin. "null" is refused: any website can send it
    // from a sandboxed frame.
    let origin_ok = match h.origin.as_deref() {
        None => true,
        Some(o) if o.starts_with("chrome-extension://") || o.starts_with("moz-extension://") => path.starts_with("/ext/"),
        Some(o) => loopback_host(o.split("://").nth(1).unwrap_or("")),
    };
    // Images, scripts, frames and links send no Origin on a GET, but browsers label them.
    let page_element = path != "/health" && matches!(h.fetch_mode.as_str(), "no-cors" | "navigate");
    if !origin_ok || page_element {
        return Some("403 Forbidden");
    }
    // Another site's page can only send JSON after a CORS preflight, which is never granted.
    if method == "POST" && h.content_type.split(';').next().unwrap_or("").trim() != "application/json" {
        return Some("415 Unsupported Media Type");
    }
    None
}

fn serve(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let mut headers = Headers::default();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        headers.read(l);
    }
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").split('?').next().unwrap_or("").to_string();

    if let Some(status) = refusal(&method, &path, &headers) {
        return respond(&mut stream, status, "{}");
    }

    let mut body = Vec::new();
    if method == "POST" {
        body = vec![0u8; headers.len.min(20 * 1024 * 1024)];
        if reader.read_exact(&mut body).is_err() {
            return respond(&mut stream, "400 Bad Request", "{}");
        }
    }
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

    match (method.as_str(), path.as_str()) {
        ("GET", "/health") => {
            let b = json!({ "status": "ok", "app": "Shortlist", "version": env!("CARGO_PKG_VERSION"),
                            "mcp": format!("http://127.0.0.1:{}/mcp", port()), "chrome": store::ext_connected() });
            respond(&mut stream, "200 OK", &b.to_string())
        }
        ("POST", "/show") => {
            crate::show_window(&store::s().app);
            respond(&mut stream, "200 OK", "{}")
        }
        ("GET", "/ext/status") => {
            let b = json!({ "app": "Shortlist", "scan": store::scan_json() });
            respond(&mut stream, "200 OK", &b.to_string())
        }
        ("GET", "/ext/next") => respond(&mut stream, "200 OK", &ext_next().to_string()),
        ("POST", "/ext/event") => respond(&mut stream, "200 OK", &ext_event(&parsed).to_string()),
        ("POST", "/ext/add") => {
            let jobs = parsed.get("jobs").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let (added, _) = store::merge_jobs(&jobs, "Added from Chrome");
            respond(&mut stream, "200 OK", &json!({ "added": added, "ok": true }).to_string())
        }
        ("POST", "/mcp") | ("POST", "/") => {
            if parsed.is_null() {
                return respond(&mut stream, "400 Bad Request", r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}"#);
            }
            let reply = if let Some(batch) = parsed.as_array() {
                let out: Vec<Value> = batch.iter().filter_map(rpc).collect();
                if out.is_empty() { None } else { Some(Value::Array(out)) }
            } else {
                rpc(&parsed)
            };
            match reply {
                Some(v) => respond(&mut stream, "200 OK", &v.to_string()),
                None => {
                    let _ = stream.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        }
        ("GET", "/mcp") => {
            let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nAllow: POST\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
        _ => respond(&mut stream, "404 Not Found", "{}"),
    }
}

/* ------------------------------------------------------------------ Chrome extension */

/// Long poll: hands the extension its next task, or {} after 20 s.
fn ext_next() -> Value {
    let shared = store::s();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut b = store::bridge();
    let was_connected = store::now().saturating_sub(b.last_seen) < 45;
    b.last_seen = store::now();
    if !was_connected {
        drop(b);
        store::notify();
        b = store::bridge();
    }
    loop {
        if let Some(task) = b.tasks.pop_front() {
            if task["kind"] == "scan" {
                if let Some(sc) = b.scan.as_mut() {
                    if Some(sc.id) == task["id"].as_u64() {
                        sc.state = "running".into();
                        sc.message = "Opening LinkedIn".into();
                        sc.last_event = store::now();
                    }
                }
            }
            drop(b);
            shared.cv.notify_all();
            store::notify();
            return task;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return json!({});
        }
        b = shared.cv.wait_timeout(b, left).unwrap_or_else(|e| e.into_inner()).0;
        b.last_seen = store::now();
    }
}

fn ext_event(ev: &Value) -> Value {
    let id = ev.get("scan").and_then(|v| v.as_u64()).unwrap_or(0);
    let kind = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let label = {
        let b = store::bridge();
        match &b.scan {
            Some(sc) if sc.id == id => sc.label.clone(),
            _ => return json!({ "cancel": true }),
        }
    };
    let added = if kind == "jobs" {
        let jobs = ev.get("jobs").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        store::merge_jobs(&jobs, &label).0
    } else {
        0
    };
    let mut b = store::bridge();
    b.last_seen = store::now();
    let mut cancel = true;
    if let Some(sc) = b.scan.as_mut().filter(|s| s.id == id) {
        cancel = sc.cancel;
        sc.last_event = store::now();
        sc.added += added;
        if let Some(m) = ev.get("message").and_then(|v| v.as_str()) {
            sc.message = m.to_string();
        }
        for (k, slot) in [("found", &mut sc.found), ("done", &mut sc.done), ("total", &mut sc.total)] {
            if let Some(n) = ev.get(k).and_then(|v| v.as_u64()) {
                *slot = n as u32;
            }
        }
        if !sc.cancel {
            match kind {
                "done" => sc.state = "done".into(),
                "error" => sc.state = "error".into(),
                _ => {}
            }
        }
    }
    drop(b);
    store::s().cv.notify_all();
    store::notify();
    json!({ "cancel": cancel })
}

/* ------------------------------------------------------------------ MCP */

const INSTRUCTIONS: &str = "Shortlist is the user's job-search CRM; jobs come from their LinkedIn via a Chrome extension. \
Workflow: 1) get_profile. If the summary is missing or stale, read the listed documents with your own file tools and call save_profile \
with a dense summary (skills, stack, domains, notable projects, seniority, what they want) plus 3-6 searches. \
2) scan: mode 'recommended' uses the user's own LinkedIn job preferences; mode 'search' runs keywords. If it says still running, call scan_status. \
3) Repeat next_jobs -> save_scores until nothing is left. Score 0-100 for real fit and odds. Strengths are the hooks that raise the user's odds: \
kind 'build' = the job wants a tool/product built (suggest a small demo the user could build first), 'domain' = sector experience wanted \
(e.g. schools, fintech), 'experience' = has done this before, 'skill' = a matching skill to lead with. text = what the job asks (short), \
action = what the user should do or highlight. gaps = short missing requirements. Keep every field short; do not repeat the job text back.";

fn tools() -> Value {
    let s = |d: &str| json!({ "type": "string", "description": d });
    json!([
        { "name": "get_profile", "description": "User profile summary, preferences, document paths, saved searches, counts.",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "save_profile", "description": "Save your condensed profile of the user (<=1500 chars) and optional searches.",
          "inputSchema": { "type": "object", "required": ["summary"], "properties": {
              "summary": s("Dense profile summary"),
              "searches": { "type": "array", "items": { "type": "object", "required": ["keywords"], "properties": {
                  "name": s("Short label"), "keywords": s("LinkedIn keywords"), "location": s("City/country, optional"), "remote": { "type": "boolean" } } } } } } },
        { "name": "scan", "description": "Collect jobs from LinkedIn in the user's Chrome. Waits up to ~45 s.",
          "inputSchema": { "type": "object", "properties": {
              "mode": { "type": "string", "enum": ["recommended", "search"] },
              "keywords": s("For mode=search"), "location": s("For mode=search"),
              "remote": { "type": "boolean" }, "days": { "type": "integer", "description": "Posted within N days" },
              "limit": { "type": "integer", "description": "Max new jobs (default from settings)" } } } },
        { "name": "scan_status", "description": "Progress of the running scan; waits up to 45 s for it to finish.",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "next_jobs", "description": "Unscored jobs, trimmed to what matters for judging fit.",
          "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer", "description": "Default 6, max 15" } } } },
        { "name": "save_scores", "description": "Save fit scores and strong points for jobs.",
          "inputSchema": { "type": "object", "required": ["items"], "properties": { "items": { "type": "array", "items": {
              "type": "object", "required": ["id", "score"], "properties": {
                  "id": s("Job id"), "score": { "type": "integer" }, "fit": s("One line: why it fits or not"),
                  "strengths": { "type": "array", "items": { "type": "object", "properties": {
                      "kind": { "type": "string", "enum": ["build", "domain", "experience", "skill", "other"] },
                      "text": s("What the job asks"), "action": s("What the user should do/highlight") } } },
                  "gaps": { "type": "array", "items": { "type": "string" } } } } } } } },
        { "name": "list_jobs", "description": "One line per job, best first.",
          "inputSchema": { "type": "object", "properties": {
              "status": s("new|saved|applying|applied|interview|offer|rejected|archived|active"),
              "min_score": { "type": "integer" }, "query": s("Text filter"), "limit": { "type": "integer" } } } },
        { "name": "get_job", "description": "Everything about one job, including the full description and notes.",
          "inputSchema": { "type": "object", "required": ["id"], "properties": { "id": s("Job id") } } },
        { "name": "update_job", "description": "Change a job's status, star it, or add a note.",
          "inputSchema": { "type": "object", "required": ["id"], "properties": {
              "id": s("Job id"), "status": s("new|saved|applying|applied|interview|offer|rejected|archived"),
              "starred": { "type": "boolean" }, "note": s("Appended to the job's notes") } } }
    ])
}

fn rpc(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned()?; // notifications get no reply
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "shortlist", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(name, &args) {
                Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            }
        }
        "resources/list" => json!({ "resources": [] }),
        "prompts/list" => json!({ "prompts": [] }),
        _ => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not found" } })),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn arg_str(a: &Value, k: &str) -> String {
    a.get(k).and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_u64().map(|n| n.to_string()))).unwrap_or_default().trim().to_string()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short(s: &str, n: usize) -> String {
    let s = one_line(s);
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

fn unscored_count(d: &store::Data) -> usize {
    d.jobs.iter().filter(|j| j.score.is_none() && j.details && j.status != "archived").count()
}

fn scan_report(sc: &store::Scan) -> String {
    let left = unscored_count(&store::data());
    match sc.state.as_str() {
        "done" => format!("Scan finished ({}): {} new jobs, {} looked at. {left} unscored jobs waiting -> next_jobs.", sc.label, sc.added, sc.found),
        "error" => format!("Scan stopped: {}. {} new jobs were saved before that.", sc.message, sc.added),
        "cancelled" => format!("Scan was cancelled by the user. {} new jobs saved.", sc.added),
        "queued" if !store::ext_connected() => "Chrome isn't connected. Ask the user to open Chrome (with the Shortlist extension) and be signed in to LinkedIn; the scan starts by itself once it connects. Then call scan_status.".into(),
        _ => format!("Still scanning ({}): {} of {} done, {} new so far ({}). Call scan_status.", sc.label, sc.done, sc.total, sc.added, sc.message),
    }
}

fn call(name: &str, a: &Value) -> Result<String, String> {
    match name {
        "get_profile" => {
            let d = store::data();
            let mut out = String::new();
            if d.summary.trim().is_empty() {
                out += "Profile summary: none yet.\n";
            } else {
                out += &format!("Profile summary:\n{}\n", d.summary.trim());
            }
            if store::summary_stale(&d) {
                out += "STALE: documents were added or changed after this summary. Read them and call save_profile.\n";
            }
            out += &format!("\nUser preferences: {}\n", if d.preferences.trim().is_empty() { "none given" } else { d.preferences.trim() });
            let docs = store::docs();
            if docs.is_empty() {
                out += "\nDocuments: none. The user can add CVs and notes in Shortlist > Profile.\n";
            } else {
                out += "\nDocuments:\n";
                for doc in &docs {
                    out += &format!("- {} ({} KB)\n", doc.path.display(), doc.size.div_ceil(1024));
                }
            }
            if !d.searches.is_empty() {
                out += "\nSaved searches:\n";
                for s in &d.searches {
                    out += &format!("- {}{}{}\n", s.keywords, if s.location.is_empty() { String::new() } else { format!(" @ {}", s.location) }, if s.remote { " (remote)" } else { "" });
                }
            }
            let active = d.jobs.iter().filter(|j| j.status != "archived").count();
            out += &format!("\nJobs: {active} active, {} unscored. Chrome extension: {}.", unscored_count(&d), if store::ext_connected() { "connected" } else { "not connected" });
            Ok(out)
        }
        "save_profile" => {
            let summary = arg_str(a, "summary");
            if summary.is_empty() {
                return Err("summary is empty".into());
            }
            let mut d = store::data();
            d.summary = summary.chars().take(4000).collect();
            d.summary_at = store::now();
            let mut added = 0;
            if let Some(list) = a.get("searches").and_then(|v| v.as_array()) {
                for s in list {
                    let kw = arg_str(s, "keywords");
                    if kw.is_empty() || d.searches.iter().any(|x| x.keywords.eq_ignore_ascii_case(&kw)) {
                        continue;
                    }
                    let name = arg_str(s, "name");
                    d.searches.push(store::Search {
                        name: if name.is_empty() { kw.clone() } else { name },
                        keywords: kw,
                        location: arg_str(s, "location"),
                        remote: s.get("remote").and_then(|v| v.as_bool()).unwrap_or(false),
                        days: 14,
                    });
                    added += 1;
                }
            }
            store::save(&d);
            Ok(format!("Profile saved. {added} searches added ({} total).", d.searches.len()))
        }
        "scan" => {
            store::start_scan(a)?;
            let sc = store::wait_scan(45).ok_or("scan vanished")?;
            Ok(scan_report(&sc))
        }
        "scan_status" => match store::wait_scan(45) {
            Some(sc) => Ok(scan_report(&sc)),
            None => Ok("No scan has run since Shortlist started.".into()),
        },
        "next_jobs" => {
            let limit = a.get("limit").and_then(|v| v.as_u64()).unwrap_or(6).clamp(1, 15) as usize;
            let d = store::data();
            let mut list: Vec<&store::Job> = d.jobs.iter().filter(|j| j.score.is_none() && j.details && j.status != "archived").collect();
            list.sort_by(|a, b| b.found_at.cmp(&a.found_at));
            let total = list.len();
            if total == 0 {
                let pending = d.jobs.iter().filter(|j| !j.details).count();
                return Ok(if pending > 0 {
                    format!("No unscored jobs. {pending} jobs are missing their description (LinkedIn didn't return it); a new scan retries them.")
                } else {
                    "No unscored jobs. Run scan to collect more.".into()
                });
            }
            let mut out = String::new();
            for j in list.iter().take(limit) {
                let mut meta = vec![j.location.clone()];
                for x in [&j.workplace, &j.salary, &j.seniority, &j.employment, &j.posted, &j.applicants] {
                    if !x.is_empty() {
                        meta.push(x.clone());
                    }
                }
                out += &format!("[{}] {} — {}\n{}\n{}\n---\n", j.id, j.title, j.company, meta.join(" · "), digest::brief(&j.description, 1400));
            }
            let more = total.saturating_sub(limit);
            out += &format!("{} shown, {more} more unscored after these.", total.min(limit));
            Ok(out)
        }
        "save_scores" => {
            let items = a.get("items").and_then(|v| v.as_array()).ok_or("items must be an array")?;
            let mut d = store::data();
            let mut saved = 0;
            let mut missing = Vec::new();
            for it in items {
                let id = arg_str(it, "id");
                let Some(j) = d.jobs.iter_mut().find(|j| j.id == id) else {
                    missing.push(id);
                    continue;
                };
                j.score = Some(it.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0).clamp(0.0, 100.0).round() as u32);
                j.fit = short(&arg_str(it, "fit"), 300);
                j.strengths = it
                    .get("strengths")
                    .and_then(|v| v.as_array())
                    .map(|l| {
                        l.iter()
                            .map(|s| Strength { kind: arg_str(s, "kind"), text: short(&arg_str(s, "text"), 240), action: short(&arg_str(s, "action"), 300) })
                            .filter(|s| !s.text.is_empty())
                            .take(8)
                            .collect()
                    })
                    .unwrap_or_default();
                j.gaps = it
                    .get("gaps")
                    .and_then(|v| v.as_array())
                    .map(|l| l.iter().filter_map(|g| g.as_str()).map(|g| short(g, 160)).filter(|g| !g.is_empty()).take(6).collect())
                    .unwrap_or_default();
                j.scored_at = store::now();
                saved += 1;
            }
            let left = unscored_count(&d);
            store::save(&d);
            let mut msg = format!("Saved {saved}. {left} unscored left.");
            if !missing.is_empty() {
                msg += &format!(" Unknown ids: {}.", missing.join(", "));
            }
            Ok(msg)
        }
        "list_jobs" => {
            let status = arg_str(a, "status");
            let min = a.get("min_score").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let q = arg_str(a, "query").to_lowercase();
            let limit = a.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).clamp(1, 100) as usize;
            let d = store::data();
            let mut list: Vec<&store::Job> = d
                .jobs
                .iter()
                .filter(|j| match status.as_str() {
                    "" | "active" => !matches!(j.status.as_str(), "archived" | "rejected"),
                    s => j.status == s,
                })
                .filter(|j| j.score.unwrap_or(0) >= min)
                .filter(|j| q.is_empty() || format!("{} {} {} {}", j.title, j.company, j.location, j.notes).to_lowercase().contains(&q))
                .collect();
            list.sort_by(|a, b| b.score.unwrap_or(0).cmp(&a.score.unwrap_or(0)).then(b.found_at.cmp(&a.found_at)));
            if list.is_empty() {
                return Ok("No jobs match.".into());
            }
            let mut out = String::new();
            for j in list.iter().take(limit) {
                let score = j.score.map(|s| s.to_string()).unwrap_or_else(|| "–".into());
                out += &format!("[{}] {score} · {}{} · {} — {}{}\n", j.id, j.status, if j.starred { " ★" } else { "" }, j.title, j.company,
                                if j.salary.is_empty() { String::new() } else { format!(" · {}", j.salary) });
            }
            if list.len() > limit {
                out += &format!("…{} more.", list.len() - limit);
            }
            Ok(out)
        }
        "get_job" => {
            let id = arg_str(a, "id");
            let d = store::data();
            let j = d.jobs.iter().find(|j| j.id == id).ok_or("No job with that id.")?;
            let mut out = format!("[{}] {} — {}\n{} · {} · {}\nStatus: {}{}\nURL: {}\n", j.id, j.title, j.company, j.location, j.workplace, j.salary, j.status,
                                  if j.starred { " (starred)" } else { "" }, j.url);
            if let Some(s) = j.score {
                out += &format!("Score {s}: {}\n", j.fit);
                for st in &j.strengths {
                    out += &format!("+ [{}] {} -> {}\n", st.kind, st.text, st.action);
                }
                for g in &j.gaps {
                    out += &format!("- gap: {g}\n");
                }
            }
            if !j.notes.trim().is_empty() {
                out += &format!("Notes:\n{}\n", j.notes.trim());
            }
            out += &format!("\n{}", j.description);
            Ok(out)
        }
        "update_job" => {
            let id = arg_str(a, "id");
            let mut d = store::data();
            let j = d.jobs.iter_mut().find(|j| j.id == id).ok_or("No job with that id.")?;
            let status = arg_str(a, "status");
            if !status.is_empty() {
                if !STATUSES.contains(&status.as_str()) {
                    return Err(format!("status must be one of {}", STATUSES.join(", ")));
                }
                j.status = status;
            }
            if let Some(b) = a.get("starred").and_then(|v| v.as_bool()) {
                j.starred = b;
            }
            let note = arg_str(a, "note");
            if !note.is_empty() {
                if !j.notes.trim().is_empty() {
                    j.notes.push('\n');
                }
                j.notes.push_str(&note);
            }
            j.updated_at = store::now();
            let msg = format!("Updated: {} is {}.", j.title, j.status);
            store::save(&d);
            Ok(msg)
        }
        _ => Err(format!("Unknown tool {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(method: &str, path: &str, extra: &[&str]) -> Option<&'static str> {
        let mut h = Headers::default();
        h.read("Host: 127.0.0.1:12731");
        for line in extra {
            h.read(line);
        }
        refusal(method, path, &h)
    }

    const JSON: &str = "Content-Type: application/json";
    const EXT: &str = "Origin: chrome-extension://abcdef";

    #[test]
    fn assistants_and_the_extension_get_in() {
        assert_eq!(check("POST", "/mcp", &[JSON]), None);
        // Node's fetch labels its requests "cors" and sends no Origin.
        assert_eq!(check("POST", "/mcp", &["Content-Type: application/json; charset=utf-8", "Sec-Fetch-Mode: cors"]), None);
        assert_eq!(check("GET", "/ext/next", &["Sec-Fetch-Mode: cors"]), None);
        assert_eq!(check("POST", "/ext/event", &[EXT, JSON, "Sec-Fetch-Mode: cors"]), None);
        assert_eq!(check("GET", "/health", &["Sec-Fetch-Mode: navigate"]), None);
    }

    #[test]
    fn web_pages_are_turned_away() {
        let no = Some("403 Forbidden");
        assert_eq!(check("POST", "/mcp", &["Origin: null", JSON]), no);
        assert_eq!(check("POST", "/mcp", &["Origin: https://example.com", JSON]), no);
        assert_eq!(check("POST", "/mcp", &["Origin: http://tauri.localhost.example.com", JSON]), no);
        assert_eq!(check("POST", "/mcp", &["Origin: http://localhost.example.com", JSON]), no);
        assert_eq!(check("POST", "/mcp", &[EXT, JSON]), no);
        assert_eq!(check("GET", "/ext/next", &["Sec-Fetch-Mode: no-cors"]), no);
        assert_eq!(check("GET", "/ext/next", &["Sec-Fetch-Mode: navigate"]), no);
        assert_eq!(check("POST", "/mcp", &["Content-Type: text/plain"]), Some("415 Unsupported Media Type"));
        assert_eq!(check("POST", "/ext/add", &[]), Some("415 Unsupported Media Type"));
        let mut rebound = Headers::default();
        rebound.read("Host: example.com:12731");
        assert_eq!(refusal("GET", "/ext/next", &rebound), no);
    }
}
