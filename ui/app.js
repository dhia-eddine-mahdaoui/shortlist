'use strict';

/* ================================================================ backend */

const T = window.__TAURI__;
const api = (cmd, args = {}) => (T ? T.core.invoke(cmd, args) : mock(cmd, args));

const STATUS = [
  ['new', 'New'], ['saved', 'Saved'], ['applying', 'Applying'], ['applied', 'Applied'],
  ['interview', 'Interview'], ['offer', 'Offer'], ['rejected', 'Rejected'], ['archived', 'Archived'],
];
const STATUS_NAME = Object.fromEntries(STATUS);
const BOARD = ['saved', 'applying', 'applied', 'interview', 'offer', 'rejected'];
const KIND = { build: 'Build this', domain: 'Domain', experience: 'Experience', skill: 'Skill', other: 'Note' };
const ASK_SCORE = 'Use Shortlist to score my new jobs.';
const ASK_FULL = 'Use Shortlist: refresh my profile if needed, scan LinkedIn, and score the new jobs.';

const S = {
  data: { jobs: [], searches: [], docs: [] },
  view: 'jobs',
  sel: null,
  f: { q: '', score: '', salary: '', workplace: '', status: 'active', source: '', starred: false, sort: 'match' },
  ideasMin: 60,
};

const $ = (s, el = document) => el.querySelector(s);
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const jobById = (id) => S.data.jobs.find((j) => j.id === id);

function toast(msg, ms = 3200) {
  const t = $('#toast');
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.hidden = true), ms);
}

async function copy(text) {
  try { await navigator.clipboard.writeText(text); toast('Copied'); } catch { toast('Could not copy'); }
}

function ago(secs) {
  if (!secs) return '';
  const d = Date.now() / 1000 - secs;
  if (d < 3600) return `${Math.max(1, Math.round(d / 60))}m`;
  if (d < 86400) return `${Math.round(d / 3600)}h`;
  if (d < 86400 * 30) return `${Math.round(d / 86400)}d`;
  return new Date(secs * 1000).toLocaleDateString();
}

function money(n, cur) {
  if (n == null) return '';
  const sym = { GBP: '£', USD: '$', EUR: '€', TRY: '₺' }[cur] || '';
  const v = n >= 1000 ? `${Math.round(n / 1000)}K` : String(Math.round(n));
  return sym ? sym + v : `${v}${cur ? ' ' + cur : ''}`;
}

/* ================================================================ data */

let loading = null;
async function load() {
  if (loading) return loading;
  loading = (async () => {
    try {
      S.data = await api('get_state');
      render();
    } finally {
      loading = null;
    }
  })();
  return loading;
}

let reloadTimer = null;
function reloadSoon() {
  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(load, 150);
}

async function patch(id, p) {
  const j = jobById(id);
  if (j) Object.assign(j, p.notes !== undefined ? { notes: p.notes } : {}, p.status ? { status: p.status } : {}, p.starred !== undefined ? { starred: p.starred } : {});
  await api('update_job', { id, patch: p });
}

/* ================================================================ filters */

const SALARY_STEPS = [30000, 50000, 70000, 100000, 150000];

function chipDefs() {
  const sources = [...new Set(S.data.jobs.map((j) => j.source).filter(Boolean))].sort();
  return [
    { key: 'score', label: 'Match', options: [['', 'Any match'], [50, '50+'], [65, '65+'], [80, '80+'], ['unscored', 'Not scored yet']], show: (v) => (v === 'unscored' ? 'Not scored' : `Match ${v}+`) },
    { key: 'salary', label: 'Salary', options: [['', 'Any salary'], ['has', 'Salary listed'], ...SALARY_STEPS.map((n) => [n, `${n / 1000}K+ a year`])], show: (v) => (v === 'has' ? 'Salary listed' : `${v / 1000}K+`) },
    { key: 'workplace', label: 'Workplace', options: [['', 'Any'], ['Remote', 'Remote'], ['Hybrid', 'Hybrid'], ['On-site', 'On-site']], show: (v) => v },
    { key: 'status', label: 'Status', options: [['active', 'Active'], ['all', 'All'], ...STATUS], show: (v) => (v === 'all' ? 'All statuses' : STATUS_NAME[v]), isOn: (v) => v !== 'active' },
    { key: 'source', label: 'Source', options: [['', 'All sources'], ...sources.map((s) => [s, s])], show: (v) => v },
  ];
}

function renderChips() {
  const box = $('#chips');
  box.innerHTML = '';
  for (const def of chipDefs()) {
    const v = S.f[def.key];
    const on = def.isOn ? def.isOn(v) : v !== '' && v != null;
    const b = document.createElement('button');
    b.className = 'chip' + (on ? ' on' : '');
    b.innerHTML = `${esc(on ? def.show(v) : def.label)}<svg viewBox="0 0 16 16" class="caret"><path d="m8 11 4-5H4l4 5Z"/></svg>`;
    b.onclick = () => openMenu(b, def.options.map(([val, text]) => ({ text, on: String(val) === String(v), run: () => { S.f[def.key] = val; renderJobs(); renderChips(); } })));
    box.appendChild(b);
  }
  const star = document.createElement('button');
  star.className = 'chip' + (S.f.starred ? ' on' : '');
  star.textContent = 'Starred';
  star.onclick = () => { S.f.starred = !S.f.starred; renderChips(); renderJobs(); };
  box.appendChild(star);
  const any = S.f.score !== '' || S.f.salary !== '' || S.f.workplace || S.f.status !== 'active' || S.f.source || S.f.starred;
  if (any) {
    const r = document.createElement('button');
    r.className = 'link';
    r.textContent = 'Reset';
    r.onclick = () => { Object.assign(S.f, { score: '', salary: '', workplace: '', status: 'active', source: '', starred: false }); renderChips(); renderJobs(); };
    box.appendChild(r);
  }
}

