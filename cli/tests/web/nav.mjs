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
  a: { key: 'a', code: 'KeyA', vk: 65, text: 'a' },
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
const RUN_ROW_SEL = "#run-list-items > .run-row-wrap > .run-row, " +
                    "#run-list-items details[open] .run-row-wrap > .run-row";
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
// A real document reload, unlike `goto`, which only changes the hash. Needed
// before a test that measures render ORDERING: earlier sections press `a` and
// click Archive without awaiting those actions' internal chains, so promises from
// them can still be in flight and touch the cursor mid-measurement — which is
// exactly how the race test below passed against a deliberately broken build.
async function reload(ready) {
  await send('Page.reload', { ignoreCache: false });
  await waitFor(ready.probe, ready.ok, 8000, ready.label);
}
const LIST_READY = { probe: rowNames, ok: r => r.length > 0, label: 'session list' };
const QUESTIONS_READY = { probe: cursorQuestion, ok: q => !!q, label: 'questions panel' };
const agentNodes = () => evaluate(`
  return Array.from(document.querySelectorAll('#agents-tree .agent-node')).map(function(e){
    return { name: e.querySelector('.agent-name').textContent,
             reaped: e.classList.contains('reaped'),
             rehydrate: !!e.querySelector('.agent-rehydrate') };
  });`);
const AGENTS_READY = { probe: agentNodes, ok: n => n.length > 0, label: 'agent tree' };

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

console.log('\n== session list: the cursor when a row leaves the view ==');
await goto('#/', LIST_READY);
// Two opposite failure modes, both reproduced in earlier rounds. What separates
// them is WHY the row left, not how long it has been gone:
//   * merely HIDDEN (collapsed into Completed, folded by a liveness flap) — the
//     run still exists, so the selection must survive and come back.
//   * actually GONE from the server's list (archived then purged, deleted) — the
//     key names nothing, so it must be released at once or the numeric index
//     repaints the cursor onto a different row after every re-sort.

// --- hidden: collapse the Completed group with the cursor inside it ---
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === true, 4000, 'group open');
await press('G');                       // last visible row = inside the group
const insideGroup = await cursorName();
check('cursor can be parked on a completed row',
  ['epsilon-done', 'zeta-archived'].indexOf(insideGroup) !== -1, true);
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === false, 4000, 'group collapsed');
check('collapsing the group does not steal the selection',
  await evaluate(`return navCursorKey;`), insideGroup);
// Survive several real render passes — the previous timeout-based rule released
// the anchor after a few of these and permanently reassigned it.
for (let i = 0; i < 5; i++) await evaluate(`return renderRunList(routeGen);`);
check('...and it still is not stolen after repeated polls',
  await evaluate(`return navCursorKey;`), insideGroup);
// Holding the KEY is only half of it. While the anchored row is off screen the
// cursor has no on-screen position, so nothing may be painted as selected and no
// key may resolve to a row. Keeping the numeric index instead silently repainted
// the cursor onto whatever row slid into that slot — and `a` then archived THAT
// run: a destructive action on a session the reviewer never selected.
check('...and no visible row is marked selected while the anchor is hidden',
  await evaluate(`return document.querySelectorAll('.nav-cursor').length;`), 0);
check('...and no row resolves under the cursor while the anchor is hidden',
  await evaluate(`var r = navRows()[navCursor]; return r ? rowKey(r) : null;`), null);
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === true, 4000, 'group reopened');
check('...so reopening the group restores the cursor to it',
  await cursorName(), insideGroup);
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === false, 4000, 'group collapsed again');

// --- gone: the run leaves /api/runs entirely ---
await goto('#/', LIST_READY);
await press('g');
await press('j');
const parked = await cursorName();
// Stub /api/runs so the run really leaves the DATA, not just the DOM.
await evaluate(`
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    if (String(u).indexOf('/api/runs') !== -1) {
      return realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.filter(function(x){ return x.name !== ${JSON.stringify(parked)}; }));
        }};
      });
    }
    return realFetch(u, o);
  };
  return 1;`);
await evaluate(`return renderRunList(routeGen);`);
const released = await evaluate(`return navCursorKey;`);
check('a run gone from the server list releases the anchor immediately',
  released !== parked, true);
check('...onto a row that actually exists',
  (await rowNames()).indexOf(released) !== -1, true);
const drift = [];
for (let i = 0; i < 4; i++) {
  await evaluate(`return renderRunList(routeGen);`);
  drift.push(await evaluate(`return navCursorKey;`));
}
check('...and the cursor then stops drifting across rows',
  drift.every(k => k === released), true);
await evaluate(`window.__restoreFetch(); return 1;`);

console.log('\n== session list: a filtered-out row is still not the reviewer\'s doing ==');
// A filter narrows the list, so when the reviewer TYPES, the cursor should follow
// into what remains — and it does, because the filter input resets the anchor on
// every keystroke. But the filtered set is recomputed on every 2s poll too, and a
// run's `state` changes server-side constantly. If the selected run's new state
// stops matching the filter text, its row leaves with no user action at all.
// Treating "a filter is active" as "the reviewer did this" handed the cursor to
// whatever slid into the slot — the same wrong-run archive, reachable again.
await goto('#/', LIST_READY);
await press('/');
// Every active run's task is "task for <name>", so this matches all of them and
// the list still has rows left after one drops out — an earlier version filtered
// down to a single visible row, so removing it hit the empty-list branch and the
// scenario never ran at all.
await typeText('task');
await waitFor(rowNames, r => r.length >= 3, 4000, 'filter matched at least three runs');
// ArrowDown, not `j`: focus is still in the filter box, where `j` is just text.
// (An earlier version pressed `j`, silently made the filter "taskj", matched
// nothing, and asserted against an empty list.)
await press('ArrowDown');
const filtered = await cursorName();
check('the filter is still the one we typed', await evaluate(`return listFilter;`), 'task');
check('...and the cursor is on a row the filter matches',
  (await rowNames()).indexOf(filtered) !== -1, true);
// Exactly what the next poll returns when this run's task text changes so it no
// longer matches. Nothing else changes; the reviewer touches nothing.
await evaluate(`
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    if (String(u).indexOf('/api/runs') !== -1) {
      return realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            return x.name === ${JSON.stringify(filtered)} ? Object.assign({}, x, {task: 'chore'}) : x;
          }));
        }};
      });
    }
    return realFetch(u, o);
  };
  return 1;`);
await evaluate(`return renderRunList(routeGen);`);
check('the filtered-out row really left the list',
  (await rowNames()).indexOf(filtered), -1);
check('...and rows remain, so this is not the empty-list branch',
  (await rowNames()).length > 0, true);
check('a poll filtering out the selected row does not reassign the anchor',
  await evaluate(`return navCursorKey;`), filtered);
