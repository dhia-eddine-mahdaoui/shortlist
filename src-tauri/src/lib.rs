mod digest;
mod server;
mod store;
#[cfg_attr(windows, allow(dead_code))]
mod tools;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use tauri::Manager;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

/// The Chrome extension is written next to the data so its folder never moves between updates.
const EXTENSION: &[(&str, &[u8])] = &[
    ("manifest.json", include_bytes!("../../extension/manifest.json")),
    ("background.js", include_bytes!("../../extension/background.js")),
    ("linkedin.js", include_bytes!("../../extension/linkedin.js")),
    ("popup.html", include_bytes!("../../extension/popup.html")),
    ("popup.js", include_bytes!("../../extension/popup.js")),
    ("icon48.png", include_bytes!("../../extension/icon48.png")),
    ("icon128.png", include_bytes!("../../extension/icon128.png")),
];

fn ext_dir() -> PathBuf {
    store::s().dir.join("chrome-extension")
}

fn mcp_url() -> String {
    format!("http://127.0.0.1:{}/mcp", server::port())
}

#[tauri::command]
fn get_state() -> Value {
    let d = store::data();
    let docs: Vec<Value> = store::docs().iter().map(|x| json!({ "name": x.name, "size": x.size, "modified": x.modified })).collect();
    json!({
        "jobs": d.jobs,
        "summary": d.summary,
        "summaryAt": d.summary_at,
        "stale": store::summary_stale(&d),
        "preferences": d.preferences,
        "searches": d.searches,
        "scanLimit": d.scan_limit,
        "docs": docs,
        "chrome": store::ext_connected(),
        "scan": store::scan_json(),
        "mcpUrl": mcp_url(),
        "dataDir": store::s().dir.display().to_string(),
        "extDir": ext_dir().display().to_string(),
        "os": std::env::consts::OS,
    })
}

#[tauri::command]
fn get_status() -> Value {
    json!({ "chrome": store::ext_connected(), "scan": store::scan_json() })
}

#[tauri::command]
fn update_job(id: String, patch: Value) -> Result<(), String> {
    let mut d = store::data();
    let j = d.jobs.iter_mut().find(|j| j.id == id).ok_or("job not found")?;
    if let Some(s) = patch.get("status").and_then(|v| v.as_str()) {
        if store::STATUSES.contains(&s) {
            j.status = s.to_string();
        }
    }
    if let Some(b) = patch.get("starred").and_then(|v| v.as_bool()) {
        j.starred = b;
    }
    if let Some(n) = patch.get("notes").and_then(|v| v.as_str()) {
        j.notes = n.to_string();
    }
    if patch.get("clearScore").and_then(|v| v.as_bool()) == Some(true) {
        j.score = None;
        j.fit.clear();
        j.strengths.clear();
        j.gaps.clear();
    }
    j.updated_at = store::now();
    store::save(&d);
    Ok(())
}

#[tauri::command]
fn delete_job(id: String) {
    let mut d = store::data();
    d.jobs.retain(|j| j.id != id);
    store::save(&d);
}

#[tauri::command]
fn set_preferences(text: String) {
    let mut d = store::data();
    d.preferences = text;
    store::save(&d);
}

#[tauri::command]
fn set_summary(text: String) {
    let mut d = store::data();
    d.summary = text;
    d.summary_at = store::now();
    store::save(&d);
}

#[tauri::command]
fn set_searches(list: Vec<store::Search>) {
    let mut d = store::data();
    d.searches = list.into_iter().filter(|s| !s.keywords.trim().is_empty()).collect();
    store::save(&d);
}

#[tauri::command]
fn set_scan_limit(limit: u32) {
    let mut d = store::data();
    d.scan_limit = limit.clamp(1, 100);
    store::save(&d);
}

#[tauri::command]
async fn add_docs() -> Result<u32, String> {
    let files = rfd::FileDialog::new()
        .set_title("Add documents about you")
        .add_filter("Documents", &["md", "txt", "pdf", "docx", "json", "html", "rtf"])
        .add_filter("All files", &["*"])
        .pick_files()
        .unwrap_or_default();
    let dir = store::docs_dir();
    let mut n = 0;
    for f in files {
        if let Some(name) = f.file_name() {
            if std::fs::copy(&f, dir.join(name)).is_ok() {
                n += 1;
            }
        }
    }
    store::notify();
    Ok(n)
}

#[tauri::command]
fn remove_doc(name: String) -> Result<(), String> {
    if name.contains(['/', '\\']) || name.contains("..") {
        return Err("bad name".into());
    }
    std::fs::remove_file(store::docs_dir().join(name)).map_err(|e| e.to_string())?;
    store::notify();
    Ok(())
}

fn shell_open(target: &str) {
    #[cfg(windows)]
    {
        let _ = Command::new("rundll32").args(["url.dll,FileProtocolHandler", target]).creation_flags(NO_WINDOW).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(target).spawn();
    }
}