function filtered() {
  const f = S.f;
  const q = f.q.trim().toLowerCase();
  const list = S.data.jobs.filter((j) => {
    if (f.status === 'active') { if (j.status === 'archived' || j.status === 'rejected') return false; }
    else if (f.status !== 'all' && j.status !== f.status) return false;
    if (f.score === 'unscored') { if (j.score != null) return false; }
    else if (f.score !== '' && (j.score ?? -1) < f.score) return false;
    if (f.salary === 'has') { if (j.salary_max == null) return false; }
    else if (f.salary !== '' && (j.salary_max ?? 0) < f.salary) return false;
    if (f.workplace && j.workplace !== f.workplace) return false;
    if (f.source && j.source !== f.source) return false;
    if (f.starred && !j.starred) return false;
    if (q) {
      const hay = `${j.title} ${j.company} ${j.location} ${j.notes} ${j.description}`.toLowerCase();
      if (!q.split(/\s+/).every((w) => hay.includes(w))) return false;
    }
    return true;
  });
  const by = {
    match: (a, b) => (b.score ?? -1) - (a.score ?? -1) || b.found_at - a.found_at,
    newest: (a, b) => b.found_at - a.found_at,
    salary: (a, b) => (b.salary_max ?? -1) - (a.salary_max ?? -1) || (b.score ?? -1) - (a.score ?? -1),
  }[f.sort];
  return list.sort(by);
}

/* ================================================================ menu */

function openMenu(anchor, items, alignRight = false) {
  const m = $('#menu');
  m.innerHTML = '';
  for (const it of items) {
    if (it.sep) { m.appendChild(document.createElement('hr')); continue; }
    if (it.label) { const l = document.createElement('div'); l.className = 'label'; l.textContent = it.label; m.appendChild(l); continue; }
    if (it.node) { m.appendChild(it.node); continue; }
    const b = document.createElement('button');
    b.className = 'item';
    b.innerHTML = `<span class="tick">${it.on ? '✓' : ''}</span><span>${esc(it.text)}</span>`;
    b.onclick = () => { closeMenu(); it.run(); };
    m.appendChild(b);
  }
  m.hidden = false;
  const r = anchor.getBoundingClientRect();
  const w = m.offsetWidth;
  let left = alignRight ? r.right - w : r.left;
  left = Math.max(8, Math.min(left, innerWidth - w - 8));
  m.style.left = left + 'px';
  m.style.top = r.bottom + 6 + 'px';
  setTimeout(() => document.addEventListener('mousedown', outside), 0);
}
function outside(e) { if (!$('#menu').contains(e.target)) closeMenu(); }
function closeMenu() { $('#menu').hidden = true; document.removeEventListener('mousedown', outside); }

/* ================================================================ jobs view */

function logo(j, cls = '') {
  const name = j.company || j.title || '?';
  const initials = name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join('').toUpperCase();
  const palette = ['#56687a', '#6c5f8d', '#45627e', '#7a6a58', '#4f6d7a', '#7b5a70', '#5b6b8c'];
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const bg = palette[h % palette.length];
  const img = j.logo ? `<img src="${esc(j.logo)}" alt="" onerror="this.remove()">` : '';
  return `<div class="logo ${cls}" style="background:${bg}">${img}${esc(initials)}</div>`;
}

function matchMini(j) {
  if (j.score == null) return '<span>Not scored</span>';
  return `<span class="match"><span class="mini"><i style="width:${j.score}%"></i></span>${j.score}% match</span>`;
}

function renderJobs() {
  const list = filtered();
  const el = $('#list');
  const total = S.data.jobs.length;
  if (!total) {
    el.innerHTML = '';
    $('#detail').innerHTML = welcome();
    bindWelcome($('#detail'));
    return;
  }
  const sortName = { match: 'Best match', newest: 'Newest', salary: 'Highest salary' }[S.f.sort];
  let html = `<div class="list-head"><div><h2>Your jobs</h2><div class="sub">${list.length} of ${total}</div></div>
    <button class="sort" id="sort">Sort: ${sortName} ▾</button></div>`;
  if (!list.length) html += `<div class="empty"><h3>No jobs match</h3><p>Loosen a filter or clear the search.</p></div>`;
  for (const j of list) {
    const place = [j.location, j.workplace && `(${j.workplace})`].filter(Boolean).join(' ');
    const meta = [matchMini(j)];
    if (j.status !== 'new') meta.push(`<span class="tag">${esc(STATUS_NAME[j.status] || j.status)}</span>`);
    if (j.starred) meta.push('<span class="star">★</span>');
    meta.push(`<span>${ago(j.found_at)}</span>`);
    html += `<div class="job${j.id === S.sel ? ' sel' : ''}" data-id="${esc(j.id)}">
      ${logo(j)}
      <div class="jb">
        <div class="jt">${esc(j.title || 'Untitled job')}</div>
        <div class="jc">${esc(j.company)}</div>
        <div class="jl">${esc(place)}</div>
        ${j.salary ? `<div class="js">${esc(j.salary)}</div>` : ''}
        <div class="jm">${meta.join('<span class="sep"></span>')}</div>
      </div>
      <button class="icon-btn x" title="${j.status === 'archived' ? 'Delete' : 'Archive'}"><svg viewBox="0 0 24 24"><path d="M13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4 6.4 5l5.6 5.6L17.6 5 19 6.4 13.4 12Z"/></svg></button>
    </div>`;
  }
  const scroll = el.scrollTop;
  el.innerHTML = html;
  el.scrollTop = scroll;
  $('#sort').onclick = (e) => openMenu(e.currentTarget, [['match', 'Best match'], ['newest', 'Newest'], ['salary', 'Highest salary']]
    .map(([k, t]) => ({ text: t, on: S.f.sort === k, run: () => { S.f.sort = k; renderJobs(); } })), true);
  for (const row of el.querySelectorAll('.job')) {
    row.onclick = (e) => {
      if (e.target.closest('.x')) return;
      select(row.dataset.id);
    };
    row.querySelector('.x').onclick = async () => {
      const j = jobById(row.dataset.id);
      if (j.status === 'archived') {
        S.data.jobs = S.data.jobs.filter((x) => x.id !== j.id);
        await api('delete_job', { id: j.id });
        toast('Deleted');
      } else {
        await patch(j.id, { status: 'archived' });
        toast('Archived. Find it under Status → Archived.');
      }
      if (S.sel === j.id) S.sel = null;
      renderJobs();
    };
  }
  if (!S.sel || !list.some((j) => j.id === S.sel)) S.sel = list[0] ? list[0].id : null;
  el.querySelector(`.job[data-id="${S.sel}"]`)?.classList.add('sel');
  renderDetail();
  renderBadge();
}