check('...and no visible row is marked selected',
  await evaluate(`return document.querySelectorAll('.nav-cursor').length;`), 0);
check('...so `a` has no row to act on',
  await evaluate(`var r = navRows()[navCursor]; return r ? rowKey(r) : null;`), null);
await evaluate(`window.__restoreFetch(); return 1;`);
await press('Escape');

// A new filter must land the cursor at the top of whatever matches — including
// when that keystroke's own fetch fails. The reset lives past an `await`, so a
// rejected fetch skipped it entirely, leaving the anchor on a run the new filter
// excludes; the cursor then parked and stayed invisible even after recovery.
await goto('#/', LIST_READY);
await press('g');
await evaluate(`
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    if (String(u).indexOf('/api/runs') !== -1) return Promise.reject(new Error('boom'));
    return realFetch(u, o);
  };
  return 1;`);
await press('/');
await typeText('beta');
await waitFor(() => evaluate(`return document.getElementById('run-list-items').textContent;`),
  x => x.indexOf('Failed to load sessions') !== -1, 4000, 'the failed filter render');
await evaluate(`window.__restoreFetch(); return 1;`);
await evaluate(`return renderRunList(routeGen);`);
check('a filter typed during a failed fetch still lands the cursor once it recovers',
  await evaluate(`return document.querySelectorAll('.nav-cursor').length;`), 1);
check('...on a row the filter actually matches',
  (await rowNames()).indexOf(await cursorName()) !== -1, true);
await press('Escape');
await waitFor(rowNames, r => r.length > 1, 4000, 'filter cleared');

console.log('\n== session list: the cursor survives a failed fetch ==');
await goto('#/', LIST_READY);
await press('g');
await press('j');
const held = await cursorName();
// A failed /api/runs replaces the list with "Failed to load sessions" — zero
// rows. That is a server hiccup, not a deletion, so the selection must survive
// it; clearing the anchor there discarded it before any hidden-vs-gone reasoning
// could run, and the cursor never came back on the recovery poll.
await evaluate(`
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    if (String(u).indexOf('/api/runs') !== -1) return Promise.reject(new Error('boom'));
    return realFetch(u, o);
  };
  return 1;`);
await evaluate(`return renderRunList(routeGen);`);
check('a failed fetch empties the list', await evaluate(`
  return document.querySelectorAll('.run-row').length;`), 0);
check('...but does not discard the selection',
  await evaluate(`return navCursorKey;`), held);
await evaluate(`window.__restoreFetch(); return 1;`);
await evaluate(`return renderRunList(routeGen);`);
check('...so the cursor is back on its run once the server recovers',
  await cursorName(), held);

// A slow response must never overwrite a newer one: `routeGen` only changes when
// the VIEW does, so it cannot order two fetches issued from the same view.
const baselineRuns = await evaluate(`return knownRunNames.length;`);
const afterRace = await evaluate(`
  var realFetch = window.fetch;
  var call = 0;
  window.fetch = function(u, o) {
    if (String(u).indexOf('/api/runs') !== -1) {
      call++;
      var mine = call;
      return realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        // Call 1 is the OLD snapshot (full list), delayed so it lands LAST.
        // Call 2 is newer and drops one run.
        var body = mine === 1 ? rows : rows.filter(function(x){ return x.name !== rows[0].name; });
        var wait = mine === 1 ? 220 : 0;
        return new Promise(function(res){
          setTimeout(function(){ res({ok: true, json: function(){ return Promise.resolve(body); }}); }, wait);
        });
      });
    }
    return realFetch(u, o);
  };
  var stale = renderRunList(routeGen);
  var fresh = renderRunList(routeGen);
  return Promise.all([stale, fresh]).then(function(){
    window.fetch = realFetch;
    return knownRunNames.length;
  });`);
check('a stale list response cannot overwrite a newer one',
  afterRace, baselineRuns - 1);
// Leave the page on real data for the sections below.
await evaluate(`return renderRunList(routeGen);`);

// The mirror of the case above, and the one the suite could not previously
// reach: the STALE call FAILS after the fresh one already rendered. Guarding
// only the success path let its catch wipe a correct list to "Failed to load
// sessions" — a phantom failure, easily read as the archive having failed.
// Counted rather than hard-coded: this asserts the list is UNCHANGED, so the
// number is whatever the fixture happens to hold. A hard-coded 6 broke the moment
// a seventh fixture run was added upstream.
const beforeStale = await evaluate(`return document.querySelectorAll('.run-row').length;`);
check('a stale FAILED fetch cannot wipe a newer successful render', await evaluate(`
  var realFetch = window.fetch;
  var call = 0;
  window.fetch = function(u, o) {
    if (String(u).indexOf('/api/runs') !== -1) {
      call++;
      if (call === 1) {
        // Older call: rejects, but only after the newer one has rendered.
        return new Promise(function(_res, rej){ setTimeout(function(){ rej(new Error('boom')); }, 220); });
      }
      return realFetch(u, o);
    }
    return realFetch(u, o);
  };
  var stale = renderRunList(routeGen);
  var fresh = renderRunList(routeGen);
  return Promise.all([stale, fresh]).then(function(){
    return new Promise(function(res){ setTimeout(res, 320); });
  }).then(function(){
    window.fetch = realFetch;
    // The claim is that the older call's rejection did not replace a good list
    // with "Failed to load sessions" — so ask exactly that. Reporting a literal
    // row count instead coupled this to the fixture size, and it broke the moment
    // a seventh fixture run was added upstream.
    var failed = document.querySelector('#run-list-items .review-empty');
    if (failed) return 'wiped: ' + failed.textContent;
    return document.querySelectorAll('.run-row').length > 0 ? 'intact' : 'empty';
  });`), 'intact');
await evaluate(`return renderRunList(routeGen);`);

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

console.log('\n== session list: archive button ==');
await goto('#/', LIST_READY);
// Deterministic whether or not a real herdr is reachable from this machine: with
// herdr up the fixtures report live:false, without it live:null, and only the
// latter prompts. Stub the prompt so both paths proceed.
const stubConfirm = () => evaluate(`
  window.__confirms = 0;
  window.confirm = function(){ window.__confirms++; return true; };
  window.__alerts = [];
  window.alert = function(m){ window.__alerts.push(m); };
  return 1;`);
const btnFor = name => evaluate(`
  var b = Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === ${JSON.stringify(name)}; });
  return b ? b.textContent : null;`);
const clickArchive = name => evaluate(`
  Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === ${JSON.stringify(name)}; }).click();
  return 1;`);

