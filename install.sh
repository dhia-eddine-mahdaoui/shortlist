#!/bin/bash
# Installs Shortlist on macOS, starts it, and connects Claude Code / Codex.
#
#   bash install.sh                        latest release (builds this checkout if none can be downloaded)
#   bash install.sh --build                build from this checkout
#   bash install.sh Shortlist-macos.dmg    use a disk image you already downloaded
#
# Works with the bash 3.2 that ships with macOS, also when piped: ... | bash
set -euo pipefail

REPO='dhia-eddine-mahdaoui/shortlist'
ASSET='Shortlist-macos.dmg'
BASE='http://127.0.0.1:12731'
DATA="$HOME/Library/Application Support/Shortlist"

SRC=''    # the source checkout this script sits in, if any
APP=''    # the Shortlist.app to install
DEST=''   # where it was installed
TMP=''
MOUNT=''

say() { printf '%s\n' "$*"; }
die() { printf 'Error: %s\n' "$*" >&2; exit 1; }

cleanup() {
    if [ -n "$MOUNT" ]; then hdiutil detach "$MOUNT" -quiet >/dev/null 2>&1 || true; fi
    if [ -n "$TMP" ]; then rm -rf "$TMP"; fi
}

download() {
    say 'Downloading the latest Shortlist...'
    if curl -fsL "https://github.com/$REPO/releases/latest/download/$ASSET" -o "$TMP/$ASSET"; then
        return 0
    fi
    # A private repository needs a signed-in GitHub CLI.
    command -v gh >/dev/null 2>&1 &&
        gh release download -R "$REPO" -p "$ASSET" -D "$TMP" --clobber </dev/null >/dev/null 2>&1
}

from_dmg() {
    MOUNT="$TMP/mount"
    hdiutil attach "$1" -nobrowse -readonly -quiet -mountpoint "$MOUNT" </dev/null
    APP="$MOUNT/Shortlist.app"
    [ -d "$APP" ] || die "Shortlist.app isn't in $1"
}

build() {
    [ -n "$SRC" ] || die 'building needs the source: clone the repository, then run: bash install.sh --build'

    if ! xcode-select -p >/dev/null 2>&1; then
        xcode-select --install >/dev/null 2>&1 || true
        die "Apple's command line tools are needed. Finish the install window that opened, then run this again."
    fi
    if ! command -v node >/dev/null 2>&1; then
        command -v brew >/dev/null 2>&1 || die 'Node.js 20+ is needed. Install it from https://nodejs.org, then run this again.'
        say 'Installing Node.js with Homebrew...'
        brew install node </dev/null
    fi
    if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
    if ! command -v cargo >/dev/null 2>&1; then
        say 'Installing Rust (https://rustup.rs)...'
        curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
        . "$HOME/.cargo/env"
    fi

    (
        cd "$SRC/src-tauri"
        cargo --version </dev/null    # fetches the Rust version pinned in rust-toolchain.toml
        cd "$SRC"
        npm install --no-fund --no-audit </dev/null
        say 'Building Shortlist (the first build takes a few minutes)...'
        npx tauri build --bundles app </dev/null
    )
    APP="$SRC/src-tauri/target/release/bundle/macos/Shortlist.app"
    [ -d "$APP" ] || die "the build finished but $APP is missing"
}

install_app() {
    local dir=/Applications
    if [ ! -w "$dir" ]; then
        dir="$HOME/Applications"
        mkdir -p "$dir"
    fi
    DEST="$dir/Shortlist.app"
    say "Installing to $DEST..."
    if pkill -x shortlist 2>/dev/null; then sleep 1; fi
    rm -rf "$DEST.new"
    ditto "$APP" "$DEST.new"
    rm -rf "$DEST"
    mv "$DEST.new" "$DEST"
    # Shortlist isn't notarized by Apple, so macOS would block a downloaded copy.
    # This removes the "downloaded from the internet" mark from this app only.
    xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true
}