function select(id) {
  S.sel = id;
  for (const r of document.querySelectorAll('.job')) r.classList.toggle('sel', r.dataset.id === id);
  renderDetail();
  $('#detail').scrollTop = 0;
}

function descHtml(text) {
  let out = '';
  let inList = false;
  for (const raw of String(text || '').split('\n')) {
    const line = raw.trim();
    if (!line) continue;
    if (line.startsWith('- ')) {
      if (!inList) { out += '<ul>'; inList = true; }
      out += `<li>${esc(line.slice(2))}</li>`;
      continue;
    }
    if (inList) { out += '</ul>'; inList = false; }
    if (line.startsWith('## ')) out += `<h4>${esc(line.slice(3))}</h4>`;
    else out += `<p>${esc(line)}</p>`;
  }
  if (inList) out += '</ul>';
  return out;
}

function renderDetail() {
  const el = $('#detail');
  const j = jobById(S.sel);
  if (!j) {
    el.innerHTML = S.data.jobs.length ? '<div class="empty"><h3>Pick a job</h3><p>Its details and the AI\'s notes show here.</p></div>' : '';
    return;
  }
  const meta = [j.location, j.posted, j.applicants].filter(Boolean).map(esc).join(' · ');
  const pills = [j.salary, j.workplace, j.employment, j.seniority && j.seniority !== 'Not Applicable' ? j.seniority : ''].filter(Boolean)
    .map((p) => `<span class="pill">${esc(p)}</span>`).join('');
  const options = STATUS.map(([k, t]) => `<option value="${k}"${k === j.status ? ' selected' : ''}>${t}</option>`).join('');

  let match;
  if (j.score != null) {
    const pts = (j.strengths || []).map((s) => `<li><div class="kind ${esc(s.kind)}">${esc(KIND[s.kind] || s.kind || 'Note')}</div>
      <div><div class="pt">${esc(s.text)}</div>${s.action ? `<div class="pa">${esc(s.action)}</div>` : ''}</div></li>`).join('');
    const gaps = (j.gaps || []).map((g) => `<li>${esc(g)}</li>`).join('');
    match = `<div class="box">
      <div class="box-head"><h2>How you match</h2><div class="score"><div class="meter"><i style="width:${j.score}%"></i></div>${j.score}%</div></div>
      ${j.fit ? `<p class="fit">${esc(j.fit)}</p>` : ''}
      ${pts ? `<h3>Strong points to lead with</h3><ul class="points">${pts}</ul>` : ''}
      ${gaps ? `<h3>Gaps</h3><ul class="gaps">${gaps}</ul>` : ''}
    </div>`;
  } else {
    match = `<div class="box">
      <div class="box-head"><h2>How you match</h2></div>
      <p class="hint">${j.details ? 'Not scored yet. Ask your AI assistant to go through your new jobs:' : 'LinkedIn didn\'t return this job\'s description yet. The next scan tries again.'}</p>
      ${j.details ? `<div class="prompt"><code>${esc(ASK_SCORE)}</code><button class="btn small plain" data-copy="${esc(ASK_SCORE)}">Copy</button></div>` : ''}
    </div>`;
  }

  el.innerHTML = `<div class="d">
    <div class="d-co">${logo(j, 'sm')}<span class="name">${j.company_url ? `<a href="#" data-open="${esc(j.company_url)}">${esc(j.company)}</a>` : esc(j.company)}</span>
      <div class="acts">
        <button class="icon-btn${j.starred ? ' on' : ''}" id="d-star" title="${j.starred ? 'Unstar' : 'Star'}"><svg viewBox="0 0 24 24"><path d="${j.starred ? 'M12 2.5l2.9 6.2 6.6.7-5 4.5 1.4 6.6L12 17.2l-5.9 3.3 1.4-6.6-5-4.5 6.6-.7L12 2.5Z' : 'M12 2.5l2.9 6.2 6.6.7-5 4.5 1.4 6.6L12 17.2l-5.9 3.3 1.4-6.6-5-4.5 6.6-.7L12 2.5Zm0 4.4-1.6 3.5-3.8.4 2.9 2.6-.8 3.8 3.3-1.9 3.3 1.9-.8-3.8 2.9-2.6-3.8-.4L12 6.9Z'}"/></svg></button>
        <button class="icon-btn" id="d-more" title="More"><svg viewBox="0 0 24 24"><path d="M5 10.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3Zm7 0a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3Zm7 0a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3Z"/></svg></button>
      </div>
    </div>
    <h1>${esc(j.title || 'Untitled job')}</h1>
    <div class="d-meta">${meta}</div>
    ${pills ? `<div class="pills">${pills}</div>` : '<div style="height:14px"></div>'}
    <div class="d-btns">
      <button class="btn primary" data-open="${esc(j.url)}">Open on LinkedIn
        <svg viewBox="0 0 16 16"><path d="M9 2h5v5h-1.5V4.6L7.6 9.5 6.5 8.4l4.9-4.9H9V2ZM3.5 4H7v1.5H3.5v7h7V9H12v3.5c0 .8-.7 1.5-1.5 1.5h-7c-.8 0-1.5-.7-1.5-1.5v-7C2 4.7 2.7 4 3.5 4Z"/></svg></button>
      <span class="select"><select id="d-status">${options}</select><svg viewBox="0 0 16 16"><path d="m8 11 4-5H4l4 5Z"/></svg></span>
    </div>
    ${match}
    <div class="sec"><h2>Notes</h2><textarea class="notes" id="d-notes" placeholder="Contacts, dates, what you sent, what to prepare…">${esc(j.notes)}</textarea></div>
    <div class="sec"><h2>About the job</h2>
      ${j.description ? `<div class="desc clamp" id="d-desc">${descHtml(j.description)}</div><button class="link more" id="d-more-desc">Show more</button>` : '<p class="hint">No description yet.</p>'}
    </div>
    <div class="foot">Found ${ago(j.found_at)} ago via ${esc(j.source || 'LinkedIn')} · Job ${esc(j.id)}</div>
  </div>`;

  el.querySelectorAll('[data-open]').forEach((a) => (a.onclick = (e) => { e.preventDefault(); api('open', { target: a.dataset.open }); }));
  el.querySelectorAll('[data-copy]').forEach((b) => (b.onclick = () => copy(b.dataset.copy)));
  $('#d-status').onchange = async (e) => { await patch(j.id, { status: e.target.value }); renderJobs(); };
  $('#d-star').onclick = async () => { await patch(j.id, { starred: !j.starred }); renderJobs(); };
  const notes = $('#d-notes');
  let t;
  notes.oninput = () => { clearTimeout(t); t = setTimeout(() => patch(j.id, { notes: notes.value }), 500); };
  notes.onblur = () => { clearTimeout(t); if (notes.value !== j.notes) patch(j.id, { notes: notes.value }); };
  const more = $('#d-more-desc');
  if (more) {
    const d = $('#d-desc');
    if (d.scrollHeight <= d.clientHeight + 4) { d.classList.remove('clamp'); more.remove(); }
    else more.onclick = () => { const c = d.classList.toggle('clamp'); more.textContent = c ? 'Show more' : 'Show less'; };
  }
  $('#d-more').onclick = (e) => openMenu(e.currentTarget, [
    { text: 'Copy link', run: () => copy(j.url) },
    ...(j.score != null ? [{ text: 'Clear the AI score', run: async () => { await api('update_job', { id: j.id, patch: { clearScore: true } }); load(); } }] : []),
    { sep: true },
    { text: j.status === 'archived' ? 'Delete for good' : 'Archive', run: async () => {
      if (j.status === 'archived') { await api('delete_job', { id: j.id }); S.data.jobs = S.data.jobs.filter((x) => x.id !== j.id); }
      else await patch(j.id, { status: 'archived' });
      S.sel = null; renderJobs();
    } },
  ], true);
}

