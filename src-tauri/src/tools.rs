//! Finding command-line tools on macOS and Linux. An app opened from Finder or the Dock gets
//! a bare PATH (/usr/bin:/bin:…), so `claude` and the `node` it may need are looked up the way
//! a terminal would: the login shell's PATH, then the usual install folders.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Install folders under the home directory.
const HOME_DIRS: &[&str] = &[".local/bin", ".claude/local", ".npm-global/bin", ".volta/bin", ".bun/bin"];
/// Homebrew (Apple silicon, then Intel) and the system.
const SYSTEM_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

/// The PATH line from the output of `env`. Anything the shell's startup files print is ignored.
fn path_from_env(output: &str) -> Option<&str> {
    output.lines().rev().find_map(|l| l.strip_prefix("PATH="))
}

/// Asks the user's login shell for its environment. Gives up after a few seconds.
fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args(["-ilc", "/usr/bin/env"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let bytes = rx.recv_timeout(Duration::from_secs(8));
    if bytes.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    path_from_env(&String::from_utf8_lossy(&bytes.ok()?)).map(str::to_string)
}

/// Folders to search, in order, without repeats.
fn search_dirs(shell_path: Option<&str>, home: &Path) -> Vec<PathBuf> {
    let from_shell = shell_path.map(|p| std::env::split_paths(p).collect::<Vec<_>>()).unwrap_or_default();
    let usual = HOME_DIRS.iter().map(|d| home.join(d)).chain(SYSTEM_DIRS.iter().map(PathBuf::from));
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in from_shell.into_iter().chain(usual) {
        if !dir.as_os_str().is_empty() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

fn find_in(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// A command ready to run `name` with the user's PATH, or None when it isn't installed.
pub fn command(name: &str) -> Option<Command> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let dirs = search_dirs(login_shell_path().as_deref(), &home);
    let mut cmd = Command::new(find_in(name, &dirs)?);
    let path: OsString = std::env::join_paths(&dirs).ok()?;
    cmd.env("PATH", path);
    Some(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_path_from_noisy_shell_output() {
        let out = "Welcome back!\nHOME=/Users/a\nPATH=/Users/a/.nvm/bin:/usr/bin\nSHELL=/bin/zsh\n";
        assert_eq!(path_from_env(out), Some("/Users/a/.nvm/bin:/usr/bin"));
        assert_eq!(path_from_env("nothing here"), None);
    }

    #[test]
    fn usual_folders_are_searched_without_a_shell() {
        let home = Path::new("home");
        let dirs = search_dirs(None, home);
        assert_eq!(dirs[0], home.join(".local/bin"));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn finds_a_tool_in_the_first_folder_that_has_it() {
        let root = std::env::temp_dir().join(format!("shortlist-tools-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("claude"), "").unwrap();
        assert_eq!(find_in("claude", &[a.clone(), b.clone()]), Some(b.join("claude")));
        assert_eq!(find_in("missing", &[a, b]), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
