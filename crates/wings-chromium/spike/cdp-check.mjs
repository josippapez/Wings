// Runs the spike and checks it over the Chrome DevTools Protocol, with no real input: Bitwarden's worker, its popup,
// a WebSocket from the worker, permissions.query, chrome.scripting, content scripts on a local login page, the Web
// Store button, focus, geometry, memory and timings. Writes evidence.json and spike.log to the results folder.
//
//   node crates/wings-chromium/spike/cdp-check.mjs [results dir] [extra spike args...]
//
// Focus is only recorded, not driven: macOS doesn't let an app started from a background process activate itself
// (activateIgnoringOtherApps and `open` both left it inactive), and without an active app there is no key window.
//
// It starts its own HTTP and WebSocket echo server on 127.0.0.1 (a random port) and stops it, the spike and every
// helper process before it exits. Run crates/wings-chromium/spike/assemble.sh first.

import { spawn, execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';

const here = path.dirname(new URL(import.meta.url).pathname);
const spike = path.join(os.homedir(), 'Library/Caches/wings-chromium-spike');
const app = path.join(spike, 'Wings Chromium Spike.app/Contents/MacOS/wings-chromium-spike');
const extension = path.join(spike, 'extensions/bitwarden');
const results = process.argv[2] ?? path.join(spike, 'results', new Date().toISOString().replace(/[:.]/g, '-'));
const extraArgs = process.argv.slice(3);
const port = 9333;
const BITWARDEN = 'nngceckbapebfimnlniiiahkandclblb';
fs.mkdirSync(results, { recursive: true });
const control = path.join(results, 'control');
fs.writeFileSync(control, '');

const evidence = { started: new Date().toISOString(), args: extraArgs };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function waitFor(what, check, timeout = 30000, every = 250) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    const value = await check();
    if (value) return value;
    await sleep(every);
  }
  throw new Error(`timed out waiting for ${what}`);
}
const note = (key, value) => {
  evidence[key] = value;
  console.log(`== ${key}: ${JSON.stringify(value).slice(0, 600)}`);
};

