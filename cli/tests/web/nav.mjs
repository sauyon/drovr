// Browser-driven checks for the review UI's keyboard navigation.
//
// Driven by `cli/tests/web_nav.rs`, which boots `drovr serve` on a throwaway
// XDG_DATA_HOME and a headless chromium, then runs this with DROVR_BASE and
// DROVR_CDP set. Exits 0 when every check passes.
//
// No dependencies: node's global WebSocket speaks CDP directly. Real key events
// go through Input.dispatchKeyEvent so the page's own listener runs — a
// synthesized JS event would not prove the binding works.
//
// Waits are condition-polled, never fixed sleeps: the suite is on the normal
// `cargo test` path, so its cost has to stay in the low seconds.

const BASE = process.env.DROVR_BASE;
const CDP = process.env.DROVR_CDP;
if (!BASE || !CDP) { console.error('DROVR_BASE and DROVR_CDP must be set'); process.exit(2); }

const targets = await (await fetch(`${CDP}/json/list`)).json();
const page = targets.find(t => t.type === 'page');
if (!page) { console.error('no chromium page target'); process.exit(2); }
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise(r => ws.addEventListener('open', r, { once: true }));

// Hard ceiling on the whole run. Without it a wedged or crashed chromium leaves
// an awaited CDP call unsettled forever, which blocks the Rust harness inside
// Command::output() — so its KillOnDrop guards never run and both the browser
// and `drovr serve` leak. Exiting here lets the harness reap them.
const WATCHDOG_MS = 120_000;
const watchdog = setTimeout(() => {
  console.error(`\n!! nav.mjs exceeded ${WATCHDOG_MS}ms — assuming the browser wedged`);
  process.exit(2);
}, WATCHDOG_MS);
watchdog.unref?.();

let id = 0;
const pending = new Map();
ws.addEventListener('message', ev => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
});
// If the socket dies, fail every in-flight call rather than leaving them pending:
// an unsettled promise here is indistinguishable from a hang.
ws.addEventListener('close', () => {
  for (const [, settle] of pending) settle({ error: { message: 'CDP socket closed' } });
  pending.clear();
});
function send(method, params = {}) {
  const myId = ++id;
  return new Promise((res, rej) => {
    const timer = setTimeout(() => {
      pending.delete(myId);
      rej(new Error(`${method}: no CDP response within 20s`));
    }, 20_000);
    pending.set(myId, m => {
      clearTimeout(timer);
      m.error ? rej(new Error(method + ': ' + JSON.stringify(m.error))) : res(m.result);
    });
    ws.send(JSON.stringify({ id: myId, method, params }));
  });
}
async function evaluate(expr) {
  const r = await send('Runtime.evaluate', {
    expression: `(function(){ ${expr} })()`, returnByValue: true, awaitPromise: true,
  });
  if (r.exceptionDetails) throw new Error('page threw: ' + JSON.stringify(r.exceptionDetails.exception));
  return r.result.value;
}

const sleep = ms => new Promise(r => setTimeout(r, ms));