function welcome() {
  return `<div class="empty" style="padding-top:72px">
    <h3>Let's find you some jobs</h3>
    <p>Shortlist reads LinkedIn in your Chrome, then your AI assistant scores each job against your CV and points out what will make you stand out.</p>
    <div class="btns">
      <button class="btn primary" data-scan="recommended">Scan my LinkedIn preferences</button>
      <button class="btn plain" data-go="settings">Set up Chrome and AI</button>
    </div>
    <p style="margin-top:28px">Or ask your assistant:</p>
    <div class="prompt" style="max-width:520px;margin:0 auto"><code>${esc(ASK_FULL)}</code><button class="btn small plain" data-copy="${esc(ASK_FULL)}">Copy</button></div>
  </div>`;
}

function bindWelcome(el) {
  el.querySelectorAll('[data-scan]').forEach((b) => (b.onclick = () => runScan({ mode: b.dataset.scan })));
  el.querySelectorAll('[data-go]').forEach((b) => (b.onclick = () => go(b.dataset.go)));
  el.querySelectorAll('[data-copy]').forEach((b) => (b.onclick = () => copy(b.dataset.copy)));
}

function renderBadge() {
  const n = S.data.jobs.filter((j) => j.status === 'new' && j.score == null && j.details).length;
  const b = $('#badge');
  b.hidden = !n;
  b.textContent = n > 99 ? '99+' : n;
  b.title = `${n} not scored yet`;
}

/* ================================================================ scanning */

async function runScan(params) {
  try {
    await api('start_scan', { params });
    S.scanDismissed = false;
    pollStatus();
    if (!S.data.chrome) toast('Waiting for Chrome. Open Chrome with the Shortlist extension, signed in to LinkedIn.', 6000);
  } catch (e) {
    toast(String(e));
  }
}

function scanMenu(anchor) {
  const items = [{ label: 'From LinkedIn' }, { text: 'My LinkedIn job preferences', run: () => runScan({ mode: 'recommended' }) }];
  if (S.data.searches.length) {
    items.push({ sep: true }, { label: 'Saved searches' });
    for (const s of S.data.searches) items.push({ text: s.name || s.keywords, run: () => runScan({ mode: 'search', ...s }) });
  }
  const form = document.createElement('form');
  form.innerHTML = `<input class="field" name="k" placeholder="Keywords, e.g. full stack engineer" required>
    <input class="field" name="l" placeholder="Location (optional)">
    <label class="check"><input type="checkbox" name="r"> Remote only</label>
    <div class="row"><button class="btn primary small" type="submit">Search</button><button class="link" type="button" data-save>Save search</button></div>`;
  const read = () => ({ mode: 'search', keywords: form.k.value.trim(), location: form.l.value.trim(), remote: form.r.checked, days: 14 });
  form.onsubmit = (e) => { e.preventDefault(); closeMenu(); runScan(read()); };
  form.querySelector('[data-save]').onclick = async () => {
    const p = read();
    if (!p.keywords) return form.k.focus();
    await api('set_searches', { list: [...S.data.searches, { name: p.keywords, keywords: p.keywords, location: p.location, remote: p.remote, days: 14 }] });
    closeMenu();
    toast('Search saved');
    load();
  };
  items.push({ sep: true }, { label: 'New search' }, { node: form });
  openMenu(anchor, items, true);
}

