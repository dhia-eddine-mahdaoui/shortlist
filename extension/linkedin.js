// Functions that run inside a LinkedIn tab (chrome.scripting.executeScript).
// Each one must be self-contained: Chrome copies the function's source into the page.

/** Job ids and card details from the job list that is open in the tab (signed-in LinkedIn). */
async function slHarvestDom() {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const seen = new Map();
  const idOf = (n) => {
    let id = n.getAttribute('data-job-id') || n.getAttribute('data-occludable-job-id');
    if (!id) {
      const m = (n.getAttribute('href') || '').match(/(?:\/jobs\/view\/(?:[^/?]*?-)?|currentJobId=)(\d{6,})/);
      id = m && m[1];
    }
    return id && /^\d{6,}$/.test(id) ? id : null;
  };
  const collect = () => {
    const nodes = document.querySelectorAll('[data-job-id], [data-occludable-job-id], a[href*="/jobs/view/"], a[href*="currentJobId="]');
    for (const n of nodes) {
      const id = idOf(n);
      if (!id) continue;
      const info = seen.get(id) || { id };
      const card = n.closest('li') || n;
      const lines = (card.innerText || '').split('\n').map((s) => s.trim()).filter(Boolean);
      if (lines.length > (info.n || 0)) {
        info.n = lines.length;
        const place = lines.find((l) => /\((On-site|Remote|Hybrid)\)\s*$/.test(l));
        if (place) {
          info.workplace = place.match(/\((On-site|Remote|Hybrid)\)/)[1];
          info.location = place.replace(/\((On-site|Remote|Hybrid)\)/, '').trim();
        }
        const pay = lines.find((l) => /\d/.test(l) && /[£$€₺]|\b\d+(\.\d+)?K\b|\/yr|\/hr|\/mo/.test(l) && l.length < 80 && !/applicant|ago|alumni|connection/i.test(l));
        if (pay) info.salary = pay;
        if (!info.title && lines[0] && lines[0].length < 140) info.title = lines[0].replace(/\s+with verification$/i, '');
      }
      seen.set(id, info);
    }
  };
  collect();
  // Scroll the list so lazily drawn cards fill in (ids are there either way).
  const first = document.querySelector('[data-occludable-job-id], [data-job-id]');
  let box = first && first.parentElement;
  while (box && box !== document.body) {
    const st = getComputedStyle(box);
    if (/(auto|scroll)/.test(st.overflowY) && box.scrollHeight > box.clientHeight + 10) break;
    box = box.parentElement;
  }
  for (let i = 0; i < 10; i++) {
    if (box && box !== document.body) box.scrollTop += 700; else window.scrollBy(0, 700);
    await sleep(350);
    collect();
  }
  // Ids that live only in the page data.
  if (seen.size < 3) {
    const html = document.documentElement.innerHTML;
    for (const re of [/urn:li:fsd_jobPosting:(\d{6,})/g, /urn:li:jobPosting:(\d{6,})/g, /jobPosting%3A(\d{6,})/g]) {
      for (const m of html.matchAll(re)) if (!seen.has(m[1])) seen.set(m[1], { id: m[1] });
    }
  }
  const signedOut = /\/(login|authwall|checkpoint|uas\/login|signup)/.test(location.pathname);
  return { signedOut, cards: [...seen.values()].map(({ n, ...rest }) => rest) };
}

/** Public job search (no sign-in needed), used when the signed-in list can't be read. */
async function slHarvestGuest(url) {
  const r = await fetch(url, { credentials: 'include' });
  if (r.status === 429) return { rate: true, cards: [] };
  if (!r.ok) return { cards: [] };
  const doc = new DOMParser().parseFromString(await r.text(), 'text/html');
  const t = (el, s) => ((el.querySelector(s) || {}).textContent || '').replace(/\s+/g, ' ').trim();
  const cards = [];
  for (const el of doc.querySelectorAll('[data-entity-urn*="jobPosting"]')) {
    const m = el.getAttribute('data-entity-urn').match(/(\d{6,})/);
    if (!m) continue;
    const img = el.querySelector('img[data-delayed-url]');
    cards.push({
      id: m[1],
      title: t(el, '.base-search-card__title'),
      company: t(el, '.base-search-card__subtitle'),
      location: t(el, '.job-search-card__location'),
      salary: t(el, '.job-search-card__salary-info'),
      posted: t(el, 'time'),
      logo: img ? img.getAttribute('data-delayed-url') : '',
    });
  }
  return { cards };
}