await stubConfirm();
// End-to-end pin: the server's `live` must actually reach the button's dataset,
// which is what gates the confirm. The gateProbe checks below call toggleArchive
// with synthetic arguments, so they prove the gating logic but NOT this wiring —
// a break here (field renamed, dataset mis-spelled) would leave every live run
// silently archivable with no prompt. Environment-independent: it asserts the
// row agrees with whatever /api/runs actually reported, live herdr or not.
check('every row\'s data-live matches what the server reported', await evaluate(`
  return fetch('/api/runs').then(function(r){return r.json();}).then(function(rows){
    var bad = [];
    rows.forEach(function(row) {
      var b = Array.from(document.querySelectorAll('.run-archive'))
        .find(function(x){ return x.dataset.run === row.name; });
      if (!b) { bad.push(row.name + ':missing-button'); return; }
      var want = row.live === null ? 'unknown' : (row.live ? '1' : '0');
      if (b.dataset.live !== want) bad.push(row.name + ':' + b.dataset.live + '!=' + want);
      if (b.dataset.archived !== (row.archived ? '1' : '0')) bad.push(row.name + ':archived-mismatch');
    });
    return bad;
  });`), []);

check('every row carries an archive control',
  await evaluate(`return document.querySelectorAll('.run-archive').length > 0;`), true);
check('an active run offers Archive', await btnFor('beta-cache'), 'Archive');

// The control is a sibling of the <a>, not a child — clicking must not navigate.
const beforeHash = await hash();
await clickArchive('beta-cache');
await waitFor(rowNames, r => r.indexOf('beta-cache') === -1, 6000, 'beta-cache leaves the list');
check('archiving does not navigate away from the list', await hash(), beforeHash);
check('the archived run leaves the active list', (await rowNames()).indexOf('beta-cache'), -1);
check('...and is still present, under Completed',
  (await allRowNames()).indexOf('beta-cache') !== -1, true);
check('it persisted server-side', await evaluate(`
  return fetch('/api/runs').then(function(r){return r.json();}).then(function(rows){
    var row = rows.find(function(x){ return x.name === 'beta-cache'; });
    return !!(row && row.archived && row.complete);
  });`), true);

await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === true, 4000, 'group open for restore');
check('an archived run offers Restore', await btnFor('beta-cache'), 'Restore');
await clickArchive('beta-cache');
await waitFor(rowNames, r => r.indexOf('beta-cache') !== -1, 6000, 'beta-cache returns');
check('restoring puts it back in the active list',
  (await rowNames()).indexOf('beta-cache') !== -1, true);
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === false, 4000, 'group collapsed again');

// The safety property the whole feature turns on: archiving a run that MAY have
// live panes must prompt first, because closing the workspace destroys the
// agent's context. Driven client-side with fetch stubbed — the fixtures record
// no workspace (so a reachable herdr reports live:false and the branch would
// never run), and stubbing keeps this from mutating server state, which made an
// earlier state-cycling version order-dependent and wrong.
const gateProbe = (archived, live) => evaluate(`
  window.__confirms = 0;
  window.__posted = 0;
  window.confirm = function(){ window.__confirms++; return true; };
  var realFetch = window.fetch;
  window.fetch = function(u, o) {
    if (String(u).indexOf('/archive') !== -1) {
      window.__posted++;
      return Promise.resolve({ok: true, json: function(){ return Promise.resolve({ok:true, workspace_closed:true}); }});
    }
    return realFetch(u, o);
  };
  return toggleArchive('probe-run', ${archived}, ${JSON.stringify(live)}).then(function(){
    window.fetch = realFetch;
    return {confirms: window.__confirms, posted: window.__posted};
  });`);

check('archiving a definitively-dead run does NOT prompt',
  await gateProbe(false, '0'), {confirms: 0, posted: 1});
check('archiving a LIVE run prompts first',
  await gateProbe(false, '1'), {confirms: 1, posted: 1});
check('archiving with UNKNOWN liveness also prompts',
  await gateProbe(false, 'unknown'), {confirms: 1, posted: 1});
check('restoring never prompts, whatever liveness says',
  await gateProbe(true, '1'), {confirms: 0, posted: 1});

// 'a' acts on the row under the nav cursor.
await goto('#/', LIST_READY);
await stubConfirm();
await press('g');
const aTarget = await cursorName();
await press('a');
await waitFor(rowNames, r => r.indexOf(aTarget) === -1, 6000, `'a' archives ${aTarget}`);
check("'a' archives the row under the cursor", (await rowNames()).indexOf(aTarget), -1);
check('...and the cursor did not strand on the now-hidden row',
  (await cursorName()) !== aTarget && (await cursorName()) !== null, true);
// Restore it so the sections below see the original fixture.
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === true, 4000, 'group open to restore');
await clickArchive(aTarget);
await waitFor(rowNames, r => r.indexOf(aTarget) !== -1, 6000, `${aTarget} restored`);
await evaluate(`document.querySelector('.run-group > summary').click(); return 1;`);
await waitFor(groupOpen, o => o === false, 4000, 'group collapsed after restore');

// A failed workspace close leaves the run a ZOMBIE: archived, but its panes are
// still alive, so the server forces `complete: false` and the row deliberately
// stays in the active list. Advancing the cursor off it — as archiving normally
// should — walks the selection onto a row that is still on screen, which is the
// wrong-row class again by a third route.
await goto('#/', LIST_READY);
await stubConfirm();
await press('g');
const zTarget = await cursorName();
await evaluate(`
  window.__zArchived = false;
  window.__zName = ${JSON.stringify(zTarget)};
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    var s = String(u);
    // The archive POST "succeeds" but reports the workspace could not be closed.
    if (s.indexOf('/archive') !== -1) {
      window.__zArchived = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: false});
      }});
    }
    // ...and the run comes back archived with live panes and complete:false —
    // exactly what the server emits for a zombie.
    if (s.indexOf('/api/runs') !== -1) {
      return realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            return x.name === window.__zName
              ? Object.assign({}, x, {live: null, archived: window.__zArchived, complete: false})
              : x;
          }));
        }};
      });
    }
    return realFetch(u, o);
  };
  return 1;`);
await evaluate(`return renderRunList(routeGen);`);
check('the zombie fixture reports unknown liveness', await evaluate(`
  var b = Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === window.__zName; });
  return b ? b.dataset.live : null;`), 'unknown');
await press('a');
await waitFor(() => evaluate(`return window.__zArchived;`), v => v === true, 4000, 'the archive POST fired');
await evaluate(`return renderRunList(routeGen);`);
// The checks below assert the cursor did NOT move, so waiting for the expected
// value would pass instantly whether or not the render landed. Wait for the
// render's observable EFFECT instead — the row showing as archived — which is
// what proves the state under test was actually reached.
await waitFor(() => evaluate(`
  var b = Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === window.__zName; });
  return b ? b.dataset.archived : null;`), v => v === '1', 6000, 'the archive to reach the row');
check('a zombie row stays in the active list', (await rowNames()).indexOf(zTarget) !== -1, true);
check('...so the cursor stays on it rather than walking to a neighbour',
  await cursorName(), zTarget);