function renderScan(scan) {
  const line = $('#scanline');
  if (!scan || S.scanDismissed === scan.id) { line.hidden = true; return; }
  const busy = scan.state === 'queued' || scan.state === 'running';
  line.hidden = false;
  line.classList.toggle('busy', busy);
  const pct = scan.total ? Math.round((scan.done / scan.total) * 100) : 0;
  $('#scan-fill').style.width = (busy ? pct : 100) + '%';
  let text;
  if (scan.state === 'queued') text = S.data.chrome ? `${scan.label}: starting…` : `${scan.label}: waiting for Chrome with the Shortlist extension`;
  else if (busy) text = `${scan.label}: ${scan.message}`;
  else if (scan.state === 'done') text = `${scan.label}: ${scan.added} new job${scan.added === 1 ? '' : 's'}. ${scan.message === 'Done' ? '' : scan.message}`;
  else if (scan.state === 'cancelled') text = `${scan.label}: cancelled`;
  else text = `${scan.label}: ${scan.message}`;
  $('#scan-text').textContent = text;
  $('#scan-cancel').hidden = !busy;
  $('#scan-close').hidden = busy;
  $('#scan-close').onclick = () => { S.scanDismissed = scan.id; line.hidden = true; };
}

let statusTimer = null;
async function pollStatus() {
  clearTimeout(statusTimer);
  try {
    const st = await api('get_status');
    S.data.chrome = st.chrome;
    S.data.scan = st.scan;
    renderConn();
    renderScan(st.scan);
    const busy = st.scan && (st.scan.state === 'queued' || st.scan.state === 'running');
    statusTimer = setTimeout(pollStatus, busy ? 1500 : 8000);
  } catch {
    statusTimer = setTimeout(pollStatus, 8000);
  }
}

function renderConn() {
  const c = $('#conn');
  c.classList.toggle('on', !!S.data.chrome);
  $('#conn-text').textContent = S.data.chrome ? 'Chrome connected' : 'Chrome not connected';
  c.title = S.data.chrome ? 'The Shortlist extension in Chrome is connected.' : 'Open Chrome with the Shortlist extension (Settings explains how).';
}

/* ================================================================ pipeline */

function renderPipeline() {
  const el = $('#view-pipeline');
  const q = S.f.q.trim().toLowerCase();
  const jobs = S.data.jobs.filter((j) => !q || `${j.title} ${j.company}`.toLowerCase().includes(q));
  let html = '<div class="board">';
  for (const st of BOARD) {
    const list = jobs.filter((j) => j.status === st).sort((a, b) => b.updated_at - a.updated_at);
    html += `<div class="col" data-status="${st}"><div class="col-head">${STATUS_NAME[st]} <span>${list.length}</span></div>`;
    if (!list.length) html += `<div class="none">${st === 'saved' ? 'Set a job to Saved, or drag one here.' : 'Drag jobs here'}</div>`;
    for (const j of list) {
      html += `<div class="bcard" draggable="true" data-id="${esc(j.id)}">
        <div class="t">${esc(j.title)}</div><div class="c">${esc(j.company)}</div>
        <div class="m">${j.score != null ? `<span class="mini"><i style="width:${j.score}%"></i></span>${j.score}%` : ''}${j.salary_max ? ` · ${esc(money(j.salary_max, j.currency))}` : ''}${j.starred ? ' · <span class="star">★</span>' : ''}</div>
      </div>`;
    }
    html += '</div>';
  }
  el.innerHTML = html + '</div>';
  for (const card of el.querySelectorAll('.bcard')) {
    card.ondragstart = (e) => e.dataTransfer.setData('text/plain', card.dataset.id);
    card.onclick = () => { S.sel = card.dataset.id; S.f.status = 'all'; go('jobs'); };
  }
  for (const col of el.querySelectorAll('.col')) {
    col.ondragover = (e) => { e.preventDefault(); col.classList.add('drop'); };
    col.ondragleave = () => col.classList.remove('drop');
    col.ondrop = async (e) => {
      e.preventDefault();
      col.classList.remove('drop');
      const id = e.dataTransfer.getData('text/plain');
      if (id) { await patch(id, { status: col.dataset.status }); renderPipeline(); }
    };
  }
}

/* ================================================================ ideas */

function renderIdeas() {
  const el = $('#view-ideas');
  const min = S.ideasMin;
  const groups = { build: [], domain: [], experience: [] };
  for (const j of S.data.jobs) {
    if (j.score == null || j.score < min || j.status === 'archived' || j.status === 'rejected') continue;
    for (const s of j.strengths || []) if (groups[s.kind]) groups[s.kind].push({ s, j });
  }
  const section = (kind, title, sub) => {
    const list = groups[kind].sort((a, b) => b.j.score - a.j.score);
    let h = `<div class="panel"><h2>${title}</h2><div class="sub">${sub}</div>`;
    if (!list.length) h += '<div class="note">Nothing yet. These show up once your assistant scores jobs.</div>';
    for (const { s, j } of list) {
      h += `<div class="idea"><div style="flex:1">
        <div class="what">${esc(s.text)}</div>${s.action ? `<div class="how">${esc(s.action)}</div>` : ''}
        <div class="for"><a href="#" data-job="${esc(j.id)}">${esc(j.title)} · ${esc(j.company)}</a><span class="note">${j.score}% match</span></div>
      </div></div>`;
    }
    return h + '</div>';
  };
  el.innerHTML = `<div class="page-in">
    <div class="row" style="justify-content:space-between">
      <div class="note">From jobs scored ${min}% or higher</div>
      <div class="seg">${[0, 50, 60, 75].map((n) => `<button data-min="${n}" class="${n === min ? 'on' : ''}">${n ? n + '+' : 'All'}</button>`).join('')}</div>
    </div>
    ${section('build', 'Things to build', 'Jobs that want a tool or product built. Making a small version first gives you something concrete to show.')}
    ${section('domain', 'Domains they want', 'Sectors these employers care about. Bring up any work you have done in them.')}
    ${section('experience', 'Experience to highlight', 'Things you have done before that these jobs ask for.')}
  </div>`;
  el.querySelectorAll('[data-min]').forEach((b) => (b.onclick = () => { S.ideasMin = +b.dataset.min; renderIdeas(); }));
  el.querySelectorAll('[data-job]').forEach((a) => (a.onclick = (e) => { e.preventDefault(); S.sel = a.dataset.job; go('jobs'); }));
}