start_app() {
    say 'Starting Shortlist...'
    open "$DEST"
    local i
    for i in $(seq 1 30); do
        if curl -fsS --max-time 2 "$BASE/health" 2>/dev/null | grep -q Shortlist; then return 0; fi
        sleep 1
    done
    die "Shortlist didn't answer on $BASE. Open $DEST and check that nothing else uses port 12731."
}

connect_assistants() {
    local url="$BASE/mcp" found='' claude out c
    claude="$(command -v claude || true)"
    if [ -z "$claude" ]; then
        for c in "$HOME/.local/bin/claude" "$HOME/.claude/local/claude" /opt/homebrew/bin/claude /usr/local/bin/claude; do
            if [ -x "$c" ]; then claude="$c"; break; fi
        done
    fi
    if [ -n "$claude" ]; then
        found=yes
        if out="$("$claude" mcp add --transport http --scope user shortlist "$url" </dev/null 2>&1)"; then
            say 'Connected to Claude Code.'
        else
            case "$out" in
                *'already exists'*) say 'Claude Code already has Shortlist.' ;;
                *) say "Couldn't add Shortlist to Claude Code: $out" ;;
            esac
        fi
    fi

    local codex="$HOME/.codex/config.toml"
    if command -v codex >/dev/null 2>&1 || [ -d "$HOME/.codex" ]; then
        found=yes
        mkdir -p "$HOME/.codex"
        if [ -f "$codex" ] && grep -q '^\[mcp_servers\.shortlist\]' "$codex"; then
            say 'Codex already has Shortlist.'
        else
            printf '\n[mcp_servers.shortlist]\nurl = "%s"\n' "$url" >>"$codex"
            say 'Connected to Codex.'
        fi
    fi

    if [ -z "$found" ]; then
        say 'Claude Code and Codex were not found. After installing one, connect it from Shortlist > Settings.'
    fi
}

chrome_step() {
    local ext="$DATA/chrome-extension"
    printf '%s' "$ext" | pbcopy 2>/dev/null || true
    say ''
    say 'Last step, in Chrome (only once):'
    say '  1. Open chrome://extensions and turn on Developer mode.'
    say '  2. Drag the chrome-extension folder from the Finder window onto that page.'
    say '     Or: Load unpacked, press Cmd+Shift+G, paste the path (already copied), press Return.'
    say "     $ext"
    say '  3. Stay signed in to LinkedIn.'
    if [ ! -d '/Applications/Google Chrome.app' ] && [ ! -d "$HOME/Applications/Google Chrome.app" ]; then
        say 'No Google Chrome in Applications. Get it from https://www.google.com/chrome/ if you need it.'
    fi
    # In the background: "open" can hang while Chrome starts up, and the install is already done.
    open -a 'Google Chrome' 'chrome://extensions/' </dev/null >/dev/null 2>&1 &
    sleep 2    # so the Finder window ends up in front of Chrome
    open -R "$ext" </dev/null >/dev/null 2>&1 &
}

main() {
    [ "$(uname -s)" = Darwin ] || die 'this installer is for macOS. On Windows run install.ps1.'

    local script="${BASH_SOURCE[0]:-}" dir
    if [ -n "$script" ] && [ -f "$script" ]; then
        dir="$(cd "$(dirname "$script")" && pwd)"
        if [ -f "$dir/src-tauri/tauri.conf.json" ]; then SRC="$dir"; fi
    fi

    TMP="$(mktemp -d)"
    trap cleanup EXIT

    case "${1:-}" in
        '')
            if download; then
                from_dmg "$TMP/$ASSET"
            elif [ -n "$SRC" ]; then
                say 'No release could be downloaded, so building from source.'
                build
            else
                die "couldn't download $ASSET. If the repository is private, run 'gh auth login' and try again, or download it from https://github.com/$REPO/releases and run: bash install.sh path/to/$ASSET"
            fi
            ;;
        --build) build ;;
        *.dmg)
            [ -f "$1" ] || die "no such file: $1"
            from_dmg "$1"
            ;;
        *) die "unknown option: $1 (use --build or a path to $ASSET)" ;;
    esac

    install_app
    start_app
    connect_assistants
    chrome_step
}

main "$@"