// --- Local test server: the login page and a WebSocket echo at /echo. ---
const loginPage = fs.readFileSync(path.join(here, 'login.html'));
const server = http.createServer((request, response) => {
  if (request.url.startsWith('/login')) {
    response.writeHead(200, { 'content-type': 'text/html' });
    response.end(loginPage);
  } else {
    response.writeHead(404);
    response.end();
  }
});
const echoed = [];
server.on('upgrade', (request, socket) => {
  const accept = crypto
    .createHash('sha1')
    .update(request.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11')
    .digest('base64');
  socket.write(
    `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`,
  );
  let buffer = Buffer.alloc(0);
  socket.on('data', (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 2) {
      const opcode = buffer[0] & 0x0f;
      let length = buffer[1] & 0x7f;
      let offset = 2;
      if (length === 126) {
        length = buffer.readUInt16BE(2);
        offset = 4;
      } else if (length === 127) {
        length = Number(buffer.readBigUInt64BE(2));
        offset = 10;
      }
      const masked = buffer[1] & 0x80;
      const mask = masked ? buffer.subarray(offset, offset + 4) : null;
      offset += masked ? 4 : 0;
      if (buffer.length < offset + length) return;
      const payload = Buffer.from(buffer.subarray(offset, offset + length));
      if (mask) for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
      buffer = buffer.subarray(offset + length);
      if (opcode === 0x8) {
        socket.end(Buffer.from([0x88, 0x00]));
        return;
      }
      if (opcode === 0x1) {
        echoed.push(payload.toString());
        const header = payload.length < 126 ? Buffer.from([0x81, payload.length]) : Buffer.from([0x81, 126, payload.length >> 8, payload.length & 0xff]);
        socket.write(Buffer.concat([header, payload]));
      }
    }
  });
  socket.on('error', () => {});
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const serverPort = server.address().port;
note('local_server', `http://127.0.0.1:${serverPort}`);

// --- The spike ---
const logPath = path.join(results, 'spike.log');
// The first page is the local login page: Chrome's Local Network Access gate stops a page elsewhere (even data:)
// from reaching 127.0.0.1 until the user allows it, and there's no prompt to allow it with.
const loginUrl = `http://127.0.0.1:${serverPort}/login`;
// --use-mock-keychain: Chromium keeps the key it encrypts cookies with in the Keychain, and every rebuild of the ad-hoc
// signed spike is a new app to the Keychain, which then asks the user before handing the key over. Until someone
// answers, the cookie store doesn't load, and every navigation and WebSocket waits.
const spikeArgs = [`--wcs-control=${control}`, `--wcs-url=${loginUrl}`, `--remote-debugging-port=${port}`, `--load-extension=${extension}`, '--use-mock-keychain', ...extraArgs];
const launchedAt = Date.now();
let exited = null;
const child = spawn(app, spikeArgs, { stdio: ['ignore', 'pipe', 'pipe'] });
const logFile = fs.createWriteStream(logPath);
for (const stream of [child.stdout, child.stderr]) stream.on('data', (data) => logFile.write(data));
child.on('exit', (code, signal) => (exited = { code, signal, at: Date.now() }));
const logLines = { filter: (f) => fs.readFileSync(logPath, 'utf8').split('\n').filter(f), slice: (n) => fs.readFileSync(logPath, 'utf8').trimEnd().split('\n').slice(n) };

const command = async (text, wait = 400) => {
  fs.writeFileSync(control, text + '\n');
  process.kill(child.pid, 'SIGUSR1');
  await sleep(wait);
};
const spikeLines = (event) => logLines.filter((line) => line.startsWith('SPIKE') && line.includes(` ${event} `));
const lastSpike = (event) => spikeLines(event).at(-1) ?? null;

// --- CDP ---
class Cdp {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.id = 0;
    this.pending = new Map();
    this.listeners = [];
    this.ws.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.id && this.pending.has(message.id)) {
        this.pending.get(message.id)(message);
        this.pending.delete(message.id);
      } else if (message.method) {
        for (const listener of this.listeners) listener(message);
      }
    };
  }
  open() {
    return new Promise((resolve, reject) => {
      this.ws.onopen = resolve;
      this.ws.onerror = reject;
    });
  }
  send(method, params = {}, sessionId, timeout = 20000) {
    return new Promise((resolve) => {
      const id = ++this.id;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve({ error: { message: `timeout after ${timeout} ms` } });
      }, timeout);
      this.pending.set(id, (message) => {
        clearTimeout(timer);
        resolve(message);
      });
      this.ws.send(JSON.stringify({ id, method, params, sessionId }));
    });
  }
  on(listener) {
    this.listeners.push(listener);
  }
  async evaluate(sessionId, expression, extra = {}) {
    const response = await this.send(
      'Runtime.evaluate',
      { expression, awaitPromise: true, returnByValue: true, ...extra },
      sessionId,
    );
    if (response.error) return { error: response.error.message };
    if (response.result.exceptionDetails) return { exception: response.result.exceptionDetails.exception?.description ?? response.result.exceptionDetails.text };
    return response.result.result.value;
  }
  async targets() {
    return (await this.send('Target.getTargets')).result.targetInfos;
  }
  async attach(targetId) {
    return (await this.send('Target.attachToTarget', { targetId, flatten: true })).result.sessionId;
  }
}

const failures = [];
async function step(name, run) {
  try {
    await run();
  } catch (error) {
    failures.push(`${name}: ${error.message}`);
    console.log(`!! ${name}: ${error.message}`);
  }
}