// Poll a probe until it satisfies `ok`, or give up. Every wait in this file goes
// through here so the suite costs what the page actually takes, not a worst-case
// guess baked into a sleep().
async function waitFor(probe, ok, timeoutMs = 6000, label = 'condition') {
  const deadline = Date.now() + timeoutMs;
  let last;
  for (;;) {
    last = await probe();
    if (ok(last)) return last;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${label} (last: ${JSON.stringify(last)})`);
    await sleep(50);
  }
}

const KEYS = {
  j: { key: 'j', code: 'KeyJ', vk: 74, text: 'j' },
  k: { key: 'k', code: 'KeyK', vk: 75, text: 'k' },
  g: { key: 'g', code: 'KeyG', vk: 71, text: 'g' },
  G: { key: 'G', code: 'KeyG', vk: 71, text: 'G', mods: 8 },
  h: { key: 'h', code: 'KeyH', vk: 72, text: 'h' },
  i: { key: 'i', code: 'KeyI', vk: 73, text: 'i' },
  '/': { key: '/', code: 'Slash', vk: 191, text: '/' },
  '?': { key: '?', code: 'Slash', vk: 191, text: '?', mods: 8 },
  '1': { key: '1', code: 'Digit1', vk: 49, text: '1' },
  '2': { key: '2', code: 'Digit2', vk: 50, text: '2' },
  '9': { key: '9', code: 'Digit9', vk: 57, text: '9' },
  Enter: { key: 'Enter', code: 'Enter', vk: 13 },
  Escape: { key: 'Escape', code: 'Escape', vk: 27 },
  ArrowDown: { key: 'ArrowDown', code: 'ArrowDown', vk: 40 },
  ArrowUp: { key: 'ArrowUp', code: 'ArrowUp', vk: 38 },
  'C-n': { key: 'n', code: 'KeyN', vk: 78, mods: 2 },
  'C-p': { key: 'p', code: 'KeyP', vk: 80, mods: 2 },
  'C-s': { key: 's', code: 'KeyS', vk: 83, mods: 2 },
  'C-g': { key: 'g', code: 'KeyG', vk: 71, mods: 2 },
  'M-<': { key: '<', code: 'Comma', vk: 188, mods: 1 | 8 },
  'M->': { key: '>', code: 'Period', vk: 190, mods: 1 | 8 },
};

// No sleep after a press: the page's keydown handler is synchronous, and the
// next Runtime.evaluate is a later CDP message, so it observes the settled DOM.
// Anything that kicks off async work (navigation, a fetch) is waitFor'd instead.
async function press(name) {
  const k = KEYS[name];
  if (!k) throw new Error('unknown key ' + name);
  const base = { key: k.key, code: k.code, windowsVirtualKeyCode: k.vk,
                 nativeVirtualKeyCode: k.vk, modifiers: k.mods || 0 };
  await send('Input.dispatchKeyEvent', { type: k.text ? 'keyDown' : 'rawKeyDown', ...base, text: k.text || '' });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
}
async function typeText(s) {
  for (const ch of s) {
    await send('Input.dispatchKeyEvent', { type: 'keyDown', text: ch, key: ch, unmodifiedText: ch });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch });
  }
}

// ---- Probes ----
const cursorName = () => evaluate(`
  var el = document.querySelector('#run-list-items .run-row.nav-cursor');
  return el ? el.querySelector('.run-name').textContent : null;`);
// VISIBLE rows only — rows inside a collapsed "Completed" group are off screen
// and the keyboard cursor deliberately skips them. Inlined rather than reading
// the page's RUN_ROW_SEL: probes run against a page that may still be parsing,
// and a bare reference throws ReferenceError before the script defines it. The
// two are pinned together by the drift check in the completed-sessions section.
const RUN_ROW_SEL = "#run-list-items > .run-row, #run-list-items details[open] .run-row";
const rowNames = () => evaluate(`
  return Array.from(document.querySelectorAll(${JSON.stringify(RUN_ROW_SEL)})).map(function(e){return e.querySelector('.run-name').textContent;});`);
// Every row in the DOM, collapsed or not.
const allRowNames = () => evaluate(`
  return Array.from(document.querySelectorAll('#run-list-items .run-row .run-name')).map(function(e){return e.textContent;});`);
const groupSummary = () => evaluate(`
  var s = document.querySelector('.run-group > summary');
  return s ? s.textContent : null;`);
const groupOpen = () => evaluate(`
  var g = document.querySelector('.run-group');
  return g ? g.open : null;`);
const metaFor = name => evaluate(`
  var rows = Array.from(document.querySelectorAll('#run-list-items .run-row'));
  var row = rows.find(function(r){ return r.querySelector('.run-name').textContent === ${JSON.stringify(name)}; });
  return row ? row.querySelector('.run-state').textContent : null;`);
const cursorQuestion = () => evaluate(`
  var el = document.querySelector('#questions-area .question-item.nav-cursor');
  return el ? el.querySelector('.question-prompt').textContent : null;`);
const checkedIn = qi => evaluate(`
  var it = document.querySelectorAll('#questions-area .question-item')[${qi}];
  if (!it) return null;
  var r = it.querySelector('input[type="radio"]:checked');
  return r ? r.value : null;`);
const hash = () => evaluate(`return location.hash;`);
const filterOpen = () => evaluate(`return document.getElementById('nav-filter').style.display !== 'none';`);
const helpOpen = () => evaluate(`return document.getElementById('key-help').classList.contains('open');`);
const activeId = () => evaluate(`
  var a = document.activeElement;
  return a ? (a.id || a.tagName.toLowerCase()) : null;`);

let pass = 0, fail = 0, skip = 0;
function check(label, actual, expected) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  console.log(`${ok ? '  ok  ' : '! FAIL'} ${label}` +
    (ok ? '' : `\n         expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`));
  ok ? pass++ : fail++;
}

async function goto(hashPath, ready) {
  await send('Page.navigate', { url: BASE + '/' + hashPath });
  await waitFor(ready.probe, ready.ok, 8000, ready.label);
}
const LIST_READY = { probe: rowNames, ok: r => r.length > 0, label: 'session list' };
const QUESTIONS_READY = { probe: cursorQuestion, ok: q => !!q, label: 'questions panel' };

// ---------------------------------------------------------------------------
console.log('\n== session list: motion ==');
await goto('#/', LIST_READY);
const names = await rowNames();
check('cursor starts on the first row', await cursorName(), names[0]);
await press('j');
check('j moves down', await cursorName(), names[1]);
await press('k');
check('k moves up', await cursorName(), names[0]);
await press('ArrowDown');
check('ArrowDown moves down', await cursorName(), names[1]);
await press('ArrowUp');
check('ArrowUp moves up', await cursorName(), names[0]);
await press('G');
check('G jumps to the last row', await cursorName(), names[names.length - 1]);
await press('j');
check('j at the bottom does not wrap', await cursorName(), names[names.length - 1]);
await press('g');
check('g jumps to the first row', await cursorName(), names[0]);
await press('k');
check('k at the top does not wrap', await cursorName(), names[0]);
await press('M->');
check('M-> jumps to the last row', await cursorName(), names[names.length - 1]);
await press('M-<');
check('M-< jumps to the first row', await cursorName(), names[0]);
await press('j');
await press('C-p');
check('C-p moves up', await cursorName(), names[0]);

// C-n is not a failure when the browser owns it: Chrome and Firefox reserve
// Ctrl+N for a new window on Linux/Windows and a page cannot preventDefault it.
// It does reach the page on macOS, so probe rather than assert.
await press('j');
await press('C-n');
if ((await cursorName()) === names[2]) {
  console.log('  ok   C-n moves down'); pass++;
} else {
  console.log('  skip C-n — reserved by this browser for a new window (expected off macOS)'); skip++;
}

console.log('\n== session list: completed sessions ==');
await goto('#/', LIST_READY);
check('the probe selector still matches the page\'s own navRows selector',
  await evaluate(`return RUN_ROW_SEL;`), RUN_ROW_SEL);
check('completed runs are hidden from the active list',
  (await rowNames()).filter(n => n === 'epsilon-done' || n === 'zeta-archived'), []);
check('...but they are in the DOM, inside the group',
  (await allRowNames()).filter(n => n === 'epsilon-done' || n === 'zeta-archived').sort(),
  ['epsilon-done', 'zeta-archived']);
check('the group is labelled with a count', await groupSummary(), 'Completed (2)');
check('the group starts collapsed', await groupOpen(), false);
check('a finished run reports phase progress, not its stale gate state',
  await metaFor('epsilon-done'), 'complete · 4/4');
check('an archived run says so rather than showing a live-looking "ready"',
  await metaFor('zeta-archived'), 'archived · 0/4');

// The whole point of collapsing: j/k must not walk into rows nobody can see.
await press('G');
check('G stops at the last ACTIVE row, not inside the collapsed group',
  ['epsilon-done', 'zeta-archived'].indexOf(await cursorName()), -1);

await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === true, 4000, 'group expands');
check('clicking the summary expands the group', await groupOpen(), true);
await waitFor(rowNames, r => r.indexOf('epsilon-done') !== -1, 4000, 'completed rows reachable');
check('expanded rows join the keyboard order', (await rowNames()).indexOf('epsilon-done') !== -1, true);
await press('G');
check('G now reaches a completed row',
  ['epsilon-done', 'zeta-archived'].indexOf(await cursorName()) !== -1, true);

// The list re-renders every 2s; the group must not slam shut under the reviewer.
await new Promise(r => setTimeout(r, 2600));
check('the expanded group survives the periodic re-render', await groupOpen(), true);

await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === false, 4000, 'group collapses');
check('clicking again re-collapses it', await groupOpen(), false);

// A filter force-opens the group so matches cannot hide inside it. That is a
// render decision, not the reviewer's — it must not be mistaken for one and
// written back as their persisted preference.
const storedOpen = () => evaluate(`return localStorage.getItem('drovr.completedOpen');`);
check('the collapsed preference is stored as collapsed', await storedOpen(), '0');
await press('/');
await typeText('zeta');
await waitFor(groupOpen, o => o === true, 4000, 'filter force-opens the group');
check('a completed-only match is force-shown', await groupOpen(), true);
check('...but that does NOT overwrite the reviewer\'s collapsed preference',
  await storedOpen(), '0');
await press('Escape');
await waitFor(groupOpen, o => o === false, 4000, 'group re-collapses after the filter clears');
check('clearing the filter restores the collapsed group', await groupOpen(), false);

// The 2s poll rebuilds the <details> from scratch; parsing `open` fires `toggle`
// on its own. That echo must not be mistaken for a gesture either.
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(storedOpen, v => v === '1', 4000, 'expanded preference stored');
await evaluate(`localStorage.setItem('drovr.completedOpen','sentinel'); return 1;`);
await new Promise(r => setTimeout(r, 2600));
check('a re-render while expanded does not rewrite localStorage',
  await storedOpen(), 'sentinel');
await evaluate(`localStorage.setItem('drovr.completedOpen','0'); return 1;`);

console.log('\n== session list: the Completed disclosure owns its own keys ==');
await goto('#/', LIST_READY);
await evaluate(`document.querySelector('.run-group > summary').focus(); return 1;`);
check('the summary can take DOM focus', await activeId(), 'summary');
const hashBefore = await hash();
await press('Enter');
await new Promise(r => setTimeout(r, 300));
check('Enter on the focused summary does not navigate to an unrelated run',
  await hash(), hashBefore);
check('Enter on the focused summary toggles the group instead', await groupOpen(), true);
// Hand the next section a collapsed group. This has to go through a real toggle,
// not just localStorage: `goto('#/')` only changes the hash, which does not
// reload the document, so the in-memory completedOpen would otherwise survive.
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === false, 4000, 'group collapsed for the next section');

console.log('\n== session list: filter ==');
await goto('#/', LIST_READY);
await press('g');
await press('/');
check('/ opens the filter', await filterOpen(), true);
check('the filter input takes focus', await activeId(), 'nav-filter-input');
await typeText('gamma');
await waitFor(rowNames, r => r.length === 1, 4000, 'filtered list');
check('the filter narrows the list', await rowNames(), ['gamma-review']);
await typeText('jkgh');
check('motion keys typed into the filter are text, not motion',
  await evaluate(`return document.getElementById('nav-filter-input').value;`), 'gammajkgh');
await press('Escape');
check('Escape closes the filter', await filterOpen(), false);
await waitFor(rowNames, r => r.length === names.length, 4000, 'restored list');
check('Escape restores the full list', (await rowNames()).length, names.length);
await press('C-s');
check('C-s opens the filter', await filterOpen(), true);
await press('C-g');
check('C-g closes the filter', await filterOpen(), false);

console.log('\n== session list: cursor is anchored to its run, not its slot ==');
// /api/runs sorts by most-recently-touched. Bump the LAST run so it jumps to the
// front and every other row shifts: an index-only cursor would then be pointing
// at a different session, and Enter would open the wrong one.
// Explicitly not alpha-deploy: posting a summary overwrites the target's
// summary.txt, and the detail-view checks below assert on alpha's seeded summary.
// Picking "the last row" blind silently clobbered it whenever alpha sorted last.
const bump = names.filter(n => n !== 'alpha-deploy').pop();
await press('g'); await press('j');
const anchored = await cursorName();
check('cursor sits on a row the re-sort will move', anchored, names[1]);
await evaluate(`return fetch('/api/runs/${bump}/summary',{method:'POST',headers:{'content-type':'text/plain'},body:'bump'}).then(function(){return 1;});`);
const after = await waitFor(rowNames, r => r[0] === bump, 8000, 'list re-sort');
check('the list really did re-sort', after[0], bump);
check('and the anchored run really changed index', after.indexOf(anchored) !== names.indexOf(anchored), true);
check('cursor stayed on its run across the re-sort', await cursorName(), anchored);

console.log('\n== opening a run ==');
await goto('#/', LIST_READY);
await press('/');
await typeText('alpha');
await waitFor(rowNames, r => r.length === 1, 4000, 'filtered to alpha');
// Closing the filter on the way INTO a run must not kick off a session-list
// rebuild; one that resolves after the view switched races the detail render
// and can drag the cursor with it.
await evaluate(`
  window.__lateRenders = 0;
  var orig = renderRunList;
  window.renderRunList = function() {
    if (location.hash.indexOf('#/runs/') === 0) window.__lateRenders++;
    return orig.apply(null, arguments);
  };
  return 1;`);
await press('Enter');
await waitFor(hash, h => h.indexOf('#/runs/alpha-deploy') === 0, 8000, 'run detail hash');
check('Enter opens the row under the cursor', (await hash()).indexOf('#/runs/alpha-deploy'), 0);
check('the filter closed on the way in', await filterOpen(), false);
await waitFor(cursorQuestion, q => !!q, 8000, 'questions panel');
check('no stale list rebuild races the detail view', await evaluate(`return window.__lateRenders;`), 0);

console.log('\n== run detail: what the agent asked for ==');
// The summary is the agent's own statement of what it wants reviewed, not a diff
// against a previous version — so it must show on the FIRST review too, where the
// reviewer has no other context. The turn counter is still 0 at this point.
check('the agent change summary shows on the first review (turn 0)', await evaluate(`
  return document.getElementById('summary-banner').style.display !== 'none' &&
         (document.getElementById('summary-text').textContent || '').indexOf('ready for review') !== -1;`), true);
check('...and the turn really is 0', await evaluate(`return currentTurn;`), 0);

console.log('\n== run detail: spec links ==');
// Specs cite sources as bare URLs far more often than as [txt](url), and
// markdown-it leaves bare URLs as plain text unless linkify is on — the whole
// citation list rendered unclickable. Read hrefs off the rendered spec.
const specLinks = () => evaluate(`
  return Array.prototype.map.call(
    document.querySelectorAll('#doc-content a'),
    function(a) { return { href: a.getAttribute('href'), target: a.getAttribute('target'),
                           rel: a.getAttribute('rel') }; });`);
const links = await specLinks();
// Asserted first and separately because one check below ('open in a new tab') is
// an .every(), which is vacuously true on an empty list — so "the spec rendered
// no links at all" has to fail loudly here rather than sail through as a green
// check. The .some() and expected-non-empty checks fail on their own in that case.
check('the rendered spec has links to inspect', links.length > 0, true);
check('a bare URL in the spec becomes a link',
  links.some(l => l.href === 'https://example.com/paper'), true);
check('an explicit [txt](url) link still works',
  links.some(l => l.href === 'https://example.com/docs'), true);
// fuzzyLink would match any word whose last segment is a live TLD, and `.rs` is
// one — every source file a spec names would turn into an outbound link.
check('a bare source filename is not linkified',
  links.filter(l => /\.rs$/.test(l.href || '')), []);
// Citations open in a new tab: unsubmitted answers live only in the radio buttons
// and an unsaved annotation draft only in the DOM, so navigating away loses both.
check('external spec links open in a new tab',
  links.filter(l => /^https?:/.test(l.href || '')).every(l => l.target === '_blank'), true);
// ...but an in-document fragment must stay in this tab: it scrolls the spec, and
// _blank would instead reload the whole SPA in a second tab.
check('an in-document fragment link stays in this tab',
  links.filter(l => (l.href || '').charAt(0) === '#').map(l => l.target), [null]);
// _blank without noopener hands the opened page a live window.opener back into the
// review UI. Asserted separately because the target check above would stay green if
// the rel line alone were dropped.
check('...and a new tab cannot reach back through window.opener',
  links.filter(l => l.target === '_blank').every(l => /noopener/.test(l.rel || '')), true);
// Leaving the fragment to the browser is NOT harmless here: the whole app routes off
// location.hash, so the router reads a bare fragment as "no run" and tears the detail
// view down mid-review — typed feedback and unsaved comments with it. Clicked for
// real; reading the href back would never have caught it.
// Read AFTER yielding: hashchange is queued as its own task, and route() — the thing
// that clears currentRun and hides the panel — only runs from that handler. Sampled
// synchronously, runHeld/docStillShown would read true even with the fix reverted,
// naming coverage the check did not have.
check('clicking a fragment link does not navigate out of the run', await evaluate(`
  return (async function() {
    var a = Array.prototype.filter.call(
      document.querySelectorAll('#doc-content a'),
      function(x) { return (x.getAttribute('href') || '').charAt(0) === '#'; })[0];
    if (!a) return 'no fragment link in the fixture spec';
    var beforeHash = location.hash, beforeRun = currentRun;
    a.click();
    await new Promise(function(r) { setTimeout(r, 100); });
    return { hashHeld: location.hash === beforeHash, runHeld: currentRun === beforeRun,
             docStillShown: document.getElementById('doc-panel').style.display !== 'none' };
  })();`),
  { hashHeld: true, runHeld: true, docStillShown: true });
// The same protection for `[t]()`, which renders href="" — it resolves to the current
// document and navigates for real, so "not a fragment" must not mean "leave it alone".
check('clicking an empty-href link does not navigate either', await evaluate(`
  return (async function() {
    var doc = document.getElementById('doc-content');
    var a = document.createElement('a');
    a.setAttribute('href', '');
    a.textContent = 'empty link';
    doc.appendChild(a);
    var beforeHash = location.hash, beforeRun = currentRun;
    a.click();
    await new Promise(function(r) { setTimeout(r, 100); });
    var got = { hashHeld: location.hash === beforeHash, runHeld: currentRun === beforeRun,
                docStillShown: document.getElementById('doc-panel').style.display !== 'none' };
    a.remove();
    return got;
  })();`),
  { hashHeld: true, runHeld: true, docStillShown: true });
// ...and it has somewhere to land: markdown-it emits no heading ids, so before
// idHeadings() every in-spec fragment pointed at nothing whatsoever.
check('spec headings get ids for a fragment to reach', await evaluate(`
  var h = document.querySelector('#doc-content h1');
  return h ? h.id : null;`), 'spec-h-spec-for-alpha-deploy');
// The prefix is the whole point: the spec is agent-written text, and a heading as
// ordinary as `# Feedback` would otherwise take id="feedback" — which precedes the
// form in the document, so getElementById('feedback') hands submitDecision an <h1>
// instead of the textarea and Submit dies on undefined.trim(), silently, for the
// rest of the review. Rendered here through the real render path.
// Rendered into a scratch container appended to the live document — the ids have to
// really be in the document for the shadowing to be possible at all — rather than by
// overwriting #doc-content, which would tear out the wired blocks later checks use.
check('a heading cannot steal an id the page itself uses', await evaluate(`
  var scratch = document.createElement('div');
  document.getElementById('doc-content').appendChild(scratch);
  scratch.innerHTML = renderMd('# Feedback\\n\\nbody\\n\\n# Submit btn\\n');
  idHeadings(scratch);
  var got = {
    // The form's own elements still answer to their ids...
    feedbackIsTextarea: document.getElementById('feedback').tagName.toLowerCase(),
    // ...and submitDecision's very first read still works rather than throwing.
    canReadFeedback: typeof document.getElementById('feedback').value === 'string',
    headingIds: Array.prototype.map.call(scratch.querySelectorAll('h1'), function(h) { return h.id; })
  };
  scratch.remove();
  return got;`), {
    feedbackIsTextarea: 'textarea', canReadFeedback: true,
    headingIds: ['spec-h-feedback', 'spec-h-submit-btn']
  });
// Non-ASCII headings must still slug to something distinct, or every heading in a
// non-English spec collapses to `section` and none of its own links resolve.
check('a non-ASCII heading still gets a usable id', await evaluate(`
  var scratch = document.createElement('div');
  document.getElementById('doc-content').appendChild(scratch);
  scratch.innerHTML = renderMd('# 設計方針\\n\\nbody\\n\\n# Café plan\\n');
  idHeadings(scratch);
  var ids = Array.prototype.map.call(scratch.querySelectorAll('h1'), function(h) { return h.id; });
  scratch.remove();
  return ids;`), ['spec-h-設計方針', 'spec-h-café-plan']);
// Having the id is not the same as being reachable. markdown-it percent-encodes
// non-ASCII in an href, so the link arrives as %E8%A8%AD… while the id holds the
// characters themselves — a lookup on the raw href misses and the jump silently
// does nothing. Only clicking a real link end to end catches that; asserting on
// idHeadings' output alone cannot, which is how it survived a round.
check('a fragment link into a non-ASCII heading actually scrolls to it', await evaluate(`
  var scratch = document.createElement('div');
  document.getElementById('doc-content').appendChild(scratch);
  scratch.innerHTML = renderMd('# 設計方針\\n\\n[jump](#設計方針)\\n');
  idHeadings(scratch);
  var h = scratch.querySelector('h1');
  var a = scratch.querySelector('a');
  var scrolled = null;
  h.scrollIntoView = function() { scrolled = this.id; };
  a.click();
  var got = { hrefWasEncoded: a.getAttribute('href') !== '#設計方針', scrolledTo: scrolled };
  scratch.remove();
  return got;`), { hrefWasEncoded: true, scrolledTo: 'spec-h-設計方針' });

console.log('\n== run detail: answering questions ==');
check('cursor lands on the first question', await cursorQuestion(), 'Which cache backend should the deploy use?');
await press('j');
check('j moves to the next question', await cursorQuestion(), 'Retry policy on a failed rollout?');
await press('k');
check('k moves back', await cursorQuestion(), 'Which cache backend should the deploy use?');
await press('2');
check('2 picks the second option of the question under the cursor', await checkedIn(0), 'memory');
await press('j');
await press('1');
check('1 picks on the second question after moving', await checkedIn(1), 'exp');
check('the first question keeps its own pick', await checkedIn(0), 'memory');
await press('9');
check('an out-of-range digit is ignored', await checkedIn(1), 'exp');
await press('i');
check('i focuses that question\'s custom-answer box', await activeId(), 'q_1_othertext');
await typeText('linear backoff');
check('typing a custom answer selects its Other radio', await checkedIn(1), '__drovr_other__');
await press('Escape');
check('Escape leaves the text box', await activeId(), 'body');
check('collectAnswers maps every question to its answer',
  await evaluate(`return collectAnswers();`),
  { cache: 'memory', retry: 'linear backoff' });

console.log('\n== run detail: answers cannot be silently dropped ==');
// answers is keyed by question id, so two questions sharing one cannot both be
// represented; refusing to submit beats losing an answer the reviewer gave.
check('duplicate question ids are refused', await evaluate(`
  var saved = currentQuestions;
  currentQuestions = [{id:'dup',prompt:'a',options:[]},{id:'dup',prompt:'b',options:[]}];
  var got = duplicateQuestionId();
  currentQuestions = saved;
  return got;`), 'dup');

console.log('\n== run detail: request-changes must say something ==');
// Driven through the real submit path, not the helper: this is the gate that
// stops a contentless request-changes reaching the agent. Safe to call — the
// guard returns before the fetch, so the run's state does not move. The decision
// radio is selected explicitly rather than trusting the page default: if it were
// ever 'approve' here, this would fall through the gate into a REAL submit and
// flip the seeded run, and every check after it would fail for the wrong reason.
check('request-changes with nothing to say is refused', await evaluate(`
  document.querySelector('input[name="decision"][value="request-changes"]').checked = true;
  document.getElementById('feedback').value = '';
  submitDecision();
  var err = document.getElementById('form-error');
  return err.style.display !== 'none' && err.textContent;`),
  'Say what to change: add feedback, or an inline comment on the spec.');
check('...and the box it tells you to fill gets the focus', await activeId(), 'feedback');
// Hand focus back: the navigator deliberately goes inert while a text box has it,
// so leaving it in the textarea would make every later keypress check a no-op.
await evaluate(`document.getElementById('feedback').blur(); return 1;`);
// An inline comment IS the feedback — the reviewer already said what to change,
// on the line they want changed. Requiring the box too was busywork.
// The quote is the fixture spec's real line 1 (web_nav.rs seeds it), hardcoded rather
// than read back from specSourceLines: a comment only counts while the line it quotes
// still reads the same (annotationIsAnchored), and comparing the page's own value to
// itself would pass even if that comparison were reduced to `x === x`.
check('a saved inline comment stands in for the feedback box', await evaluate(`
  var saved = annotations;
  annotations = { 1: { quote: '# Spec for alpha-deploy', comments: [{ id: 1, text: 'drop this', quote: '# Spec for alpha-deploy' }] } };
  var got = needsWrittenFeedback('');
  annotations = saved;
  return got;`), false);
// ...but only a SAVED one: collectAnnotations is what reaches feedback.json, so
// an entry with no comments on it must not unlock the submit.
check('an empty annotation entry does not', await evaluate(`
  var saved = annotations;
  annotations = { 12: { quote: 'a spec line', comments: [] } };
  var got = needsWrittenFeedback('');
  annotations = saved;
  return got;`), true);
check('typed feedback still works on its own',
  await evaluate(`return needsWrittenFeedback('please rework the cache section');`), false);
// Asserting on the helper alone is only half the gate: drop the annotation term at
// the call site and every check above stays green while the old behavior is back.
// So submit for REAL, with fetch stubbed — what reached the wire is the assertion.
// The stub answers {ok:false}, the one reply that re-enables the buttons without a
// refresh(), so the seeded run's state never moves. `setup` runs with annotations
// already cleared; both it and the feedback box are undone afterwards.
const stubbedSubmit = setup => evaluate(`
  return (async function() {
    var savedAnnots = annotations, savedFetch = window.fetch, sent = null;
    annotations = {};
    // Set here, not inherited from whichever check ran last: this only worked
    // because it matches the HTML's default, so a reordering would break it silently.
    document.querySelector('input[name="decision"][value="request-changes"]').checked = true;
    ${setup}
    window.fetch = function(url, opts) {
      sent = { url: String(url), body: JSON.parse(opts.body) };
      return Promise.resolve({ json: function() { return Promise.resolve({ ok: false }); } });
    };
    try { await submitDecision(); } finally {
      window.fetch = savedFetch;
      annotations = savedAnnots;
      document.getElementById('feedback').value = '';
    }
    if (!sent) return 'nothing was sent';
    return { posted: /\\/submit$/.test(sent.url), decision: sent.body.decision,
             feedback: sent.body.feedback, annotations: sent.body.annotations.length };
  })();`);

check('an annotation-only request-changes reaches the server',
  await stubbedSubmit(`annotations = { 1: { quote: '# Spec for alpha-deploy', comments: [{ id: 1, text: 'drop this', quote: '# Spec for alpha-deploy' }] } };`),
  { posted: true, decision: 'request-changes', feedback: '', annotations: 1 });
// The mirror case. Without it, a payload that hardcoded feedback:'' would pass every
// other check in this file — the reviewer's prose would vanish between box and wire.
check('...and typed feedback reaches it intact, verbatim',
  await stubbedSubmit(`document.getElementById('feedback').value = 'rework the cache section';`),
  { posted: true, decision: 'request-changes', feedback: 'rework the cache section', annotations: 0 });

// A comment can outlive the line it was written on: `review summary` re-renders the
// doc panel, and on a re-summarised SAME turn loadAnnotations restores by turn number
// alone, so old anchors come back over new text. Now that one comment can be a turn's
// entire payload, a stale one must not be what the agent receives.
check('a comment stranded by a spec revision does not count as feedback', await evaluate(`
  var saved = annotations;
  annotations = { 1: { quote: 'a line the spec no longer has', comments: [{ id: 1, text: 'drop this', quote: 'a line the spec no longer has' }] } };
  var got = [needsWrittenFeedback(''), collectAnnotations().length, strandedAnnotations()];
  annotations = saved;
  return got;`), [true, 0, 1]);
// ...and the refusal has to name the real reason: the comment is still on screen, so
// "you said nothing" would read as the submit button being broken.
check('the refusal says the spec moved under the comment', await evaluate(`
  var saved = annotations;
  annotations = { 1: { quote: 'a line the spec no longer has', comments: [{ id: 1, text: 'drop this', quote: 'a line the spec no longer has' }] } };
  document.querySelector('input[name="decision"][value="request-changes"]').checked = true;
  document.getElementById('feedback').value = '';
  submitDecision();
  annotations = saved;
  var err = document.getElementById('form-error');
  return err.style.display !== 'none' && /no longer match/.test(err.textContent);`), true);
await evaluate(`document.getElementById('feedback').blur(); return 1;`);
// The stranding test is only meaningful if an UNCHANGED line still counts — otherwise
// annotationIsAnchored could reject everything and both checks above would still pass.
check('...while a comment still matching its line does count', await evaluate(`
  var saved = annotations;
  annotations = { 1: { quote: '# Spec for alpha-deploy', comments: [{ id: 1, text: 'drop this', quote: '# Spec for alpha-deploy' }] } };
  var got = [needsWrittenFeedback(''), collectAnnotations().length, strandedAnnotations()];
  annotations = saved;
  return got;`), [false, 1, 0]);
// Dropping it from the payload is only half the job: on approve, or with feedback
// typed, the submit gate never runs, so the ONLY place the reviewer can learn their
// comment was dropped is the comment itself. It has to say so on screen, in words —
// a colour or a title tooltip alone reaches neither a keyboard nor a touch reviewer.
check('a stranded comment says so where the reviewer can see it', await evaluate(`
  var saved = annotations, savedLines = specSourceLines;
  annotations = { 1: { quote: 'a line the spec no longer has', comments: [{ id: 7, text: 'drop this', quote: 'a line the spec no longer has' }] } };
  var el = document.getElementById('saved-annots-1');
  renderSavedAnnotations(1, el);
  var chip = el.querySelector('.saved-annot-chip');
  var got = [!!chip && chip.classList.contains('stranded'),
             chip ? (chip.querySelector('.annot-stranded-tag') || {}).textContent : null];
  annotations = saved; specSourceLines = savedLines;
  renderSavedAnnotations(1, el);
  return got;`), [true, 'line changed — not sent']);
// ...and an anchored one is left alone, or the marker would just always be on.
check('...and a comment that still matches is not marked', await evaluate(`
  var saved = annotations;
  annotations = { 1: { quote: specSourceLines[0], comments: [{ id: 8, text: 'keep this', quote: specSourceLines[0] }] } };
  var el = document.getElementById('saved-annots-1');
  renderSavedAnnotations(1, el);
  var chip = el.querySelector('.saved-annot-chip');
  var got = [!!chip, !!chip && chip.classList.contains('stranded'),
             !!chip && !!chip.querySelector('.annot-stranded-tag')];
  annotations = saved;
  renderSavedAnnotations(1, el);
  return got;`), [true, false, false]);
// The inline marker can only reach a comment whose line still renders as a block.
// Delete the commented line — the most natural way for an agent to answer "cut this"
// — and wireAnnotations never visits it, so the chip is never drawn: the comment
// would vanish from the page AND from the turn, silently. Hence a panel of its own.
check('a comment whose line is gone is listed, not just dropped', await evaluate(`
  var saved = annotations;
  annotations = { 999: { quote: 'a paragraph that was deleted', comments: [{ id: 9, text: 'rework this', quote: 'a paragraph that was deleted' }] } };
  renderUnanchoredPanel();
  var panel = document.getElementById('unanchored-annots');
  var got = { shown: panel.style.display !== 'none',
              says: /will not be sent/.test(panel.textContent),
              quotesIt: /a paragraph that was deleted/.test(panel.textContent),
              keepsTheWords: /rework this/.test(panel.textContent) };
  annotations = saved;
  renderUnanchoredPanel();
  return got;`), { shown: true, says: true, quotesIt: true, keepsTheWords: true });
// ...and it stays out of the way when there is nothing to report, rather than sitting
// empty above every spec.
check('...and the panel is hidden when every comment is anchored', await evaluate(`
  var saved = annotations;
  annotations = { 1: { quote: specSourceLines[0], comments: [{ id: 10, text: 'fine', quote: specSourceLines[0] }] } };
  renderUnanchoredPanel();
  var hidden = document.getElementById('unanchored-annots').style.display === 'none';
  annotations = saved;
  renderUnanchoredPanel();
  return hidden;`), true);
// With no spec text — an absent spec.md answers the same as a failed fetch — these
// comments are unverifiable, not deleted. Saying "the line is gone" next to a
// one-click permanent discard would talk a reviewer into throwing away good work
// over a transient hiccup, so the wording changes and the discard button is withheld.
check('a panel with no spec to compare against does not claim the lines were deleted',
  await evaluate(`
  var saved = annotations, savedLines = specSourceLines;
  annotations = { 4: { quote: 'a line that is still there', comments: [{ id: 12, text: 'rework this', quote: 'a line that is still there' }] } };
  specSourceLines = [];
  renderUnanchoredPanel();
  var panel = document.getElementById('unanchored-annots');
  var got = { shown: panel.style.display !== 'none',
              claimsGone: /is gone|are gone/.test(panel.textContent),
              saysNotLoaded: /not loaded/.test(panel.textContent),
              reassures: /Nothing has been discarded/.test(panel.textContent),
              keepsTheWords: /rework this/.test(panel.textContent),
              discardButtons: panel.querySelectorAll('.annot-remove').length };
  annotations = saved; specSourceLines = savedLines;
  renderUnanchoredPanel();
  return got;`), {
    shown: true, claimsGone: false, saysNotLoaded: true, reassures: true,
    keepsTheWords: true, discardButtons: 0
  });
// The third arm of the submit gate. A doc that failed to load leaves specSourceLines
// empty, which makes every comment unverifiable — telling the reviewer their spec
// "changed" would be a guess, and telling them they said nothing is plainly false.
check('no spec text to match against is named as the reason, not blamed on the reviewer',
  await evaluate(`
  var saved = annotations, savedLines = specSourceLines;
  annotations = { 1: { quote: 'whatever was here', comments: [{ id: 11, text: 'rework this', quote: 'whatever was here' }] } };
  specSourceLines = [];
  document.querySelector('input[name="decision"][value="request-changes"]').checked = true;
  document.getElementById('feedback').value = '';
  submitDecision();
  annotations = saved; specSourceLines = savedLines;
  var err = document.getElementById('form-error');
  return err.style.display !== 'none' && err.textContent;`),
  'There is no spec text to match your inline comments against. ' +
  'Reload the page, or type feedback here.');
await evaluate(`document.getElementById('feedback').blur(); return 1;`);

// Everything above seeds `annotations` by hand, which would keep passing even if the
// UI that populates it were broken — and then a reviewer's real comment would neither
// unlock the gate nor ship. Drive the actual widget: reveal the trigger, type, Save.
// `line`/`quote` are checked against the spec source the server served, because the
// agent-facing docs (skills/pipeline/phase-prompts/brainstorm.md) promise an agent it
// can find the commented text by that line number — a wrong anchor sends it editing
// the wrong part of its own spec.
check('a comment saved through the real annotation UI is what unlocks the gate', await evaluate(`
  return (async function() {
    var specLines = (await (await fetch(api('doc'))).text()).split('\\n');
    var block = document.querySelector('#doc-content .annotatable');
    block.querySelector('.annot-trigger').click();
    var box = block.querySelector('.annotation-box');
    box.querySelector('textarea').value = 'tighten this section';
    box.querySelector('.annotation-save-btn').click();
    var saved = collectAnnotations();
    var unlocked = !needsWrittenFeedback('');
    block.querySelector('.annot-remove').click();   // undo through the real remove button
    return {
      comment: saved.length === 1 && saved[0].comment,
      lineIsBlockStart: !!saved[0] && saved[0].line === parseInt(block.getAttribute('data-source-line'), 10),
      quotesThatSpecLine: !!saved[0] && saved[0].quote === specLines[saved[0].line - 1],
      unlocked: unlocked,
      relocksOnRemove: needsWrittenFeedback('')
    };
  })();`), {
    comment: 'tighten this section', lineIsBlockStart: true, quotesThatSpecLine: true,
    unlocked: true, relocksOnRemove: true
  });

// Every staleness check above seeds `annotations` directly and exercises the READ
// side. The write side is where it broke: saving a second comment on a line used to
// overwrite one shared per-line quote, which retroactively re-anchored the stale
// comment already sitting there and shipped it as if it still applied — with the
// marker gone from its chip, so the reviewer could not even see it happen. Drive the
// real Save button on a line that already holds a stranded comment.
check('re-commenting a line does not revive the stale comment on it', await evaluate(`
  var savedAnnots = annotations;
  var block = document.querySelector('#doc-content .annotatable');
  var lineNum = parseInt(block.getAttribute('data-source-line'), 10);
  // A comment quoting text this line no longer has: stranded, as the chip shows.
  annotations = {};
  annotations[lineNum] = { quote: 'text this line has not had for a while',
                           comments: [{ id: 101, text: 'the old ask',
                                        quote: 'text this line has not had for a while' }] };
  var before = { stranded: strandedAnnotations(), sent: collectAnnotations().length };
  // Now the reviewer comments on what the line says TODAY, through the real widget.
  block.querySelector('.annot-trigger').click();
  var box = block.querySelector('.annotation-box');
  box.querySelector('textarea').value = 'the new ask';
  box.querySelector('.annotation-save-btn').click();
  var sent = collectAnnotations();
  var chips = block.querySelectorAll('.saved-annot-chip');
  var got = {
    beforeStranded: before.stranded, beforeSent: before.sent,
    // Only the new comment travels; the old one stays out of the payload.
    sentTexts: Array.prototype.map.call(sent, function(a) { return a.comment; }),
    stillStranded: strandedAnnotations(),
    // ...and its chip still says so, rather than silently looking fresh.
    strandedChips: Array.prototype.filter.call(chips, function(el) {
      return el.classList.contains('stranded'); }).length,
    chipCount: chips.length
  };
  annotations = savedAnnots;
  saveAnnotations();
  renderSavedAnnotations(lineNum, document.getElementById('saved-annots-' + lineNum));
  renderUnanchoredPanel();
  return got;`), {
    beforeStranded: 1, beforeSent: 0,
    sentTexts: ['the new ask'], stillStranded: 1, strandedChips: 1, chipCount: 2
  });
// The same trap one upgrade earlier. A comment saved before quotes moved onto
// comments has no `quote` of its own, and borrowing the line's — which the next
// save rewrites — revives it exactly as above. The reviewers carrying that data are
// the ones who were already stranded, so they must not be the ones it breaks for.
check('an old-shape comment is migrated at load, not left borrowing the line quote',
  await evaluate(`
  var savedAnnots = annotations, savedTurn = currentTurn;
  var wasOn = 'the text it was written against';
  var block = document.querySelector('#doc-content .annotatable');
  var lineNum = parseInt(block.getAttribute('data-source-line'), 10);
  // Old shape, exactly as it sits in a pre-upgrade localStorage: no per-comment quote.
  var stored = { turn: currentTurn, seq: 201, annotations: {} };
  stored.annotations[lineNum] = { quote: wasOn, comments: [{ id: 201, text: 'the old ask' }] };
  localStorage.setItem(annotKey(), JSON.stringify(stored));
  loadAnnotations(currentTurn);
  var migrated = annotations[lineNum].comments[0].quote;
  // The reviewer now comments on what the line says today, through the REAL Save
  // button — the path that used to overwrite the line's shared quote. Pushing the
  // comment by hand instead would skip that write and prove nothing.
  block.querySelector('.annot-trigger').click();
  var box = block.querySelector('.annotation-box');
  box.querySelector('textarea').value = 'the new ask';
  box.querySelector('.annotation-save-btn').click();
  var got = { migratedQuote: migrated,
              sentTexts: collectAnnotations().map(function(a) { return a.comment; }),
              stranded: strandedAnnotations() };
  annotations = savedAnnots; currentTurn = savedTurn;
  saveAnnotations();
  renderSavedAnnotations(lineNum, document.getElementById('saved-annots-' + lineNum));
  renderUnanchoredPanel();
  return got;`), {
    migratedQuote: 'the text it was written against',
    sentTexts: ['the new ask'], stranded: 1
  });

console.log('\n== pointer and keyboard agree ==');
check('clicking a question adopts it as the cursor', await evaluate(`
  var items = document.querySelectorAll('#questions-area .question-item');
  items[2].click();
  return items[2].classList.contains('nav-cursor');`), true);

console.log('\n== assistive technology ==');
check('the cursor is exposed as aria-current', await evaluate(`
  return !!document.querySelector('#questions-area .question-item.nav-cursor[aria-current="true"]');`), true);
await evaluate(`document.getElementById('nav-live').textContent = ''; return 1;`);
await press('k');
const announced = await evaluate(`return document.getElementById('nav-live').textContent || '';`);
check('moving the cursor announces the row it landed on',
  announced.indexOf(await cursorQuestion()) !== -1 && /\(\d+ of \d+\)/.test(announced), true);
check('the help is reachable by pointer, not only by its own key',
  await evaluate(`return !!document.querySelector('#keyhint button');`), true);
check('the help card is a labelled modal dialog', await evaluate(`
  var c = document.querySelector('#key-help .card');
  return !!c && c.getAttribute('role') === 'dialog' && c.getAttribute('aria-modal') === 'true'
      && !!document.getElementById(c.getAttribute('aria-labelledby'));`), true);
check('opening the help moves focus into the dialog', await evaluate(`
  openKeyHelp();
  return document.activeElement === document.querySelector('#key-help .card');`), true);
await evaluate(`closeKeyHelp(); return 1;`);
check('closing the help returns focus to whatever opened it', await evaluate(`
  return document.activeElement !== document.querySelector('#key-help .card');`), true);

console.log('\n== help overlay ==');
// Opened with the KEY, not by calling openKeyHelp() — otherwise the `?` binding
// itself, which the README and the in-app help both advertise, goes untested.
check('the help is closed to start with', await helpOpen(), false);
await press('?');
check('? opens the help', await helpOpen(), true);
// Capture the cursor BEFORE the keypress. Comparing two reads taken after it is
// a tautology that passes even if the modal stops blocking motion entirely.
const beforeHelp = await cursorQuestion();
await press('j');
check('motion is inert while the help is open', await cursorQuestion(), beforeHelp);
await press('Escape');
check('Escape closes the help', await helpOpen(), false);
await press('j');
check('motion resumes once the help is closed',
  (await cursorQuestion()) !== beforeHelp, true);

console.log('\n== layout ==');
// The regression this guards dropped padding from 60px to 34px, which still
// technically clears a ~27px bar — so require real headroom, not just clearance.
check('the page can scroll clear of the fixed hint bar', await evaluate(`
  var pad = parseInt(getComputedStyle(document.querySelector('.shell')).paddingBottom, 10);
  var bar = document.getElementById('keyhint').getBoundingClientRect().height;
  return pad >= bar * 2;`), true);

console.log('\n== leaving a run ==');
await press('h');
await waitFor(hash, h => h === '#/', 8000, 'back at the list');
check('h returns to the session list', await hash(), '#/');

console.log(`\n${pass} passed, ${fail} failed, ${skip} skipped\n`);
ws.close();
clearTimeout(watchdog);
process.exit(fail ? 1 : 0);