await evaluate(`window.__restoreFetch(); return 1;`);

// The cursor decision must come from what ACTUALLY happened, not from the
// client's cached `live` at click time — those disagree in both directions.

// (1) Cached live '0' (so no confirm), but the run comes back a real zombie.
// Predicting from the cached value advanced the cursor off a surviving row, and
// silently: the "panes may still be running" alert did not fire either.
await goto('#/', LIST_READY);
await stubConfirm();
await press('g');
const staleTarget = await cursorName();
await evaluate(`
  window.__zArchived = false;
  window.__zName = ${JSON.stringify(staleTarget)};
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    var s = String(u);
    if (s.indexOf('/archive') !== -1) {
      window.__zArchived = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: false});
      }});
    }
    if (s.indexOf('/api/runs') !== -1) {
      return realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            if (x.name !== window.__zName) return x;
            // Before the click the page is told "definitely not live", so the
            // key does not prompt. Afterwards the truth: panes alive.
            return window.__zArchived
              ? Object.assign({}, x, {live: true, archived: true, complete: false})
              : Object.assign({}, x, {live: false, archived: false, complete: false});
          }));
        }};
      });
    }
    return realFetch(u, o);
  };
  return 1;`);
await evaluate(`return renderRunList(routeGen);`);
check('the stale-liveness fixture reports not-live before the click', await evaluate(`
  var b = Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === window.__zName; });
  return b ? b.dataset.live : null;`), '0');
await press('a');
await waitFor(() => evaluate(`return window.__zArchived;`), v => v === true, 4000, 'the archive POST fired');
await evaluate(`return renderRunList(routeGen);`);
// Same reason as above: assert-a-negative needs the render's effect waited on.
await waitFor(() => evaluate(`
  var b = Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === window.__zName; });
  return b ? b.dataset.archived : null;`), v => v === '1', 6000, 'the archive to reach the row');
check('a run that turns out to be a zombie stays in the active list',
  (await rowNames()).indexOf(staleTarget) !== -1, true);
check('...so the cursor stays on it even though cached liveness said otherwise',
  await cursorName(), staleTarget);
await evaluate(`window.__restoreFetch(); return 1;`);

// (2) The converse. `workspace_closed:false` is overwhelmingly "the workspace was
// already gone", so the row really does leave — the cursor must move on rather
// than park on a row that is never coming back.
await goto('#/', LIST_READY);
await stubConfirm();
await press('g');
const goneTarget = await cursorName();
await evaluate(`
  window.__gArchived = false;
  window.__gName = ${JSON.stringify(goneTarget)};
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    var s = String(u);
    if (s.indexOf('/archive') !== -1) {
      window.__gArchived = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: false});
      }});
    }
    if (s.indexOf('/api/runs') !== -1) {
      return realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            if (x.name !== window.__gName) return x;
            return window.__gArchived
              ? Object.assign({}, x, {live: false, archived: true, complete: true})
              : Object.assign({}, x, {live: null, archived: false, complete: false});
          }));
        }};
      });
    }
    return realFetch(u, o);
  };
  return 1;`);
await evaluate(`return renderRunList(routeGen);`);
await press('a');
await waitFor(() => evaluate(`return window.__gArchived;`), v => v === true, 4000, 'the archive POST fired');
// Wait for the row to go rather than asserting straight after dispatching a
// render: `press` does not await toggleArchive, so its own render can be
// dispatched after this one and win the staleness guard, leaving this one to bail
// without painting. Asserting immediately made this check flake.
await waitFor(rowNames, r => r.indexOf(goneTarget) === -1, 6000,
  'the archived row to leave the active list');
check('a run whose workspace was already gone really leaves the active list',
  (await rowNames()).indexOf(goneTarget), -1);
// `press` does not await toggleArchive — its own re-render, and the cursor
// decision that follows it, land afterwards. Wait for that rather than probing
// mid-flight; if the advance never happens this still fails, on the timeout.
await waitFor(cursorName, n => n !== null && n !== goneTarget, 6000,
  'the cursor to advance off the archived row');
check('...so the cursor advances to a neighbour instead of stranding',
  (await cursorName()) !== goneTarget, true);
await evaluate(`window.__restoreFetch(); return 1;`);

// (3) The render that decides must be the one that can answer. toggleArchive
// used to decide right after awaiting its OWN render — but that render bails
// without painting whenever a newer one has been dispatched (the 2s poll), while
// its promise still resolves. The check then ran against pre-archive DOM, saw the
// row still present, skipped the hand-off, and the later render parked the cursor
// forever. Reproduced here by controlling resolution order explicitly.
// A clean document: no leftover chains from earlier sections may touch the
// cursor while this test is measuring who answers the hand-off.
await reload(LIST_READY);
await stubConfirm();
await press('g');
const raceTarget = await cursorName();
await evaluate(`
  window.__rArchived = false;
  window.__rName = ${JSON.stringify(raceTarget)};
  window.__held = [];
  window.__hold = true;
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; window.__hold = false; };
  window.fetch = function(u, o) {
    var s = String(u);
    if (s.indexOf('/archive') !== -1) {
      window.__rArchived = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: true});
      }});
    }
    if (s.indexOf('/api/runs') !== -1) {
      var p = realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            return x.name === window.__rName && window.__rArchived
              ? Object.assign({}, x, {live: false, archived: true, complete: true})
              : x;
          }));
        }};
      });
      // Park every list fetch until the test releases it, in order.
      if (window.__hold) return new Promise(function(res){ window.__held.push(function(){ res(p); }); });
      return p;
    }
    return realFetch(u, o);
  };
  return 1;`);
await press('a');
// toggleArchive's own render is now in flight and held.
await waitFor(() => evaluate(`return window.__held.length;`), n => n >= 1, 4000,
  'toggleArchive to dispatch its own render');
// A second render — the 2s poll, in real use — is dispatched AFTER it, so it wins
// the staleness guard and toggleArchive's render will bail without painting.
await evaluate(`renderRunList(routeGen); return 1;`);
await waitFor(() => evaluate(`return window.__held.length;`), n => n >= 2, 4000,
  'the newer render to be dispatched');
// Resolve the older one first: it bails, DOM still shows the pre-archive list.
await evaluate(`window.__held[0](); return 1;`);
// Then the newer one paints the truth.
await evaluate(`window.__held[1](); return 1;`);
await waitFor(cursorName, n => n !== null && n !== raceTarget, 6000,
  'the cursor to advance despite the losing render');
check('a render losing the staleness race cannot strand the cursor',
  (await cursorName()) !== raceTarget, true);
check('...and the cursor is on a real, visible row',
  (await rowNames()).indexOf(await cursorName()) !== -1, true);
await evaluate(`window.__restoreFetch(); return 1;`);