/* ================================================================ profile */

function renderProfile() {
  const d = S.data;
  const el = $('#view-profile');
  const files = d.docs.map((f) => `<li><div class="ficon">${esc((f.name.split('.').pop() || '').slice(0, 4))}</div>
    <div class="fname">${esc(f.name)}</div><div class="fsize">${Math.max(1, Math.round(f.size / 1024))} KB</div>
    <button class="icon-btn" data-rm="${esc(f.name)}" title="Remove"><svg viewBox="0 0 24 24"><path d="M13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4 6.4 5l5.6 5.6L17.6 5 19 6.4 13.4 12Z"/></svg></button></li>`).join('');
  const rows = d.searches.map((s, i) => `<tr data-i="${i}">
    <td><input class="field" data-k="keywords" value="${esc(s.keywords)}"></td>
    <td><input class="field" data-k="location" value="${esc(s.location)}" placeholder="Anywhere"></td>
    <td class="c"><input type="checkbox" data-k="remote"${s.remote ? ' checked' : ''}></td>
    <td class="a"><button class="btn small" data-run="${i}">Run</button> <button class="icon-btn" data-del="${i}" title="Remove"><svg viewBox="0 0 24 24"><path d="M13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4 6.4 5l5.6 5.6L17.6 5 19 6.4 13.4 12Z"/></svg></button></td>
  </tr>`).join('');

  el.innerHTML = `<div class="page-in">
    <div class="panel">
      <h2>Documents about you</h2>
      <div class="sub">CVs, portfolios, notes in Markdown, anything. Your AI assistant reads these once and keeps a short summary, so it doesn't spend tokens re-reading them.</div>
      ${files ? `<ul class="files">${files}</ul>` : '<div class="note" style="margin-bottom:14px">No documents yet.</div>'}
      <div class="row"><button class="btn primary" id="p-add">Add files</button><button class="btn plain" id="p-folder">Open folder</button></div>
    </div>
    <div class="panel">
      <h2>What you're looking for</h2>
      <div class="sub">Plain words. The AI weighs these when scoring: places, salary floor, company size, deal-breakers.</div>
      <textarea class="field" id="p-pref" placeholder="e.g. London or remote, £70K+, product companies, no agencies, no crypto.">${esc(d.preferences)}</textarea>
      <div class="note" id="p-pref-state" style="margin-top:6px">Saved as you type.</div>
    </div>
    <div class="panel">
      <h2>Your AI summary</h2>
      <div class="sub">Written by your assistant from the documents above.</div>
      ${d.summary ? `<div class="summary" id="p-sum">${esc(d.summary)}</div>` : '<div class="note" style="margin-bottom:10px">None yet.</div>'}
      ${d.stale ? '<div class="note warn">Your documents changed since this was written. Ask your assistant to refresh it.</div>' : ''}
      <div class="prompt" style="margin-top:10px"><code>Read my documents in Shortlist and update my profile summary.</code><button class="btn small plain" data-copy="Read my documents in Shortlist and update my profile summary.">Copy</button></div>
    </div>
    <div class="panel">
      <h2>Saved searches</h2>
      <div class="sub">Custom LinkedIn searches on top of your LinkedIn preferences. Your assistant can add these from your documents.</div>
      ${rows ? `<table class="searches"><thead><tr><th>Keywords</th><th>Location</th><th style="text-align:center">Remote</th><th></th></tr></thead><tbody>${rows}</tbody></table>` : ''}
      <button class="btn plain" id="p-new">Add a search</button>
    </div>
  </div>`;

  $('#p-add').onclick = async () => { const n = await api('add_docs'); if (n) toast(`Added ${n} file${n === 1 ? '' : 's'}`); load(); };
  $('#p-folder').onclick = () => api('open', { target: 'docs' });
  el.querySelectorAll('[data-rm]').forEach((b) => (b.onclick = async () => { await api('remove_doc', { name: b.dataset.rm }); load(); }));
  el.querySelectorAll('[data-copy]').forEach((b) => (b.onclick = () => copy(b.dataset.copy)));
  const pref = $('#p-pref');
  let t;
  pref.oninput = () => { clearTimeout(t); t = setTimeout(async () => { d.preferences = pref.value; await api('set_preferences', { text: pref.value }); }, 500); };

  const saveSearches = () => api('set_searches', { list: d.searches });
  el.querySelectorAll('.searches tr[data-i]').forEach((tr) => {
    const s = d.searches[+tr.dataset.i];
    tr.querySelectorAll('[data-k]').forEach((inp) => {
      inp.onchange = () => {
        const k = inp.dataset.k;
        s[k] = inp.type === 'checkbox' ? inp.checked : inp.value.trim();
        if (k === 'keywords') s.name = s.keywords;
        saveSearches();
      };
    });
  });
  el.querySelectorAll('[data-run]').forEach((b) => (b.onclick = () => { runScan({ mode: 'search', ...d.searches[+b.dataset.run] }); go('jobs'); }));
  el.querySelectorAll('[data-del]').forEach((b) => (b.onclick = async () => { d.searches.splice(+b.dataset.del, 1); await saveSearches(); load(); }));
  $('#p-new').onclick = () => {
    d.searches.push({ name: '', keywords: '', location: '', remote: false, days: 14 });
    renderProfile();
    el.querySelector('.searches tr:last-child [data-k="keywords"]').focus();
  };
}

