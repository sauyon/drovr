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
    var failed = document.querySelector('#run-list-items .review-empty');
    return failed ? 'wiped: ' + failed.textContent : 'rows:' + document.querySelectorAll('.run-row').length;
  });`), 'rows:6');
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
const bump = names.filter(n => n !== 'alpha-deploy').pop();
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