// (4) Two archives before either repaints. `pendingAdvance` was a single slot, so
// the second overwrote the first and nothing ever answered for row A — the cursor
// stayed naming it, and since archived runs remain in knownRunNames, applyNavCursor
// read that as merely hidden and parked forever. Two ordinary clicks, no
// adversarial timing: batch-archiving finished sessions does it.
await reload(LIST_READY);
await stubConfirm();
await press('g');
const firstRow = await cursorName();
const secondRow = (await rowNames()).filter(n => n !== firstRow)[0];
await evaluate(`
  window.__done = {};
  window.__held = [];
  window.__hold = true;
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; window.__hold = false; };
  window.fetch = function(u, o) {
    var s = String(u);
    var m = s.match(/\\/api\\/runs\\/([^/]+)\\/archive/);
    if (m) {
      window.__done[decodeURIComponent(m[1])] = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: true});
      }});
    }
    if (s.indexOf('/api/runs') !== -1) {
      var p = realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            return window.__done[x.name]
              ? Object.assign({}, x, {live: false, archived: true, complete: true})
              : x;
          }));
        }};
      });
      if (window.__hold) return new Promise(function(res){ window.__held.push(function(){ res(p); }); });
      return p;
    }
    return realFetch(u, o);
  };
  return 1;`);
// Archive the row under the cursor, then a different row, before either repaints.
await press('a');
await waitFor(() => evaluate(`return window.__held.length;`), n => n >= 1, 4000,
  'the first archive to dispatch its render');
await evaluate(`
  Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === ${JSON.stringify(secondRow)}; }).click();
  return 1;`);
await waitFor(() => evaluate(`return window.__held.length;`), n => n >= 2, 4000,
  'the second archive to dispatch its render');
await evaluate(`window.__held.forEach(function(f){ f(); }); return 1;`);
await waitFor(rowNames, r => r.indexOf(firstRow) === -1, 6000, 'the first row to leave');
// Both halves matter, and the first is deliberately not just `!== firstRow`: a
// parked cursor reads as null, which satisfies that on its own.
check('the first archive is still answered after a second one is started',
  (await cursorName()) !== null && (await cursorName()) !== firstRow, true);
check('...and the cursor is on a visible row, not parked',
  (await cursorName()) !== null && (await rowNames()).indexOf(await cursorName()) !== -1, true);
await evaluate(`window.__restoreFetch(); return 1;`);

// (5) An archive slower than one poll tick. `route()` bumps routeGen every 2s,
// so gating the hand-off on `gen === routeGen` meant any archive whose herdr
// round-trip outlasted a tick was dropped un-applied and the cursor stranded.
// No double click, no adversarial timing — just a slow workspace close.
await reload(LIST_READY);
await stubConfirm();
await press('g');
const slowRow = await cursorName();
await evaluate(`
  window.__sDone = false;
  window.__held = [];
  window.__hold = true;
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; window.__hold = false; };
  window.fetch = function(u, o) {
    var s = String(u);
    if (s.indexOf('/archive') !== -1) {
      window.__sDone = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: true});
      }});
    }
    if (s.indexOf('/api/runs') !== -1) {
      var p = realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            return x.name === ${JSON.stringify(slowRow)} && window.__sDone
              ? Object.assign({}, x, {live: false, archived: true, complete: true})
              : x;
          }));
        }};
      });
      if (window.__hold) return new Promise(function(res){ window.__held.push(function(){ res(p); }); });
      return p;
    }
    return realFetch(u, o);
  };
  return 1;`);
await press('a');
await waitFor(() => evaluate(`return window.__held.length;`), n => n >= 1, 4000,
  'the archive to dispatch its render');
// Let the real 2s poll tick at least once while the archive is still in flight,
// which is what used to invalidate the hand-off's generation.
const genBefore = await evaluate(`return routeGen;`);
await waitFor(() => evaluate(`return routeGen;`), g => g > genBefore, 8000,
  'the poll to advance routeGen');
await evaluate(`window.__held.forEach(function(f){ f(); }); return 1;`);
await waitFor(cursorName, n => n !== null && n !== slowRow, 8000,
  'the cursor to advance after a slow archive');
check('an archive slower than a poll tick still hands the cursor on',
  (await cursorName()) !== null && (await cursorName()) !== slowRow, true);
await evaluate(`window.__restoreFetch(); return 1;`);

// (6) The LAST two rows archived before either repaints. `neighbourKey` falls
// back to the previous row when there is no next one, so those two name each
// other — following a captured neighbour blindly put the cursor on the other
// archived row. The neighbour has to be re-checked against what is on screen.
await reload(LIST_READY);
await stubConfirm();
const allRows = await rowNames();
const lastRow = allRows[allRows.length - 1];
const secondLast = allRows[allRows.length - 2];
await evaluate(`
  window.__done2 = {};
  window.__held = [];
  window.__hold = true;
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; window.__hold = false; };
  window.fetch = function(u, o) {
    var s = String(u);
    var m = s.match(/\\/api\\/runs\\/([^/]+)\\/archive/);
    if (m) {
      window.__done2[decodeURIComponent(m[1])] = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: true});
      }});
    }
    if (s.indexOf('/api/runs') !== -1) {
      var p = realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            return window.__done2[x.name]
              ? Object.assign({}, x, {live: false, archived: true, complete: true})
              : x;
          }));
        }};
      });
      if (window.__hold) return new Promise(function(res){ window.__held.push(function(){ res(p); }); });
      return p;
    }
    return realFetch(u, o);
  };
  return 1;`);
await evaluate(`
  navCursorKey = ${JSON.stringify(secondLast)};
  applyNavCursor(false);
  Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === ${JSON.stringify(secondLast)}; }).click();
  return 1;`);
await waitFor(() => evaluate(`return window.__held.length;`), n => n >= 1, 4000,
  'the first of the two archives');
await evaluate(`
  Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === ${JSON.stringify(lastRow)}; }).click();
  return 1;`);
await waitFor(() => evaluate(`return window.__held.length;`), n => n >= 2, 4000,
  'the second of the two archives');
await evaluate(`window.__held.forEach(function(f){ f(); }); return 1;`);
await waitFor(rowNames, r => r.indexOf(secondLast) === -1 && r.indexOf(lastRow) === -1,
  8000, 'both rows to leave');
const survivor = await cursorName();
check('archiving the last two rows leaves the cursor on a surviving row',
  survivor !== null && survivor !== secondLast && survivor !== lastRow, true);
check('...and that row is actually visible',
  (await rowNames()).indexOf(survivor) !== -1, true);
await evaluate(`window.__restoreFetch(); return 1;`);