/// Shows the extension folder. On macOS it is selected in Finder, ready to drag into Chrome
/// (it lives in ~/Library, which Chrome's folder picker hides).
fn reveal_extension() {
    let dir = ext_dir().display().to_string();
    if cfg!(target_os = "macos") {
        let _ = Command::new("open").args(["-R", &dir]).spawn();
    } else {
        shell_open(&dir);
    }
}

fn open_chrome_extensions() -> Result<(), String> {
    #[cfg(windows)]
    {
        Command::new("cmd").args(["/C", "start", "", "chrome", "chrome://extensions/"]).creation_flags(NO_WINDOW).spawn().map_err(|e| e.to_string())?;
    }
    // `open` fails when Chrome isn't installed.
    if cfg!(target_os = "macos") {
        let status = Command::new("open").args(["-a", "Google Chrome", "chrome://extensions/"]).status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("Google Chrome wasn't found".into());
        }
    }
    Ok(())
}

// Off the main thread: macOS "open" can take a long time to return while Chrome starts up.
#[tauri::command(async)]
fn open(target: String) -> Result<(), String> {
    match target.as_str() {
        "docs" => shell_open(&store::docs_dir().display().to_string()),
        "data" => shell_open(&store::s().dir.display().to_string()),
        "extension" => reveal_extension(),
        "chrome-extensions" => open_chrome_extensions()?,
        t if t.starts_with("https://") => shell_open(t),
        _ => return Err("not allowed".into()),
    }
    Ok(())
}

#[tauri::command]
fn start_scan(params: Value) -> Result<Value, String> {
    store::start_scan(&params)
}

#[tauri::command]
fn cancel_scan() {
    store::cancel_scan();
}

const NO_CLAUDE: &str = "The claude command wasn't found. Install Claude Code or run the command shown below yourself.";

#[cfg(windows)]
fn claude_command() -> Option<Command> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "claude"]).creation_flags(NO_WINDOW);
    Some(cmd)
}

#[cfg(not(windows))]
fn claude_command() -> Option<Command> {
    tools::command("claude")
}

/// Registers the MCP server with Claude Code or Codex for this user.
#[tauri::command]
async fn connect_ai(kind: String) -> Result<String, String> {
    let url = mcp_url();
    match kind.as_str() {
        "claude" => {
            let mut cmd = claude_command().ok_or(NO_CLAUDE)?;
            cmd.args(["mcp", "add", "--transport", "http", "--scope", "user", "shortlist", &url]);
            let out = cmd.output().map_err(|e| e.to_string())?;
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            if out.status.success() {
                Ok("Added to Claude Code. Restart Claude Code sessions to pick it up.".into())
            } else if text.contains("already exists") {
                Ok("Claude Code already has Shortlist.".into())
            } else if text.contains("not recognized") {
                Err(NO_CLAUDE.into())
            } else {
                Err(text.trim().to_string())
            }
        }
        "codex" => {
            let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).map_err(|e| e.to_string())?;
            let dir = PathBuf::from(home).join(".codex");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join("config.toml");
            let current = std::fs::read_to_string(&path).unwrap_or_default();
            if current.contains("[mcp_servers.shortlist]") {
                return Ok("Codex already has Shortlist.".into());
            }
            let block = format!("{}[mcp_servers.shortlist]\nurl = \"{url}\"\n", if current.is_empty() || current.ends_with("\n\n") { "" } else if current.ends_with('\n') { "\n" } else { "\n\n" });
            std::fs::write(&path, current + &block).map_err(|e| e.to_string())?;
            Ok(format!("Added to {}. Restart Codex to pick it up.", path.display()))
        }
        _ => Err("unknown assistant".into()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_state, get_status, update_job, delete_job, set_preferences, set_summary, set_searches, set_scan_limit,
            add_docs, remove_doc, open, start_scan, cancel_scan, connect_ai
        ])
        .setup(|app| {
            let dir = match std::env::var_os("SHORTLIST_DATA_DIR") {
                Some(d) => PathBuf::from(d),
                None => app.path().data_dir().unwrap_or_else(|_| std::env::temp_dir()).join("Shortlist"),
            };
            store::init(app.handle().clone(), dir);
            let ext = ext_dir();
            let _ = std::fs::create_dir_all(&ext);
            for (name, bytes) in EXTENSION {
                let _ = std::fs::write(ext.join(name), bytes);
            }
            server::start();
            Ok(())
        })
        .on_window_event(|_window, _event| {
            // macOS: closing the window keeps Shortlist (and its server) running in the Dock.
            // Quit with ⌘Q.
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::CloseRequested { api, .. } = _event {
                api.prevent_close();
                // Hiding a full-screen window leaves a black screen, so leave full screen first.
                if _window.is_fullscreen().unwrap_or(false) {
                    let _ = _window.set_fullscreen(false);
                } else {
                    let _ = _window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running Shortlist")
        .run(|_app, _event| {
            // Clicking the Dock icon brings the window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                if let Some(w) = _app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        });
}