/* ================================================================ settings */

function renderSettings() {
  const d = S.data;
  const el = $('#view-settings');
  const claudeCmd = `claude mcp add --transport http --scope user shortlist ${d.mcpUrl}`;
  const codexCfg = `[mcp_servers.shortlist]  url = "${d.mcpUrl}"`;
  el.innerHTML = `<div class="page-in">
    <div class="panel">
      <h2>Connect your AI assistant</h2>
      <div class="sub">Shortlist has a built-in MCP server. Your assistant does the reading and scoring; Shortlist keeps everything.</div>
      <div class="row" style="margin-bottom:12px"><button class="btn primary" data-ai="claude">Add to Claude Code</button><button class="btn" data-ai="codex">Add to Codex</button></div>
      <div class="note">Or do it yourself. Claude Code:</div>
      <div class="code"><span>${esc(claudeCmd)}</span><button class="btn small plain" data-copy="${esc(claudeCmd)}">Copy</button></div>
      <div class="note">Codex, in ~/.codex/config.toml:</div>
      <div class="code"><span>${esc(codexCfg)}</span><button class="btn small plain" data-copy="${esc(`[mcp_servers.shortlist]\nurl = "${d.mcpUrl}"`)}">Copy</button></div>
      <div class="note">Then ask it: “${esc(ASK_FULL)}”</div>
    </div>
    <div class="panel">
      <h2>Chrome extension</h2>
      <div class="sub">Reads LinkedIn in your own signed-in Chrome, so there are no API keys and nothing to log in to here. ${d.chrome ? '<b>Connected.</b>' : 'Not connected yet.'}</div>
      <ol class="steps">
        <li>Open <a href="#" id="s-chrome">chrome://extensions</a> and turn on <b>Developer mode</b> (top right).</li>
        <li>Click <b>Load unpacked</b> and pick this folder: <a href="#" id="s-ext">${esc(d.extDir)}</a></li>
        <li>Stay signed in to LinkedIn in Chrome. The extension opens a background tab during scans and closes it after.</li>
      </ol>
      <div class="code"><span>${esc(d.extDir)}</span><button class="btn small plain" data-copy="${esc(d.extDir)}">Copy path</button></div>
    </div>
    <div class="panel">
      <h2>Scanning</h2>
      <div class="sub">Scans go slowly on purpose (about a second between jobs) so LinkedIn doesn't flag your account.</div>
      <div class="kv">
        <label for="s-limit">New jobs per scan</label>
        <div><input class="field" id="s-limit" type="number" min="1" max="100" value="${d.scanLimit}" style="width:100px"></div>
        <label>Your data</label>
        <div><a href="#" id="s-data">${esc(d.dataDir)}</a></div>
      </div>
    </div>
  </div>`;
  el.querySelectorAll('[data-copy]').forEach((b) => (b.onclick = () => copy(b.dataset.copy)));
  el.querySelectorAll('[data-ai]').forEach((b) => (b.onclick = async () => {
    b.disabled = true;
    try { toast(await api('connect_ai', { kind: b.dataset.ai }), 5000); } catch (e) { toast(String(e), 6000); }
    b.disabled = false;
  }));
  $('#s-chrome').onclick = (e) => { e.preventDefault(); api('open', { target: 'chrome-extensions' }).catch(() => copy('chrome://extensions')); };
  $('#s-ext').onclick = (e) => { e.preventDefault(); api('open', { target: 'extension' }); };
  $('#s-data').onclick = (e) => { e.preventDefault(); api('open', { target: 'data' }); };
  $('#s-limit').onchange = (e) => api('set_scan_limit', { limit: Math.max(1, Math.min(100, +e.target.value || 30)) });
}

/* ================================================================ shell */

function go(view) {
  S.view = view;
  for (const b of document.querySelectorAll('#nav button')) b.classList.toggle('on', b.dataset.view === view);
  for (const v of ['jobs', 'pipeline', 'ideas', 'profile', 'settings']) $(`#view-${v}`).hidden = v !== view;
  $('#bar').hidden = view !== 'jobs';
  render();
}

function render() {
  renderConn();
  renderBadge();
  if (S.data.scan !== undefined) renderScan(S.data.scan);
  // Don't redraw a page while the user is typing in it.
  const active = document.activeElement;
  const typing = active && /INPUT|TEXTAREA/.test(active.tagName) && active.id !== 'q' && !$('#menu').contains(active);
  if (S.view === 'jobs') {
    renderChips();
    if (typing && active.id === 'd-notes') { renderListOnly(); return; }
    renderJobs();
  } else if (typing) {
    return;
  } else if (S.view === 'pipeline') renderPipeline();
  else if (S.view === 'ideas') renderIdeas();
  else if (S.view === 'profile') renderProfile();
  else if (S.view === 'settings') renderSettings();
}

function renderListOnly() {
  const keep = $('#detail').innerHTML;
  const notes = $('#d-notes');
  const val = notes && notes.value;
  const pos = notes && notes.selectionStart;
  const detailScroll = $('#detail').scrollTop;
  const old = renderDetail;
  renderDetail = () => {};
  try { renderJobs(); } finally { renderDetail = old; }
  if (!$('#d-notes')) { $('#detail').innerHTML = keep; renderDetail(); }
  const n = $('#d-notes');
  if (n && val != null) { n.value = val; n.focus(); n.selectionStart = n.selectionEnd = pos; }
  $('#detail').scrollTop = detailScroll;
}