/** Full details of one job from LinkedIn's public job page. */
async function slDetail(id) {
  const r = await fetch('https://www.linkedin.com/jobs-guest/jobs/api/jobPosting/' + id, { credentials: 'include' });
  if (r.status === 429) return { rate: true };
  if (!r.ok) return { id };
  const doc = new DOMParser().parseFromString(await r.text(), 'text/html');
  const t = (s) => ((doc.querySelector(s) || {}).textContent || '').replace(/\s+/g, ' ').trim();

  const toText = (root) => {
    let out = '';
    const alone = (n) => {
      const p = n.parentElement;
      return p && p.textContent.trim() === n.textContent.trim();
    };
    const walk = (n) => {
      if (n.nodeType === 3) { out += n.textContent.replace(/\s+/g, ' '); return; }
      if (n.nodeType !== 1) return;
      const tag = n.tagName;
      if (tag === 'BR') { out += '\n'; return; }
      if (tag === 'BUTTON' || tag === 'SCRIPT' || tag === 'STYLE') return;
      const block = /^(P|DIV|UL|OL|LI|H[1-6]|SECTION|TABLE|TR)$/.test(tag);
      if (block) out += '\n';
      if (tag === 'LI') out += '- ';
      if (/^H[1-6]$/.test(tag) || ((tag === 'STRONG' || tag === 'B') && alone(n) && n.textContent.trim().length < 90)) out += '## ';
      n.childNodes.forEach(walk);
      if (block) out += '\n';
    };
    walk(root);
    return out.split('\n').map((s) => s.trim()).filter((s) => s && s !== '-' && s !== '##')
      .join('\n').replace(/\n## \n/g, '\n').trim();
  };

  const criteria = {};
  for (const li of doc.querySelectorAll('.description__job-criteria-item')) {
    const k = ((li.querySelector('.description__job-criteria-subheader') || {}).textContent || '').trim().toLowerCase();
    const v = ((li.querySelector('.description__job-criteria-text') || {}).textContent || '').replace(/\s+/g, ' ').trim();
    if (k) criteria[k] = v;
  }
  const desc = doc.querySelector('.show-more-less-html__markup') || doc.querySelector('.description__text');
  const org = doc.querySelector('.topcard__org-name-link');
  const img = doc.querySelector('img[data-delayed-url*="company-logo"]') || doc.querySelector('.top-card-layout__entity-image');
  return {
    id,
    title: t('.top-card-layout__title') || t('h2'),
    company: t('.topcard__org-name-link') || t('.topcard__flavor'),
    company_url: org ? org.getAttribute('href').split('?')[0] : '',
    logo: img ? img.getAttribute('data-delayed-url') || img.getAttribute('src') || '' : '',
    location: t('.topcard__flavor--bullet:not(.num-applicants__caption)'),
    posted: t('.posted-time-ago__text'),
    applicants: t('.num-applicants__caption') || t('.num-applicants__figure'),
    salary: t('.compensation__salary') || t('.salary'),
    seniority: criteria['seniority level'] || '',
    employment: criteria['employment type'] || '',
    description: desc ? toText(desc) : '',
    details: !!desc,
  };
}

/** The job id of the LinkedIn page that is open in the tab, if any. */
function slCurrentJobId() {
  const m = location.href.match(/(?:\/jobs\/view\/(?:[^/?]*?-)?|currentJobId=)(\d{6,})/);
  return m ? m[1] : null;
}
