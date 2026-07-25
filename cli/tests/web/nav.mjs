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
const rowNames = () => evaluate(`
  return Array.from(document.querySelectorAll('#run-list-items .run-row .run-name')).map(function(e){return e.textContent;});`);
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

console.log('\n== session list: filter ==');
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

console.log(`\n${pass} passed, ${fail} failed, ${skip} skipped\n`);
ws.close();
clearTimeout(watchdog);
process.exit(fail ? 1 : 0);
