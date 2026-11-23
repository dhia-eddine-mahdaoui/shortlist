const $ = (id) => document.getElementById(id);

chrome.runtime.sendMessage({ type: 'status' }, (s) => {
  const on = !!(s && s.app);
  $('dot').classList.toggle('on', on);
  $('state').textContent = on ? 'Connected to the Shortlist app' : "Shortlist app isn't running";
  $('add').disabled = !on;
  if (s && s.scan && (s.scan.state === 'running' || s.scan.state === 'queued')) {
    $('scan').textContent = `Scanning: ${s.scan.message} (${s.scan.done}/${s.scan.total})`;
  }
});

$('add').addEventListener('click', () => {
  $('add').disabled = true;
  $('msg').textContent = 'Reading the job…';
  chrome.runtime.sendMessage({ type: 'add' }, (r) => {
    $('add').disabled = false;
    if (!r || r.error) { $('msg').textContent = (r && r.error) || 'Something went wrong.'; return; }
    $('msg').textContent = r.added ? `Added: ${r.title}` : `Updated: ${r.title}`;
  });
});
