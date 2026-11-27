// Release build. Rust writes source file paths into the app, so they are rewritten first:
// otherwise a published build shows the user name and folders of whoever built it.
// Usage: npm run desktop [-- <tauri build options>]
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';

const project = fileURLToPath(new URL('..', import.meta.url)).replace(/[\\/]$/, '');

// When two prefixes match, rustc uses the later one, so the more specific folders come last.
const remap = [[homedir(), '~'], [process.env.CARGO_HOME, 'cargo'], [project, 'shortlist']]
  .filter(([from]) => from)
  .map(([from, to]) => `--remap-path-prefix=${from}=${to}`);

// The encoded form keeps paths with spaces whole. It replaces RUSTFLAGS, so carry those over.
const { CARGO_ENCODED_RUSTFLAGS: encoded, RUSTFLAGS: plain = '' } = process.env;
const own = encoded ? encoded.split('\x1f') : plain.split(/\s+/).filter(Boolean);

const tauri = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
const build = spawnSync(process.execPath, [tauri, 'build', ...process.argv.slice(2)], {
  cwd: project,
  stdio: 'inherit',
  env: { ...process.env, CARGO_ENCODED_RUSTFLAGS: [...own, ...remap].join('\x1f') },
});
process.exit(build.status ?? 1);