let cdp;
try {
  const version = await waitFor('DevTools', async () => {
    try {
      return await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
    } catch {
      return null;
    }
  });
  note('devtools_version', { Browser: version.Browser, 'Protocol-Version': version['Protocol-Version'] });
  note('devtools_listen', execFileSync('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN']).toString().trim().split('\n').slice(1));
  cdp = new Cdp(version.webSocketDebuggerUrl);
  await cdp.open();

  const sessionEvents = new Map();
  cdp.on((message) => {
    if (!message.sessionId) return;
    if (!sessionEvents.has(message.sessionId)) sessionEvents.set(message.sessionId, []);
    sessionEvents.get(message.sessionId).push(message);
  });
  const eventsOf = (sessionId, method) => (sessionEvents.get(sessionId) ?? []).filter((message) => message.method === method);

  // 1. Bitwarden's worker
  let worker;
  await step('worker', async () => {
    const target = await waitFor('the Bitwarden worker', async () =>
      (await cdp.targets()).find((t) => t.type === 'service_worker' && t.url.startsWith(`chrome-extension://${BITWARDEN}/`)),
    );
    note('worker_target', { type: target.type, url: target.url, attachedAfterMs: Date.now() - launchedAt });
    worker = await cdp.attach(target.targetId);
    await cdp.send('Runtime.enable', {}, worker);
    await cdp.send('Log.enable', {}, worker);
    await sleep(2500);
    const exceptions = eventsOf(worker, 'Runtime.exceptionThrown').map((m) => m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
    const consoleErrors = eventsOf(worker, 'Runtime.consoleAPICalled')
      .filter((m) => ['error', 'assert'].includes(m.params.type))
      .map((m) => m.params.args.map((a) => a.value ?? a.description).join(' ').slice(0, 300));
    const logErrors = eventsOf(worker, 'Log.entryAdded').filter((m) => m.params.entry.level === 'error').map((m) => m.params.entry.text.slice(0, 300));
    const consoleCount = eventsOf(worker, 'Runtime.consoleAPICalled').length;
    note('worker_startup', { exceptions, consoleErrors, logErrors, consoleMessagesSeen: consoleCount });
    note('worker_api', await cdp.evaluate(worker, `({
      scripting: typeof chrome.scripting,
      scriptingMethods: Object.keys(chrome.scripting ?? {}).sort(),
      actionOpenPopup: typeof chrome.action?.openPopup,
      offscreen: typeof chrome.offscreen,
      manifestVersion: chrome.runtime.getManifest().version,
      extensionId: chrome.runtime.id,
    })`));
    note('worker_permissions_query', await cdp.evaluate(worker, `Promise.all(['some-unknown-name', 'notifications', 'clipboard-read'].map((name) =>
      navigator.permissions.query({ name }).then((s) => name + ': resolved ' + s.state, (e) => name + ': rejected ' + e.name + ': ' + e.message)))`));
  });

  // 2. A WebSocket from the worker, and the worker still answering afterwards.
  await step('worker_websocket', async () => {
    const started = Date.now();
    const result = await cdp.evaluate(worker, `new Promise((resolve) => {
      const opened = performance.now();
      const socket = new WebSocket('ws://127.0.0.1:${serverPort}/echo');
      const timer = setTimeout(() => resolve({ result: 'timeout', readyState: socket.readyState }), 8000);
      socket.onopen = () => socket.send('ping from the Bitwarden worker');
      socket.onmessage = (event) => { clearTimeout(timer); resolve({ result: 'echo', data: event.data, ms: Math.round(performance.now() - opened) }); socket.close(); };
      socket.onerror = () => { clearTimeout(timer); resolve({ result: 'error', readyState: socket.readyState }); };
    })`);
    const after = [];
    for (let i = 0; i < 3; i++) {
      const t = Date.now();
      const value = await cdp.evaluate(worker, '1 + 1');
      after.push({ value, ms: Date.now() - t });
      await sleep(300);
    }
    note('worker_websocket', { ...result, totalMs: Date.now() - started, serverSaw: [...echoed], workerAnswersAfter: after });
  });

  // 3. The first page's first paint, and memory with that one page open at idle.
  await step('first_paint', async () => {
    const page = (await cdp.targets()).find((t) => t.type === 'page' && t.url.startsWith(loginUrl));
    const session = await cdp.attach(page.targetId);
    const timing = await cdp.evaluate(session, `({ timeOrigin: performance.timeOrigin, paint: performance.getEntriesByType('paint').map((e) => ({ name: e.name, startTime: e.startTime })) })`);
    const launch = Number(/epoch_ms=(\d+)/.exec(lastSpike('launch') ?? '')?.[1]);
    const firstPaint = timing.paint?.find((e) => e.name === 'first-paint');
    note('first_paint', {
      launchEpochMs: launch,
      firstPaintEpochMs: firstPaint ? Math.round(timing.timeOrigin + firstPaint.startTime) : null,
      launchToFirstPaintMs: firstPaint ? Math.round(timing.timeOrigin + firstPaint.startTime - launch) : null,
      spikeLines: [lastSpike('cef_loaded'), lastSpike('context_initialized'), lastSpike('browser_created'), lastSpike('load_end')],
    });
    await cdp.send('Target.detachFromTarget', { sessionId: session });
  });

  await step('idle_memory_and_pump', async () => {
    await sleep(4000);
    await command('pump', 300);
    const before = lastSpike('pump');
    await sleep(10000);
    await command('pump', 300);
    const after = lastSpike('pump');
    const work = (line) => Number(/work=(\d+)/.exec(line)?.[1]);
    note('idle_pump', { before, after, workPerSecond: (work(after) - work(before)) / 10.3 });
    note('idle_memory', memory());
  });

  // 4. Focus: what AppKit and the page report, and what giving the browser the keyboard and handing it back does in an
  // app that isn't active.
  await step('focus', async () => {
    const page = await waitFor('the first page', async () => (await cdp.targets()).find((t) => t.type === 'page' && t.url.startsWith(loginUrl)));
    const session = await cdp.attach(page.targetId);
    const hasFocus = () => cdp.evaluate(session, 'document.hasFocus()');
    const state = async (label, text) => {
      await command(text, 800);
      return { [label]: { spike: lastSpike(`state_after_${text.replace('-', '_')}`) ?? lastSpike('state'), pageHasFocus: await hasFocus() } };
    };
    const steps = { ...(await state('atStart', 'state')) };
    Object.assign(steps, await state('browserFocused', 'focus-browser'), await state('hostFocused', 'focus-host'));
    note('focus', { ...steps, windowEvents: spikeLines('window_event').slice(-16), cefActivation: spikeLines('cef_window_activation').slice(-8) });
    await cdp.send('Target.detachFromTarget', { sessionId: session });
  });

  // 5. Geometry: the browser follows the host when it moves and resizes.
  await step('geometry', async () => {
    await command('geometry', 300);
    const start = lastSpike('geometry');
    await command('resize 1200 820', 500);
    const resized = lastSpike('geometry');
    await command('move 60 -40', 500);
    const moved = lastSpike('geometry');
    await command('resize 1000 700', 500);
    await command('order', 300);
    const order = lastSpike('window_order');
    await command('hide-browser', 300);
    await command('order', 300);
    const hidden = lastSpike('window_order');
    await command('show-browser', 300);
    await command('order', 300);
    const shown = lastSpike('window_order');
    note('geometry', { start, resized, moved, order, hidden, shown, attached: spikeLines('child_attached') });
  });

  // 6. The popup: opens under the button, reaches the login screen, talks to the worker; closed and reopened.
  const popupTarget = () => cdp.targets().then((targets) => targets.find((t) => t.type === 'page' && t.url.startsWith(`chrome-extension://${BITWARDEN}/popup/`)));
  const checkPopup = async (label) => {
    const target = await waitFor(`${label} popup target`, popupTarget, 20000);
    const session = await cdp.attach(target.targetId);
    await cdp.send('Runtime.enable', {}, session);
    const screens = [];
    const text = await waitFor(`${label} login screen`, async () => {
      const value = await cdp.evaluate(session, `document.body ? document.body.innerText : ''`);
      if (typeof value !== 'string') return null;
      // Chrome-only onboarding before the login screen: asks to take over Chrome's password manager. Skip it.
      if (/default password manager/i.test(value) && !screens.includes('default-password-manager')) {
        screens.push('default-password-manager');
        await cdp.evaluate(session, `[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Skip')?.click()`);
        return null;
      }
      return /log in|email address/i.test(value) ? value : null;
    }, 45000, 500);
    // The logged-out screen's "Log in" leads to the email field; nothing is typed or submitted.
    await cdp.evaluate(session, `[...document.querySelectorAll('button, a')].find((b) => b.innerText.trim() === 'Log in')?.click()`);
    const loginForm = await waitFor(`${label} email field`, async () => {
      const value = await cdp.evaluate(session, `document.querySelector('input[type=email]') ? location.hash + ' | ' + document.body.innerText.slice(0, 200) : ''`);
      return value || null;
    }, 15000, 500).catch((error) => error.message);
    const details = await cdp.evaluate(session, `(async () => ({
      emailField: !!document.querySelector('input[type=email], input[formcontrolname=email]'),
      buttons: [...document.querySelectorAll('button')].map((b) => b.innerText.trim()).filter(Boolean).slice(0, 8),
      ping: await chrome.runtime.sendMessage({ command: 'wings-spike-ping' }).then((r) => 'resolved ' + JSON.stringify(r ?? null), (e) => 'rejected ' + e.message),
      permissionsUnknown: await navigator.permissions.query({ name: 'some-unknown-name' }).then((s) => 'resolved ' + s.state, (e) => 'rejected ' + e.name + ': ' + e.message),
      tabsCurrentWindow: (await chrome.tabs.query({ active: true, currentWindow: true })).map((t) => t.url),
      tabsLastFocused: (await chrome.tabs.query({ active: true, lastFocusedWindow: true })).map((t) => t.url),
      windows: (await chrome.windows.getAll()).map((w) => ({ id: w.id, type: w.type, focused: w.focused })),
    }))()`);
    await sleep(1500);
    const exceptions = eventsOf(session, 'Runtime.exceptionThrown').map((m) => m.params.exceptionDetails.exception?.description?.slice(0, 200) ?? m.params.exceptionDetails.text);
    note(`popup_${label}`, { url: target.url, screensSkipped: screens, textStart: text.slice(0, 300), loginForm, ...details, exceptions, geometry: lastSpike('child_attached') });
    await cdp.send('Target.detachFromTarget', { sessionId: session });
  };
  await step('popup_first', async () => {
    await command('popup', 500);
    await checkPopup('first');
  });
  await step('popup_reopened', async () => {
    await command('popup', 500);
    await waitFor('popup to close', async () => !(await popupTarget()), 10000);
    await sleep(1000);
    await command('popup', 500);
    await checkPopup('reopened');
    await command('popup', 500);
  });

  // 7. Content scripts on a local login page.
  await step('content_scripts', async () => {
    const page = (await cdp.targets()).find((t) => t.type === 'page' && t.url.startsWith(loginUrl));
    const session = await cdp.attach(page.targetId);
    await cdp.send('Runtime.enable', {}, session);
    await cdp.send('Page.enable', {}, session);
    await cdp.send('Page.reload', {}, session);
    await sleep(4000);
    const contexts = eventsOf(session, 'Runtime.executionContextCreated').map((m) => m.params.context);
    const isolated = contexts.filter((c) => c.auxData?.type === 'isolated');
    const bitwarden = isolated.find((c) => c.origin?.includes(BITWARDEN) || /bitwarden/i.test(c.name));
    await cdp.evaluate(session, `document.querySelector('#username').focus()`);
    await sleep(2500);
    const dom = await cdp.evaluate(session, `(async () => ({
      customElements: [...document.querySelectorAll('*')].map((e) => e.tagName.toLowerCase()).filter((t) => t.includes('-')),
      bitwardenAttributes: [...document.querySelectorAll('*')].flatMap((e) => [...e.attributes].map((a) => a.name)).filter((n) => /bw|bitwarden/i.test(n)),
      iframes: [...document.querySelectorAll('iframe')].map((f) => f.src),
      permissionsUnknown: await navigator.permissions.query({ name: 'some-unknown-name' }).then((s) => 'resolved ' + s.state, (e) => 'rejected ' + e.name + ': ' + e.message),
    }))()`);
    const inIsolated = bitwarden
      ? await cdp.evaluate(session, `({ runtimeId: chrome.runtime?.id, globals: Object.keys(window).filter((k) => /bitwarden|autofill|bw/i.test(k)).slice(0, 10) })`, { contextId: bitwarden.id })
      : null;
    const pageWebSocket = await cdp.evaluate(session, `new Promise((resolve) => {
      const socket = new WebSocket('ws://127.0.0.1:${serverPort}/echo');
      const timer = setTimeout(() => resolve({ result: 'timeout', readyState: socket.readyState }), 8000);
      socket.onopen = () => socket.send('ping from the login page');
      socket.onmessage = (event) => { clearTimeout(timer); resolve({ result: 'echo', data: event.data }); socket.close(); };
      socket.onerror = () => { clearTimeout(timer); resolve({ result: 'error' }); };
    })`);
    const frames = (await cdp.send('Page.getFrameTree', {}, session)).result?.frameTree;
    const childFrames = (frames?.childFrames ?? []).map((f) => f.frame.url);
    note('content_scripts', {
      isolatedWorlds: isolated.map((c) => ({ name: c.name, origin: c.origin })),
      bitwardenWorld: bitwarden ? { name: bitwarden.name, origin: bitwarden.origin } : null,
      inIsolated,
      dom,
      childFrames,
      pageWebSocket,
    });
    await cdp.send('Target.detachFromTarget', { sessionId: session });
  });

  // 8. Opening the real action popup from the worker, with no toolbar to anchor it.
  await step('action_open_popup', async () => {
    const before = (await cdp.targets()).map((t) => t.targetId);
    const result = await cdp.evaluate(worker, `chrome.action.openPopup().then(() => 'resolved', (e) => 'rejected: ' + e.message)`);
    await sleep(2000);
    const opened = (await cdp.targets()).filter((t) => !before.includes(t.targetId)).map((t) => ({ type: t.type, url: t.url }));
    note('action_open_popup', { result, newTargets: opened });
    for (const target of (await cdp.targets()).filter((t) => !before.includes(t.targetId) && t.type === 'page')) {
      await cdp.send('Target.closeTarget', { targetId: target.targetId });
    }
  });

  // 9. Loading an unpacked extension through CDP (needs --remote-debugging-pipe, so expected to be refused here).
  await step('cdp_load_unpacked', async () => {
    const response = await cdp.send('Extensions.loadUnpacked', { path: extension });
    note('cdp_load_unpacked', response.error ?? response.result);
  });

  // 10. The Chrome Web Store: does its page offer "Add to Chrome"? Nothing is clicked.
  await step('web_store', async () => {
    const page = (await cdp.targets()).find((t) => t.type === 'page' && t.url.startsWith(loginUrl));
    const session = await cdp.attach(page.targetId);
    await cdp.send('Page.enable', {}, session);
    const store = {};
    for (const [name, id] of [['bitwarden', BITWARDEN], ['ublock_origin_lite', 'ddkjiahejlhfcafbddmgiahcphecmpfh']]) {
      await cdp.send('Page.navigate', { url: `https://chromewebstore.google.com/detail/${id}` }, session);
      store[name] = await waitFor(`store page for ${name}`, async () => {
        // Google's EU cookie consent comes first: decline it (the spike's own throwaway profile).
        const consent = await cdp.evaluate(session, `location.host === 'consent.google.com' ? ([...document.querySelectorAll('button')].find((b) => /reject all/i.test(b.innerText))?.click(), 'declined') : ''`);
        if (consent) return null;
        const value = await cdp.evaluate(session, `[...document.querySelectorAll('button')].map((b) => ({ text: b.innerText.trim(), disabled: b.disabled })).filter((b) => /chrome/i.test(b.text))`);
        return Array.isArray(value) && value.length ? { url: await cdp.evaluate(session, 'location.href'), buttons: value } : null;
      }, 30000, 1000).catch((error) => ({ error: error.message }));
    }
    note('web_store', store);
    await cdp.send('Page.navigate', { url: 'about:blank' }, session);
    await cdp.send('Target.detachFromTarget', { sessionId: session });
  });

  note('browsers_seen', spikeLines('browser_created'));
} finally {
  // Quit the spike the way Wings would, then make sure nothing it started is left.
  if (!exited) {
    const quitAt = Date.now();
    await command('quit', 0).catch(() => {});
    await waitFor('the spike to exit', async () => exited, 20000, 200).catch(() => {});
    note('quit', { exitedAfterMs: exited ? exited.at - quitAt : null, exit: exited, tail: logLines.slice(-4) });
  }
  if (!exited) child.kill('SIGKILL');
  await sleep(1500);
  const left = pids();
  for (const pid of left) {
    try {
      process.kill(pid, 'SIGKILL');
    } catch {}
  }
  note('leftover_processes_killed', left);
  note('failures', failures);
  server.close();
  cdp?.ws.close();
  fs.writeFileSync(path.join(results, 'evidence.json'), JSON.stringify(evidence, null, 2));
  console.log(`results: ${results}`);
}