// (7) A filter hides rows without their going anywhere. Answering "did it leave"
// from whether a row is on screen therefore handed the cursor away from a run
// that still exists — here a zombie, which stays active on purpose — just because
// the filter stopped matching it. The answer has to come from the run data.
//
// The filter is applied BEFORE the archive: typing one deliberately resets the
// anchor, so a filter typed afterwards could never observe the hand-off.
await reload(LIST_READY);
await stubConfirm();
await evaluate(`
  window.__kDone = false;
  window.__kName = '';
  var realFetch = window.fetch;
  window.__restoreFetch = function(){ window.fetch = realFetch; };
  window.fetch = function(u, o) {
    var s = String(u);
    if (s.indexOf('/archive') !== -1) {
      window.__kDone = true;
      return Promise.resolve({ok: true, json: function(){
        return Promise.resolve({workspace_closed: false});
      }});
    }
    if (s.indexOf('/api/runs') !== -1) {
      return realFetch(u, o).then(function(r){ return r.json(); }).then(function(rows){
        return {ok: true, json: function(){
          return Promise.resolve(rows.map(function(x){
            if (x.name !== window.__kName || !window.__kDone) return x;
            // A zombie: archived, panes alive, so still ACTIVE — and its task text
            // no longer matches the filter, so it is hidden without having left.
            return Object.assign({}, x, {live: true, archived: true, complete: false, task: 'zzz'});
          }));
        }};
      });
    }
    return realFetch(u, o);
  };
  return 1;`);
await press('/');
await typeText('task');
await waitFor(rowNames, r => r.length >= 2, 4000, 'the filter to match several runs');
const keepRow = await cursorName();
await evaluate(`window.__kName = navCursorKey; return 1;`);
check('the cursor is on a filtered row before archiving',
  (await rowNames()).indexOf(keepRow) !== -1, true);
await evaluate(`
  Array.from(document.querySelectorAll('.run-archive'))
    .find(function(x){ return x.dataset.run === window.__kName; }).click();
  return 1;`);
await waitFor(() => evaluate(`return window.__kDone;`), v => v === true, 4000, 'the archive POST');
await evaluate(`return renderRunList(routeGen);`);
// The cursor check below is an assert-a-negative, so wait for the render's
// effect — the row leaving the filtered list — before trusting it.
await waitFor(rowNames, r => r.indexOf(keepRow) === -1, 6000,
  'the zombie to drop out of the filtered list');
check('the zombie is hidden by the filter', (await rowNames()).indexOf(keepRow), -1);
check('...but it is still active, so the cursor is not handed away from it',
  await evaluate(`return navCursorKey;`), keepRow);
await press('Escape');
await evaluate(`window.__restoreFetch(); return 1;`);

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

console.log('\n== agent tree: a reaped phase ==');
await goto('#/runs/delta-idle', AGENTS_READY);
const tree = await agentNodes();
check('a reaped phase is still listed — hiding it would look like it never ran',
  tree.map(n => n.name), ['brainstorm', 'plan', 'implement']);
check('reaped phases render dimmed', tree.map(n => n.reaped), [true, true, false]);
// The ⟳ is gated on `rehydratable` — the same predicate the CLI refuses on — so
// it appears on BOTH reaped phases (the second reseeds rather than resuming, and
// its tooltip says so) and on neither the live one nor a phase that never ran.
// Gating it on "has a session" instead would hide a recovery that works, and
// gating it on `reaped` alone would offer one the CLI then rejects.
check('⟳ appears exactly where a click will work',
  tree.map(n => n.rehydrate), [true, true, false]);
check('the ⟳ says which one you get', await evaluate(`
  return Array.from(document.querySelectorAll('#agents-tree .agent-rehydrate'))
    .map(function(b){ return b.title.indexOf('resume this') !== -1 ? 'session' : 'reseed'; });`),
  ['session', 'reseed']);
// The stub answers the way the server does, so the click handler's real
// response path runs — including `r.json()`, which is where the outcome the
// human needs lives.
const stubbedClick = (ok, body) => evaluate(`
  var seen = null, real = window.fetch;
  window.fetch = function(u, o) {
    seen = { url: u, method: (o || {}).method };
    return Promise.resolve({ ok: ${ok}, status: ${ok ? 200 : 500},
                             json: function(){ return Promise.resolve(${JSON.stringify(body)}); } });
  };
  document.querySelector('#agents-tree .agent-rehydrate').click();
  window.fetch = real;
  return seen;`);
const agentsNoteText = () => evaluate(`
  var e = document.getElementById('agents-note');
  return { text: e.textContent, bad: e.classList.contains('bad'), shown: e.style.display !== 'none' };`);

check('⟳ posts to the run-scoped rehydrate endpoint',
  await stubbedClick(true, { ok: true, complete: false, phase: 'brainstorm',
    detail: "phase 'brainstorm' relaunched INCOMPLETE — its seed was NOT re-sent" }),
  { url: '/api/runs/delta-idle/rehydrate?phase=brainstorm', method: 'POST' });
// A rehydrate that could not deliver the seed is an HTTP 200. If the page drops
// the body, the user sees the ⟳ vanish and never learns the agent is blank.
await waitFor(agentsNoteText, n => n.text.indexOf('NOT re-sent') !== -1, 8000,
  'the outcome detail to be shown');
// …and it must READ as a problem. `complete: false` means the pane came back
// but the agent was never told what it is doing — not a success to scroll past.
check('an incomplete rehydrate is flagged, not reported as plain success',
  (await agentsNoteText()).bad, true);

// The control: a rehydrate that DID everything must not be flagged, or "flag
// everything" would pass the check above.
const enabledButton = () => evaluate(`
  var b = document.querySelector('#agents-tree .agent-rehydrate');
  return !!b && !b.disabled;`);
await waitFor(enabledButton, v => v === true, 8000, 'the tree to re-render');
await stubbedClick(true, { ok: true, complete: true, phase: 'brainstorm',
  detail: "phase 'brainstorm' resumed with its recorded session" });
await waitFor(agentsNoteText, n => n.text.indexOf('resumed with') !== -1, 8000,
  'the success detail');
check('a complete rehydrate is NOT flagged', (await agentsNoteText()).bad, false);

// Wait for a button that is ENABLED, not merely present: the click above left
// the old element disabled on purpose, and clicking a disabled button is a
// silent no-op — the next check would then assert against the previous note.
await waitFor(enabledButton, v => v === true, 8000, 'the tree to re-render');
check('a failed rehydrate reports the reason the server gave, not just a status code',
  await (async () => {
    await stubbedClick(false, { ok: false, error: 'run has no herdr workspace' });
    await waitFor(agentsNoteText, n => n.bad, 8000, 'the failure note');
    return await agentsNoteText();
  })(),
  { text: 'run has no herdr workspace', bad: true, shown: true });
check('a failed rehydrate re-enables its button', await evaluate(`
  var b = document.querySelector('#agents-tree .agent-rehydrate');
  return b ? b.disabled : null;`), false);


