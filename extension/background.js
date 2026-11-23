// Connects Chrome to the Shortlist desktop app (127.0.0.1:12731).
// The app hands out scan tasks; this opens LinkedIn in a background tab, reads the
// job list and each job's public page, and sends the results back. Nothing else.
importScripts('linkedin.js');

const APP = 'http://127.0.0.1:12731';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let polling = false;
let connected = null;

async function post(path, body) {
  const r = await fetch(APP + path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  return r.json().catch(() => ({}));
}

function setConnected(on) {
  // Calling an extension API here also keeps the service worker awake between polls.
  chrome.storage.session.set({ connected: on, at: Date.now() });
  if (on === connected) return;
  connected = on;
  chrome.action.setBadgeBackgroundColor({ color: '#56687a' });
  chrome.action.setBadgeText({ text: on ? '' : 'off' });
}

async function poll() {
  if (polling) return;
  polling = true;
  try {
    for (;;) {
      let task;
      try {
        const r = await fetch(APP + '/ext/next', { cache: 'no-store' });
        task = await r.json();
        setConnected(true);
      } catch {
        setConnected(false);
        await sleep(6000);
        continue;
      }
      if (task && task.kind === 'scan') await runScan(task);
    }
  } finally {
    polling = false;
  }
}

async function inject(tabId, func, args = []) {
  const [res] = await chrome.scripting.executeScript({ target: { tabId }, func, args });
  return res && res.result;
}

async function waitLoad(tabId, timeout = 30000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const t = await chrome.tabs.get(tabId);
    if (t.status === 'complete') return t;
    await sleep(300);
  }
  return chrome.tabs.get(tabId);
}

const signedOut = (url) => /linkedin\.com\/(login|authwall|checkpoint|uas\/login|signup)/.test(url || '');

function listUrl(p, start) {
  if (p.mode === 'search') {
    const q = new URLSearchParams({ keywords: p.keywords });
    if (p.location) q.set('location', p.location);
    if (p.days) q.set('f_TPR', 'r' + p.days * 86400);
    if (p.remote) q.set('f_WT', '2');
    if (start) q.set('start', String(start));
    return 'https://www.linkedin.com/jobs/search/?' + q;
  }
  return 'https://www.linkedin.com/jobs/collections/recommended/' + (start ? '?start=' + start : '');
}

function guestUrl(p, start) {
  const q = new URLSearchParams({ keywords: p.keywords || '' });
  if (p.location) q.set('location', p.location);
  if (p.days) q.set('f_TPR', 'r' + p.days * 86400);
  if (p.remote) q.set('f_WT', '2');
  q.set('start', String(start));
  return 'https://www.linkedin.com/jobs-guest/jobs/api/seeMoreJobPostings/search?' + q;
}

// Card info first, then the detail page wins wherever it has a value.
function combine(card, detail) {
  const out = { ...(card || {}) };
  for (const [k, v] of Object.entries(detail || {})) if (v !== '' && v != null && k !== 'rate') out[k] = v;
  return out;
}

async function runScan(task) {
  const p = task.params || {};
  const limit = p.limit || 30;
  const skip = new Set(task.skip || []);
  const ev = (type, extra = {}) => post('/ext/event', { scan: task.id, type, ...extra }).catch(() => ({}));
  let tab = null;
  try {
    tab = await chrome.tabs.create({ url: listUrl(p, 0), active: false });
    const cards = new Map();
    const ids = [];
    let guestStart = 0;
    for (let page = 0; page < 8 && ids.length < limit; page++) {
      if (page > 0) await chrome.tabs.update(tab.id, { url: listUrl(p, page * 25) });
      const t = await waitLoad(tab.id);
      if (signedOut(t.url)) throw new Error('Not signed in to LinkedIn in Chrome. Sign in there, then scan again');
      await sleep(2500);
      let res = await inject(tab.id, slHarvestDom);
      if (res && res.signedOut) throw new Error('Not signed in to LinkedIn in Chrome. Sign in there, then scan again');
      let got = (res && res.cards) || [];
      if (!got.length && p.mode === 'search') {
        // Fall back to the public search when the signed-in list can't be read.
        res = await inject(tab.id, slHarvestGuest, [guestUrl(p, guestStart)]);
        got = (res && res.cards) || [];
        guestStart += got.length || 10;
      }
      let fresh = 0;
      for (const c of got) {
        if (cards.has(c.id)) continue;
        cards.set(c.id, c);
        fresh++;
        if (!skip.has(c.id) && ids.length < limit) ids.push(c.id);
      }
      const r = await ev('progress', { message: `Reading the job list, page ${page + 1}`, found: cards.size, total: ids.length });
      if (r.cancel) return;
      if (!got.length || !fresh) break;
    }
    if (!cards.size) throw new Error('No jobs were found on that LinkedIn page');

    let batch = [];
    let done = 0;
    let limited = false;
    for (const id of ids) {
      let d = limited ? null : await inject(tab.id, slDetail, [id]);
      if (d && d.rate) {
        await ev('progress', { message: 'LinkedIn asked to slow down, waiting 20 s' });
        await sleep(20000);
        d = await inject(tab.id, slDetail, [id]);
        if (d && d.rate) { limited = true; d = null; }
      }
      batch.push(combine(cards.get(id), d));
      done++;
      if (batch.length >= 4 || done === ids.length) {
        const r = await ev('jobs', { jobs: batch, done, total: ids.length, found: cards.size, message: `Reading job ${done} of ${ids.length}` });
        batch = [];
        if (r.cancel) return;
      }
      if (!limited) await sleep(900 + Math.random() * 800);
    }
    const msg = limited ? 'LinkedIn rate limit: some jobs were saved without a description' : ids.length ? 'Done' : 'Nothing new since the last scan';
    await ev('done', { message: msg, done, total: ids.length, found: cards.size });
  } catch (e) {
    await ev('error', { message: String((e && e.message) || e) });
  } finally {
    if (tab) chrome.tabs.remove(tab.id).catch(() => {});
  }
}

async function addCurrentJob() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab || !/^https:\/\/www\.linkedin\.com\//.test(tab.url || '')) return { error: 'Open a LinkedIn job first.' };
  const id = await inject(tab.id, slCurrentJobId);
  if (!id) return { error: 'Open a LinkedIn job first.' };
  const d = await inject(tab.id, slDetail, [id]);
  if (d && d.rate) return { error: 'LinkedIn asked to slow down. Try again in a minute.' };
  try {
    const r = await post('/ext/add', { jobs: [d] });
    return { ok: true, added: r.added, title: d.title };
  } catch {
    return { error: "Shortlist isn't running." };
  }
}

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg && msg.type === 'add') {
    addCurrentJob().then(reply);
    return true;
  }
  if (msg && msg.type === 'status') {
    fetch(APP + '/ext/status', { cache: 'no-store' }).then((r) => r.json()).then(reply).catch(() => reply(null));
    return true;
  }
});

chrome.runtime.onStartup.addListener(poll);
chrome.runtime.onInstalled.addListener(() => {
  chrome.alarms.create('wake', { periodInMinutes: 1 });
  poll();
});
chrome.alarms.onAlarm.addListener(poll);
poll();