function pids() {
  try {
    // The executables only: a pattern like "wings-chromium-spike" also matches any command line naming the cache folder.
    return execFileSync('pgrep', ['-f', 'Contents/MacOS/(Wings Chromium Helper|wings-chromium-spike)']).toString().trim().split('\n').filter(Boolean).map(Number);
  } catch {
    return [];
  }
}

function memory() {
  const list = [child.pid, ...pids().filter((pid) => pid !== child.pid)];
  const rows = execFileSync('ps', ['-o', 'pid=,rss=,command=', '-p', list.join(',')]).toString().trim().split('\n');
  const processes = rows.map((row) => {
    const [, pid, rss, command] = /\s*(\d+)\s+(\d+)\s+(.*)/.exec(row);
    const type = /--type=(\S+)/.exec(command)?.[1] ?? 'browser (host)';
    const sub = /--utility-sub-type=(\S+)/.exec(command)?.[1];
    let footprint = null;
    try {
      footprint = /Footprint:\s+([\d.]+ \w+)/.exec(execFileSync('footprint', [pid], { stdio: ['ignore', 'pipe', 'ignore'] }).toString())?.[1] ?? null;
    } catch {}
    return { pid: Number(pid), type: sub ? `${type} ${sub}` : type, extension: command.includes('--extension-process'), rssMB: Math.round(Number(rss) / 1024), footprint };
  });
  return { processes, totalRssMB: processes.reduce((sum, p) => sum + p.rssMB, 0) };
}
