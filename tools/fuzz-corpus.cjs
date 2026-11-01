#!/usr/bin/env node
// Fuzz corpus maintenance for the targets in fuzz/ (docs/TEST-SUITE.md section 12, item 7).
//
//   node tools/fuzz-corpus.cjs run [seconds] [target...]   run each target for a bounded time
//   node tools/fuzz-corpus.cjs minimise [target...]         shrink corpus/fuzz/<target> with cmin
//   node tools/fuzz-corpus.cjs replay [target...]           run every saved crash in fuzz/artifacts
//
// Targets default to all of them. On Windows the AddressSanitizer runtime DLL that ships with
// Visual Studio is put on PATH, because the fuzz binaries load it at start.

'use strict';
const { spawnSync, spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const FUZZ_DIR = path.join(ROOT, 'fuzz');
const CORPUS = path.join(ROOT, 'corpus', 'fuzz');
const TARGETS = ['f01_parser', 'f02_terminal', 'f03_osc', 'f04_dcs_apc', 'f05_kitty_graphics', 'f06_sixel', 'f07_png', 'f08_key_encoder', 'f09_mouse_encoder', 'f10_resize', 'f11_page'];

/** Finds the directory of the Visual Studio AddressSanitizer runtime, or null. */
function asanDir() {
  if (process.platform !== 'win32') return null;
  const base = 'C:/Program Files/Microsoft Visual Studio';
  const found = [];
  const walk = (dir, depth) => {
    if (depth > 9 || found.length) return;
    let entries = [];
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      const p = path.join(dir, e.name);
      if (e.isFile() && e.name === 'clang_rt.asan_dynamic-x86_64.dll' && p.includes('Hostx64')) found.push(dir);
      else if (e.isDirectory()) walk(p, depth + 1);
    }
  };
  walk(base, 0);
  return found[0] || null;
}

/** The environment for fuzz processes, with the ASan runtime on PATH on Windows. */
function fuzzEnv() {
  const env = { ...process.env };
  const dir = asanDir();
  if (dir) env.PATH = `${dir}${path.delimiter}${env.PATH}`;
  return env;
}

/** Runs targets in parallel for `seconds` each; resolves with a failure count. */
async function run(seconds, targets) {
  const env = fuzzEnv();
  const results = await Promise.all(targets.map((t) => new Promise((resolve) => {
    fs.mkdirSync(path.join(CORPUS, t), { recursive: true });
    const args = ['+nightly', 'fuzz', 'run', '--fuzz-dir', FUZZ_DIR, t, path.join(CORPUS, t), '--', `-max_total_time=${seconds}`, '-max_len=4096', '-rss_limit_mb=4096'];
    let log = '';
    const child = spawn('cargo', args, { env });
    child.stdout.on('data', (d) => { log += d; });
    child.stderr.on('data', (d) => { log += d; });
    child.on('close', (code) => {
      const done = (log.match(/Done \d+ runs in \d+ second\(s\)/) || ['no summary'])[0];
      console.log(`${t}: ${code === 0 ? 'ok' : 'FAILED'} (${done})`);
      if (code !== 0) console.log(log.split('\n').slice(-30).join('\n'));
      resolve(code === 0 ? 0 : 1);
    });
  })));
  return results.reduce((a, b) => a + b, 0);
}

/** Shrinks each target's corpus to the inputs that add coverage. */
function minimise(targets) {
  const env = fuzzEnv();
  for (const t of targets) {
    const r = spawnSync('cargo', ['+nightly', 'fuzz', 'cmin', '--fuzz-dir', FUZZ_DIR, t, path.join(CORPUS, t)], { env, stdio: 'inherit' });
    if (r.status !== 0) return 1;
  }
  return 0;
}

/** Runs each saved crash input once so a fix can be confirmed. */
function replay(targets) {
  const env = fuzzEnv();
  let failures = 0;
  for (const t of targets) {
    const dir = path.join(FUZZ_DIR, 'artifacts', t);
    if (!fs.existsSync(dir)) continue;
    for (const f of fs.readdirSync(dir)) {
      const r = spawnSync('cargo', ['+nightly', 'fuzz', 'run', '--fuzz-dir', FUZZ_DIR, t, path.join(dir, f)], { env, encoding: 'utf8' });
      console.log(`${t}/${f}: ${r.status === 0 ? 'passes now' : 'still fails'}`);
      if (r.status !== 0) failures++;
    }
  }
  return failures;
}

/** Entry point. */
async function main() {
  const [cmd, ...rest] = process.argv.slice(2);
  const seconds = cmd === 'run' && /^\d+$/.test(rest[0] || '') ? Number(rest.shift()) : 60;
  const targets = rest.length ? rest : TARGETS;
  let failures = 0;
  if (cmd === 'run') failures = await run(seconds, targets);
  else if (cmd === 'minimise') failures = minimise(targets);
  else if (cmd === 'replay') failures = replay(targets);
  else {
    console.log('usage: node tools/fuzz-corpus.cjs run [seconds] [target...] | minimise [target...] | replay [target...]');
    process.exit(2);
  }
  process.exit(failures ? 1 : 0);
}

main();