document.querySelectorAll('#nav button').forEach((b) => (b.onclick = () => go(b.dataset.view)));
$('#scan-btn').onclick = (e) => scanMenu(e.currentTarget);
$('#scan-cancel').onclick = async () => { await api('cancel_scan'); pollStatus(); };
$('#q').oninput = (e) => { S.f.q = e.target.value; if (S.view === 'jobs') renderJobs(); else if (S.view === 'pipeline') renderPipeline(); };
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') closeMenu();
  if ((e.ctrlKey && e.key === 'f') || (e.key === '/' && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName))) { e.preventDefault(); $('#q').focus(); }
  if (S.view === 'jobs' && (e.key === 'ArrowDown' || e.key === 'ArrowUp') && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)) {
    const list = filtered();
    const i = list.findIndex((j) => j.id === S.sel);
    const next = list[Math.max(0, Math.min(list.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))];
    if (next) { e.preventDefault(); select(next.id); $(`.job[data-id="${next.id}"]`)?.scrollIntoView({ block: 'nearest' }); }
  }
});

if (T) T.event.listen('changed', reloadSoon);
load().then(pollStatus);

/* ================================================================ browser preview (no Tauri) */

function mock(cmd, args) {
  if (!mock.db) {
    const now = Math.floor(Date.now() / 1000);
    mock.db = {
      jobs: [
        { id: '4012345678', url: 'https://www.linkedin.com/jobs/view/4012345678/', title: 'Full Stack Product Engineer', company: 'Northwind Learning', location: 'City of Westminster, England', workplace: 'On-site', salary: '70K GBP/yr - 100K GBP/yr', salary_min: 70000, salary_max: 100000, currency: 'GBP', posted: '5 days ago', applicants: '47 applicants', seniority: 'Mid-Senior level', employment: 'Full-time', source: 'LinkedIn preferences', found_at: now - 7200, details: true, status: 'new', starred: true, notes: '', updated_at: now,
          score: 86, fit: 'Strong stack overlap (TypeScript, React, Node) and you have shipped tools for schools; on-site in London is fine per your preferences.',
          strengths: [
            { kind: 'build', text: 'Wants a timetable planner that teachers can edit together', action: 'Build a small shared timetable editor demo this week and link it in your application' },
            { kind: 'domain', text: 'Prior work with schools or education products', action: 'Lead with the school portal you built and the number of schools using it' },
            { kind: 'skill', text: 'TypeScript, React, Postgres', action: 'Put these first in your CV skills line' },
          ],
          gaps: ['No mention of Postgres at scale in your documents'],
          description: '## About the role\nWe are building the operating system for schools.\n## What you\'ll do\n- Build a timetable planner that teachers can edit together\n- Own features end to end, from database to UI\n## What we\'re looking for\n- 4+ years with TypeScript and React\n- Experience working with schools is a big plus\n## Benefits\n- Equity and yearly reviews' },
        { id: '4023456789', url: 'https://www.linkedin.com/jobs/view/4023456789/', title: 'Software Engineer, Applied AI', company: 'Circuit Labs', location: 'London', workplace: 'On-site', salary: '80K GBP/yr - 125K GBP/yr', salary_min: 80000, salary_max: 125000, currency: 'GBP', posted: '1 month ago', applicants: 'Over 200 applicants', employment: 'Full-time', source: 'Search: ai engineer in London', found_at: now - 86400, details: true, status: 'applied', starred: false, notes: 'Sent CV on Monday.', updated_at: now,
          score: 72, fit: 'Good AI tooling match; they want electronics domain knowledge you lack.', strengths: [{ kind: 'experience', text: 'Shipped LLM features to production', action: 'Mention the MCP servers you built' }], gaps: ['Hardware / PCB domain'], description: 'Build AI tools for electronics design.' },
        { id: '4034567890', url: 'https://www.linkedin.com/jobs/view/4034567890/', title: 'AI/ML & Prompt Engineer: LLM, RAG & Voice Agent', company: 'QuantumLoop', location: 'London', workplace: 'Hybrid', salary: '', posted: '2 weeks ago', employment: 'Contract', source: 'LinkedIn preferences', found_at: now - 3600, details: true, status: 'new', starred: false, notes: '', updated_at: now, score: null, strengths: [], gaps: [], description: 'Voice agents and RAG pipelines.' },
      ],
      summary: '', summaryAt: 0, stale: false, preferences: 'London or remote, £70K+.', searches: [{ name: 'ai engineer', keywords: 'ai engineer', location: 'London', remote: false, days: 14 }],
      scanLimit: 30, docs: [{ name: 'cv.pdf', size: 120000, modified: now }], chrome: true, scan: null,
      mcpUrl: 'http://127.0.0.1:12731/mcp', dataDir: 'C:\\Users\\you\\AppData\\Roaming\\Shortlist', extDir: 'C:\\Users\\you\\AppData\\Roaming\\Shortlist\\chrome-extension',
    };
  }
  const db = mock.db;
  switch (cmd) {
    case 'get_state': return Promise.resolve(JSON.parse(JSON.stringify(db)));
    case 'get_status': return Promise.resolve({ chrome: db.chrome, scan: db.scan });
    case 'update_job': { const j = db.jobs.find((x) => x.id === args.id); Object.assign(j, args.patch); return Promise.resolve(); }
    case 'delete_job': db.jobs = db.jobs.filter((x) => x.id !== args.id); return Promise.resolve();
    case 'set_searches': db.searches = args.list; return Promise.resolve();
    case 'start_scan': db.scan = { id: 1, label: args.params.mode === 'search' ? `Search: ${args.params.keywords}` : 'LinkedIn preferences', state: 'running', message: 'Reading job 3 of 30', found: 25, added: 2, done: 3, total: 30 }; return Promise.resolve(db.scan);
    case 'cancel_scan': db.scan.state = 'cancelled'; return Promise.resolve();
    default: return Promise.resolve(0);
  }
}
