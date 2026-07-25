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
const docText = () => evaluate(`return (document.getElementById('doc-content').textContent || '').trim();`);
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
// epsilon-nospec is out for the same reason: it is the fixture for "no spec yet",
// and POSTing a summary would move it to `ready` under the stale-doc check below.
const bump = names.filter(n => n !== 'alpha-deploy' && n !== 'epsilon-nospec').pop();
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

console.log('\n== switching runs never leaves the previous run\'s spec on screen ==');
// A run has no spec.md until its gate is first opened with `review summary`, so
// /doc answers 200-with-an-empty-body. The doc panel was only ever WRITTEN when
// the fetched doc was non-empty, so navigating from a run that has a spec to one
// that does not left the old spec rendered under the new run's name — the
// reviewer reads a stale document and believes it belongs to this run.
// Gate on the panel being VISIBLE, not just on its text: leaving a run hides the
// panel without emptying it, and textContent reads hidden nodes — so probing the
// text alone is satisfied by the previous visit's leftovers and this goto() would
// return before the navigation had happened at all.
await goto('#/runs/alpha-deploy', {
  probe: () => evaluate(`
    var p = document.getElementById('doc-panel');
    return p.style.display !== 'none' ? (document.getElementById('doc-content').textContent || '') : '';`),
  ok: t => t.indexOf('Spec for alpha-deploy') !== -1,
  label: "alpha's spec",
});
check('a run with a spec renders it', (await docText()).indexOf('Spec for alpha-deploy') !== -1, true);

// In-page hash navigation, not a reload: a fresh page load would start with an
// empty #doc-content and pass no matter what, proving nothing.
const seqBefore = await evaluate(`return refreshSeq;`);
await evaluate(`location.hash = '#/runs/epsilon-nospec'; return 1;`);
await waitFor(hash, h => h === '#/runs/epsilon-nospec', 4000, 'nospec hash');
// refreshSeq is monotonic and bumped only once a refresh's fetches have landed,
// so "advanced past seqBefore" really does mean refresh() ran for THIS run.
// currentDocText would not: it reads empty both after a spec-less refresh and
// before anything has rendered at all, so a reload would satisfy it instantly.
await waitFor(() => evaluate(`return currentRun + '|' + (refreshSeq > ${seqBefore});`),
  v => v === 'epsilon-nospec|true', 8000, 'refresh for the spec-less run');
check('a run with no spec shows no doc at all', await docText(), '');
check('...and does not claim to be showing a spec',
  await evaluate(`return document.getElementById('doc-panel').style.display;`), 'none');

// The two checks above pass on route()'s defensive clear alone, so they do NOT
// pin the invariant where it actually has to hold: refresh() re-runs on the poll
// timer with no navigation, so IT must never leave a doc it did not fetch. Plant
// a stale render, call refresh() directly, and require it to clean up.
await evaluate(`
  document.getElementById('doc-content').innerHTML = '<p>STALE DOC FROM ANOTHER RUN</p>';
  document.getElementById('doc-panel').style.display = '';
  return 1;`);
check('the planted stale doc is really on screen', (await docText()).indexOf('STALE DOC') !== -1, true);
// Surface a rejection as a check failure rather than an uncaught exception that
// kills the driver before it prints its summary.
check('refresh() completed without throwing', await evaluate(`
  return refresh().then(function(){ return ''; }, function(e){ return String((e && e.message) || e); });`), '');
check('refresh() alone clears a doc it did not fetch', await docText(), '');
check('refresh() alone hides the panel it did not fill',
  await evaluate(`return document.getElementById('doc-panel').style.display;`), 'none');

console.log('\n== a refresh that never completes still leaves no stale doc ==');
// route()'s own defensive clear is invisible on the happy path, because route()
// awaits refresh() before any probe can sample the DOM — so the checks above pass
// even with that clear reverted. What it actually defends is refresh() FAILING:
// the reviewer navigates, the fetches reject, and nothing downstream ever runs.
// Force that by rejecting every fetch, and require the old spec to be gone anyway.
await goto('#/runs/alpha-deploy', {
  probe: () => evaluate(`
    var p = document.getElementById('doc-panel');
    return p.style.display !== 'none' ? (document.getElementById('doc-content').textContent || '') : '';`),
  ok: t => t.indexOf('Spec for alpha-deploy') !== -1,
  label: "alpha's spec (again)",
});
// Plant a line comment on ALPHA first, so there is something real to leak: with
// no annotations on the outgoing run this check would pass no matter what.
await evaluate(`
  annotations = { 1: { quote: 'Spec for alpha-deploy', comments: [{ id: 1, text: 'alpha-only comment' }] } };
  return JSON.stringify(collectAnnotations()).indexOf('alpha-only') !== -1;`);
check('the planted annotation is really submittable on alpha',
  await evaluate(`return collectAnnotations().length;`), 1);
await evaluate(`
  window.__origFetch = window.fetch;
  window.fetch = function() { return Promise.reject(new Error('forced network failure')); };
  return 1;`);
await evaluate(`location.hash = '#/runs/epsilon-nospec'; return 1;`);
await waitFor(() => evaluate(`return currentRun;`), r => r === 'epsilon-nospec', 4000,
  'route() to reach the spec-less run');
check('a failed refresh leaves no stale doc behind', await docText(), '');
check('...and no panel claiming to show one',
  await evaluate(`return document.getElementById('doc-panel').style.display;`), 'none');
// The same window is what let the previous run's line comments stay submittable:
// loadAnnotations() runs after refresh()'s awaits, so a rejected fetch skips it
// entirely and alpha's comment would still be in the payload POSTed for epsilon.
check('...and no stale annotations to submit under this run',
  await evaluate(`return JSON.stringify(collectAnnotations());`), '[]');
await evaluate(`window.fetch = window.__origFetch; return 1;`);

console.log(`\n${pass} passed, ${fail} failed, ${skip} skipped\n`);
ws.close();
clearTimeout(watchdog);
process.exit(fail ? 1 : 0);
