# Shortlist

A small macOS and Windows app that keeps your job search in one place. A Chrome extension reads LinkedIn in your own signed-in Chrome; your AI assistant (Claude Code, Codex, anything that speaks MCP) scores each job against your CV and notes, and points out what will make you stand out: a tool the job wants built, a domain they care about, experience to lead with.

No API keys, no LinkedIn login inside the app, no AI backend. The assistant you already use does the thinking through MCP.

## Install on macOS

One command in Terminal (needs the [GitHub CLI](https://cli.github.com) signed in with `gh auth login`, because this repository is private):

```bash
gh api repos/dhia-eddine-mahdaoui/shortlist/contents/install.sh -H "Accept: application/vnd.github.raw" | bash
```

It downloads the app (Apple silicon and Intel, macOS 11 or later), puts it in Applications, starts it, connects Claude Code and Codex if they're installed, and opens Chrome and Finder for the last step:

1. In Chrome, open `chrome://extensions` and turn on **Developer mode**.
2. Drag the `chrome-extension` folder from the Finder window onto that page.
3. Stay signed in to LinkedIn.

**Without Terminal:** download `Shortlist-macos.dmg` from **Releases** and drag Shortlist to Applications. The app isn't notarized by Apple, so the first launch is blocked: open **System Settings → Privacy & Security** and click **Open Anyway** (on macOS 14 or earlier, right-click the app → **Open**). Then follow the app's **Settings** page.

**From source:** in a clone of this repository, `bash install.sh --build`. It installs Rust if needed (and Node.js through Homebrew), asks for Apple's command line tools if they're missing, then builds and installs the same way.

Closing the window leaves Shortlist running in the Dock, so your assistant can still reach it. Quit with ⌘Q.

## Install on Windows

Download `Shortlist_x.y.z_x64-setup.exe` from **Releases** and run it (it isn't code-signed yet: on the SmartScreen warning click **More info** → **Run anyway**). Then connect your assistant from the app's **Settings** page.

Or build from source:

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

This builds the app, installs it for your user, starts it, and registers the MCP server with Claude Code and Codex if they're installed. Then, in Chrome:

1. Open `chrome://extensions`, turn on **Developer mode**.
2. **Load unpacked** → `%APPDATA%\Shortlist\chrome-extension` (the app writes it there on start).
3. Stay signed in to LinkedIn.

The **Settings** page in the app has the same steps, plus buttons that register the MCP server for you.

## Use it

1. **Profile** → *Add files*: CVs, Markdown notes about you, a portfolio, anything. Write what you want in *What you're looking for*.
2. Ask your assistant: *“Use Shortlist: refresh my profile if needed, scan LinkedIn, and score the new jobs.”*
3. Work the list in **Jobs**: filter by match, salary, workplace, status, source. Move jobs along in **Pipeline**. **Ideas** collects the things to build and domains employers keep asking for.

You can also scan from the app (*Scan LinkedIn* → your LinkedIn preferences, a saved search, or a new one) and ask the assistant to score later.

## How it stays light on tokens

- The assistant reads your documents once and saves a short summary (`save_profile`); later sessions use the summary. The app flags it as stale when documents change.
- `next_jobs` sends a trimmed brief of each job: requirements and responsibilities kept, benefits, company blurb and legal text dropped, capped at ~1,400 characters.
- Tool replies are plain text, not JSON.

## MCP tools

Server: `http://127.0.0.1:12731/mcp` (runs while the app is open; `curl http://127.0.0.1:12731/health`).

| Tool | What it does |
| --- | --- |
| `get_profile` | Summary, preferences, document paths, saved searches, counts |
| `save_profile` | Store the condensed profile and suggested searches |
| `scan` / `scan_status` | Collect jobs: `recommended` (your LinkedIn job preferences) or `search` (keywords, location, remote) |
| `next_jobs` | Unscored jobs, trimmed |
| `save_scores` | Score 0–100, one-line fit, strong points (`build` / `domain` / `experience` / `skill`) with an action each, gaps |
| `list_jobs` / `get_job` | Browse what's saved |
| `update_job` | Status, star, notes |

## How scanning works

The app queues a scan; the extension picks it up, opens LinkedIn in a background tab, collects job ids from the list (your recommended jobs, or a search), then reads each job's public page for the full description. About one job per second, at most 100 per scan, and it stops if LinkedIn asks it to slow down. It never clicks Apply, Connect, or anything else. Jobs already saved in full are skipped.

Data lives in `%APPDATA%\Shortlist\data.json` on Windows and `~/Library/Application Support/Shortlist/data.json` on macOS.

## Develop

```powershell
npm install
npx tauri dev
```

The UI is plain HTML/CSS/JS in `ui/` (no build step); opening it without Tauri uses sample data. The Rust side is in `src-tauri/src/`: `server.rs` (MCP + extension bridge), `store.rs` (data), `digest.rs` (salary parsing, job briefs), `tools.rs` (finding `claude` on macOS).

**Release the Mac app:** push a `v*` tag. The *macOS* workflow builds `Shortlist-macos.dmg`, installs it on a Mac runner to check it starts, and attaches it to that release. It also runs on pull requests and can be started by hand from the Actions tab.