// A pane that leaves the tree must not stay selected: `?pane=<gone>` answers
// 204 on the mirror and 409 on send, which reads as a wedged UI rather than as
// a stale selection — and a reaped node is exactly how a pane leaves.
check('a live pane can be selected', await evaluate(`
  selectPane('w1:p3', 'implement'); return selectedPane;`), 'w1:p3');
check('a pane outside the tree starts out selected too', await evaluate(`
  selectPane('w1:gone', 'ghost'); return selectedPane;`), 'w1:gone');
await waitFor(() => evaluate(`return selectedPane;`), v => v === null, 8000,
  'the stale pane to be dropped');
check('the mirror falls back to the run default once it is dropped',
  await evaluate(`return document.getElementById('session-target').textContent;`), 'active');

// A reseed waits for the fresh agent to attach — up to 30s — so the response can
// easily land after the user has moved on. `#agents-note` is page-global, so an
// unguarded write files run A's outcome under run B. Hold the response open,
// navigate, THEN settle it: without the guard the note lands on the wrong run.
// `location.hash` (not Page.navigate) so the in-flight promise survives.
check('a rehydrate response can be held open across a navigation', await evaluate(`
  window.__settle = null;
  var real = window.fetch;
  window.fetch = function() { return new Promise(function(res){ window.__settle = res; }); };
  document.querySelector('#agents-tree .agent-rehydrate').click();
  window.fetch = real;
  location.hash = '#/runs/beta-cache';
  return typeof window.__settle === 'function';`), true);
await waitFor(hash, h => h === '#/runs/beta-cache', 8000, 'the other run');
await evaluate(`
  window.__settle({ ok: true, status: 200,
                    json: function(){ return Promise.resolve({ detail: 'LEAKED FROM delta-idle' }); } });
  return 1;`);
await sleep(400);
check('a response arriving after navigation is dropped, not filed under the new run',
  await evaluate(`return document.getElementById('agents-note').textContent;`), '');

console.log('\n== leaving a run ==');
await goto('#/runs/alpha-deploy', QUESTIONS_READY);
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
  // Quote taken from the live spec so the comment is anchored: a comment counts
  // only while the line it quotes still reads the same, and this check is about
  // cross-run leakage, not anchoring — an unanchored plant would leave it with
  // nothing to leak.
  annotations = { 1: { quote: specSourceLines[0],
                       comments: [{ id: 1, text: 'alpha-only comment', quote: specSourceLines[0] }] } };
  // A SECOND plant, deliberately unanchored, so #unanchored-annots is populated
  // too. The anchored one above never reaches that panel, so on its own it leaves
  // the panel's own leak untested — and that panel renders the comment's text
  // verbatim, which is the reviewer's private prose about another run's spec.
  // Line 999: a line the spec does not have, so no block renders for it and the
  // comment lands in the panel rather than as an inline chip on a real block.
  annotations[999] = { quote: 'a line alpha no longer has',
                       comments: [{ id: 2, text: 'alpha-only stranded note',
                                    quote: 'a line alpha no longer has' }] };
  renderUnanchoredPanel();
  return JSON.stringify(collectAnnotations()).indexOf('alpha-only') !== -1;`);
check('the planted annotation is really submittable on alpha',
  await evaluate(`return collectAnnotations().length;`), 1);
check('...and the stranded one is really on screen to leak', await evaluate(`
  var p = document.getElementById('unanchored-annots');
  return p.style.display !== 'none' && /alpha-only stranded note/.test(p.textContent);`), true);
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
// The unanchored panel is a sibling of #doc-panel, so main's reset of that panel
// does not reach it, and it only ever repaints from refresh() — which just
// rejected. Left out of the synchronous reset it keeps displaying the previous
// run's comment text, under this run's name, with nothing to correct it.
check('...and no previous run\'s comments still displayed', await evaluate(`
  var p = document.getElementById('unanchored-annots');
  return { hidden: p.style.display === 'none', leaks: /alpha-only/.test(p.textContent) };`),
  { hidden: true, leaks: false });
await evaluate(`window.fetch = window.__origFetch; return 1;`);

console.log('\n== the decision form does not carry across runs ==');
// The decision radio and the feedback box are plain form fields, so nothing reset
// them on navigation: prose typed for one run stayed in the box and the radio kept
// its pick, and submitting on the next run wrote them into THAT run's
// feedback.json — a decision the reviewer never made about a spec they never read.
await goto('#/runs/alpha-deploy', QUESTIONS_READY);
await evaluate(`
  document.getElementById('feedback').value = 'alpha-only feedback, must not follow me';
  document.querySelector('input[name="decision"][value="approve"]').checked = true;
  return 1;`);
check('the planted decision is really staged on alpha', await evaluate(`
  var r = document.querySelector('input[name="decision"]:checked');
  return r.value + '|' + (document.getElementById('feedback').value.length > 0);`), 'approve|true');
await evaluate(`location.hash = '#/runs/epsilon-nospec'; return 1;`);
await waitFor(() => evaluate(`return currentRun;`), r => r === 'epsilon-nospec', 4000,
  'route() to reach the spec-less run');
check('the feedback box is empty on the next run',
  await evaluate(`return document.getElementById('feedback').value;`), '');
check('the decision falls back to request-changes, not the previous pick',
  await evaluate(`
    var r = document.querySelector('input[name="decision"]:checked');
    return r ? r.value : null;`), 'request-changes');

console.log('\n== staying on the same run keeps the reviewer\'s work ==');
// The cross-run resets above must fire on a RUN CHANGE, not on every route().
// `#/runs/<run>?task=<t>` is a supported URL (reviewTask(), and the router
// comment documents it), so browser back/forward — or opening a task link while
// already on that run — re-enters route() with the SAME run. Feedback is never
// persisted anywhere, so clearing it there destroys the reviewer's typed prose
// with no warning and no way back.
await goto('#/runs/alpha-deploy', QUESTIONS_READY);
await evaluate(`
  document.getElementById('feedback').value = 'half-written feedback I am still editing';
  document.querySelector('input[name="decision"][value="approve"]').checked = true;
  // Anchored for the same reason: this pins annotation PERSISTENCE, so the plant
  // has to be one that would actually survive to be submitted. Line 3, not line 2:
  // line 2 of the fixture spec is blank, and '' === '' anchors against any spec
  // with a blank line there — a pin that holds without the real text matching.
  annotations = { 3: { quote: specSourceLines[2],
                       comments: [{ id: 9, text: 'in-progress note', quote: specSourceLines[2] }] } };
  saveAnnotations();   // every real mutation site does this, so match real usage
  return 1;`);
const sameRunGen = await evaluate(`return routeGen;`);
await evaluate(`location.hash = '#/runs/alpha-deploy?task=task-1'; return 1;`);
await waitFor(hash, h => h === '#/runs/alpha-deploy?task=task-1', 4000, 'same-run task hash');
// Wait on routeGen, NOT refreshSeq: refreshSeq is also bumped by the background
// pollState->refresh() loop that runs for a `ready` run, so it can advance before
// route() has touched the hashchange at all — which made these checks pass whether
// or not the reset block ran. routeGen is incremented only by route(), on entry,
// in the same synchronous task as the reset block below it.
await waitFor(() => evaluate(`return routeGen;`), g => g > sameRunGen, 8000,
  'route() to process the same-run navigation');
check('typed feedback survives a same-run navigation',
  await evaluate(`return document.getElementById('feedback').value;`),
  'half-written feedback I am still editing');
check('the decision pick survives too', await evaluate(`
  var r = document.querySelector('input[name="decision"]:checked');
  return r ? r.value : null;`), 'approve');
// Note this one holds via loadAnnotations() restoring from localStorage, not via
// the run-change gate — it still passes with the gate removed. Kept because the
// user-visible invariant is worth pinning (it would catch persistence breaking),
// but it is not what proves the gate works: the two checks above are.
check('and in-progress annotations survive',
  await evaluate(`return collectAnnotations().length;`), 1);

console.log('\n== a stale review panel cannot repaint over the session list ==');
// refreshReview() is called fire-and-forget and awaits twice, so it outlives a
// navigation. Without a routeGen guard its late resolution re-showed the panel —
// on the session list, which had already hidden it once on the way out.
await goto('#/runs/alpha-deploy?task=task-1', {
  probe: () => evaluate(`return currentRun;`), ok: r => r === 'alpha-deploy', label: 'alpha detail',
});
// Hold the findings fetch open, navigate away, then release it: the resolution
// lands with the reviewer already back on the list.
await evaluate(`
  window.__release = null;
  window.__origFetch2 = window.fetch;
  window.fetch = function(u) {
    if (String(u).indexOf('review/findings') !== -1) {
      return new Promise(function(res) { window.__release = function() { res(window.__origFetch2(u)); }; });
    }
    return window.__origFetch2.apply(window, arguments);
  };
  refreshReview();
  return 1;`);
await waitFor(() => evaluate(`return !!window.__release;`), v => v === true, 4000, 'findings fetch parked');
await evaluate(`location.hash = '#/'; return 1;`);
await waitFor(rowNames, r => r.length > 0, 8000, 'back on the session list');
await evaluate(`window.__release(); return 1;`);
// Give the released promise a turn to resolve and (wrongly) paint.
await waitFor(() => evaluate(`return 1;`), () => true, 500, 'tick');
await sleep(300);
check('the review panel stays hidden on the session list',
  await evaluate(`return document.getElementById('review-panel').style.display;`), 'none');
check('...and the session list is still what is on screen', (await rowNames()).length > 0, true);
await evaluate(`window.fetch = window.__origFetch2; return 1;`);

console.log('\n== blocked-agent alarms ==');
// The badge is server-fed and needs a herdr with a genuinely stuck agent, which
// this harness has no way to produce. What IS testable — and is where the bugs
// live — is the page's own bookkeeping: notify once per block, restore the title
// when it clears, and never let one view's feed clear another view's alarms.
//
// The run names below are deliberately absent from the fixture, so the 2s
// session-list poll (which syncs with the real, unblocked runs) cannot clear
// them mid-section.
await goto('#/', LIST_READY);
await evaluate(`
  window.__notes = [];
  window.__origNotifyBlocked = notifyBlocked;
  notifyBlocked = function(label) { window.__notes.push(label); };
  blockedAlarms = {}; blockedNotified = {};
  applyBlockedTitle();
  return 1;`);
const sync = (alarms, scope) => evaluate(
  `syncBlockedAlarms(${JSON.stringify(alarms)}, ${JSON.stringify(scope)}); return document.title;`);
const notes = () => evaluate(`return window.__notes.slice();`);

const A = { key: 'zz-one/implement', label: 'zz-one · implement is stopped on a destructive prompt' };
check('a new block puts a count in the tab title',
  (await sync([A], ['zz-one'])).slice(0, 6), '⚠ (1) ');
check('...and fires exactly one notification', await notes(), [A.label]);
await sync([A], ['zz-one']);
check('a block that persists across polls does not re-notify', (await notes()).length, 1);

const B = { key: 'zz-two/plan', label: 'zz-two · plan is stopped on an unknown prompt' };
check('a second stuck run raises the count',
  (await sync([A, B], ['zz-one', 'zz-two'])).slice(0, 6), '⚠ (2) ');
check('...and notifies only about the new one', (await notes()).length, 2);

// The scope rule: the run detail view can only speak for the run it is showing.
check('a feed scoped to one run leaves the others alone',
  (await sync([A], ['zz-one'])).slice(0, 6), '⚠ (2) ');
check('a feed that CAN speak for a run clears it',
  (await sync([A], ['zz-one', 'zz-two'])).slice(0, 6), '⚠ (1) ');
check('the title goes back to normal when nothing is stuck',
  await sync([], ['zz-one', 'zz-two']), 'Drovr Review Loop');
check('a block that recurs after clearing notifies again',
  (await sync([A], ['zz-one']), (await notes()).length), 3);

check('a destructive prompt renders an alarming badge',
  await evaluate(`var h = blockedBadge({count:1, needs_human:1, phase:'implement', class:'destructive'});
    return [h.indexOf('needs-human') !== -1, h.indexOf('⚠ blocked') !== -1];`), [true, true]);
check('a routine prompt renders a quiet one',
  await evaluate(`var h = blockedBadge({count:1, needs_human:0, phase:'implement', class:'routine'});
    return [h.indexOf('needs-human') !== -1, h.indexOf('⚠') !== -1];`), [false, false]);
check('several blocks in one run are counted on the badge',
  await evaluate(`return blockedBadge({count:3, needs_human:1, phase:'implement', class:'unknown'})
    .indexOf('+2') !== -1;`), true);
check('nothing blocked renders nothing',
  await evaluate(`return blockedBadge(null);`), '');
check('a blocked review panel nested under a phase is still found',
  await evaluate(`return treeBlocked([{ name: 'implement-task-1', blocked: null, children: [
      { name: 'review:task-1:1:security', blocked: { needs_human: true, class: 'unknown' } },
      { name: 'review:task-1:1:perf', blocked: { needs_human: false, class: 'routine' } }] }])
    .map(function(n){ return n.name; });`), ['review:task-1:1:security']);

// Leave the page as found: later sections (and the 2s poll) share this tab.
await evaluate(`
  notifyBlocked = window.__origNotifyBlocked;
  blockedAlarms = {}; blockedNotified = {}; applyBlockedTitle();
  return 1;`);

console.log(`\n${pass} passed, ${fail} failed, ${skip} skipped\n`);
ws.close();
clearTimeout(watchdog);
process.exit(fail ? 1 : 0);
