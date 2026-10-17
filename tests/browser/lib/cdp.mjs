// Headless Chrome over the DevTools protocol, with Node's own WebSocket: no
// npm packages. One browser per test file, one page per test.
//
//   const browser = await Browser.launch();
//   const page = await browser.page();
//   await page.goto(url);
//   await page.type('#name', 'Ana');
//   await page.click('button[type=submit]');
//   await page.waitFor(() => document.querySelector('[aria-invalid]'));
//   await browser.close();
//
// Every console error, uncaught exception and CSP violation is collected in
// `page.problems`; `page.assertClean()` fails the test when there are any
// (pass the ones a test expects to `allow`).

import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CHROME = process.env.CHROME_BIN || 'google-chrome';
const TIMEOUT = Number(process.env.BROWSER_TIMEOUT_MS || 10_000);

export class Browser {
  /**
   * `args` adds Chrome flags, e.g. the fake camera of the capture tests:
   * `['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream']`.
   */
  static async launch({ args: extra = [] } = {}) {
    const profile = mkdtempSync(join(tmpdir(), 'renox-chrome-'));
    const args = [
      '--headless=new',
      '--no-sandbox',
      '--disable-gpu',
      '--disable-dev-shm-usage',
      '--no-first-run',
      '--no-default-browser-check',
      '--window-size=1280,900',
      // A mouse that hovers, as on a desktop (headless has neither).
      '--blink-settings=primaryHoverType=2,availableHoverTypes=2,primaryPointerType=4,availablePointerTypes=4',
      '--remote-debugging-port=0',
      `--user-data-dir=${profile}`,
      ...extra,
      'about:blank',
    ];
    const proc = spawn(CHROME, args, { stdio: ['ignore', 'ignore', 'pipe'] });
    let output = '';
    const url = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`Chrome didn't start: ${output}`)), 20_000);
      proc.stderr.on('data', (chunk) => {
        output += chunk;
        const found = output.match(/DevTools listening on (ws:\/\/\S+)/);
        if (found) {
          clearTimeout(timer);
          resolve(found[1]);
        }
      });
      proc.on('exit', (code) => reject(new Error(`Chrome exited (${code}): ${output}`)));
    });
    return new Browser(proc, profile, new URL(url).port);
  }

  constructor(proc, profile, port) {
    this.proc = proc;
    this.profile = profile;
    this.port = port;
  }

  /**
   * Runs `fn` with a fresh page and always closes it (tabs left open keep
   * their connections, and Chrome allows six per host).
   */
  async with(fn, options) {
    const page = await this.page(options);
    try {
      return await fn(page);
    } catch (error) {
      // A picture of the page as it failed, for CI's artifacts.
      const dir = process.env.BROWSER_SCREENS || join(process.cwd(), 'target', 'browser-screens');
      const name = `${new Date().toISOString().replace(/[:.]/g, '-')}.png`;
      try {
        await page.screenshot(join(dir, name));
        error.message += `\n(screenshot: ${join(dir, name)})`;
      } catch {}
      throw error;
    } finally {
      await page.close();
    }
  }

  /** A fresh page (its own tab), listening for problems. */
  async page({ width = 1280, height = 900 } = {}) {
    const target = await (
      await fetch(`http://127.0.0.1:${this.port}/json/new?about:blank`, { method: 'PUT' })
    ).json();
    const page = new Page(target, this.port);
    await page.open();
    await page.send('Emulation.setDeviceMetricsOverride', {
      width,
      height,
      deviceScaleFactor: 1,
      mobile: width < 600,
    });
    // A phone-width page is a touch screen.
    if (width < 600) await page.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
    return page;
  }

  async close() {
    this.proc.kill('SIGTERM');
    await new Promise((resolve) => {
      if (this.proc.exitCode !== null) return resolve();
      this.proc.on('exit', resolve);
      setTimeout(resolve, 3000);
    });
    rmSync(this.profile, { recursive: true, force: true });
  }
}

export class Page {
  constructor(target, port) {
    this.target = target;
    this.port = port;
    this.next = 0;
    this.pending = new Map();
    this.listeners = [];
    this.problems = [];
  }

  async open() {
    this.socket = new WebSocket(this.target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      this.socket.addEventListener('open', resolve, { once: true });
      this.socket.addEventListener('error', reject, { once: true });
    });
    this.socket.addEventListener('message', (event) => {
      const message = JSON.parse(event.data);
      if (message.id && this.pending.has(message.id)) {
        const { resolve, reject } = this.pending.get(message.id);
        this.pending.delete(message.id);
        if (message.error) reject(new Error(`${message.error.message} (${message.error.data || ''})`));
        else resolve(message.result);
      } else if (message.method) {
        for (const listener of this.listeners) listener(message);
      }
    });
    this.on('Runtime.exceptionThrown', ({ exceptionDetails: d }) => {
      const where = d.url ? ` (${d.url.split('/').pop()}:${d.lineNumber + 1}:${d.columnNumber + 1})` : '';
      this.problems.push(`exception: ${d.exception?.description || d.text}${where}`);
    });
    this.on('Runtime.consoleAPICalled', ({ type, args }) => {
      if (type === 'error' || type === 'assert') {
        this.problems.push(`console.${type}: ${args.map((a) => a.value ?? a.description).join(' ')}`);
      }
    });
    this.on('Log.entryAdded', ({ entry }) => {
      if (entry.level === 'error') this.problems.push(`${entry.source}: ${entry.text}`);
    });
    // Native dialogs (alert, confirm: htmx's hx-confirm) would block the
    // page: they are accepted and recorded in `dialogs`.
    this.dialogs = [];
    this.on('Page.javascriptDialogOpening', ({ message, type }) => {
      this.dialogs.push({ type, message });
      this.send('Page.handleJavaScriptDialog', { accept: true });
    });
    await this.send('Page.enable');
    await this.send('Runtime.enable');
    // A headless tab that has just navigated can count as unfocused, and an
    // unfocused page gets no key events (#354, the family of #327): the page
    // is always focused for the browser, as with a person at the keyboard.
    await this.send('Emulation.setFocusEmulationEnabled', { enabled: true });
    await this.send('Log.enable');
    await this.send('Network.enable');
  }

  send(method, params = {}) {
    const id = ++this.next;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }

  on(method, handler) {
    const listener = (message) => message.method === method && handler(message.params);
    this.listeners.push(listener);
    return () => (this.listeners = this.listeners.filter((l) => l !== listener));
  }

  /** Resolves on the next `method` event that passes `check`. */
  once(method, check = () => true, timeout = TIMEOUT) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        off();
        reject(new Error(`no ${method} within ${timeout} ms`));
      }, timeout);
      const off = this.on(method, (params) => {
        if (!check(params)) return;
        clearTimeout(timer);
        off();
        resolve(params);
      });
    });
  }

  /** Loads `url` and waits for the load event and for htmx to be idle. */
  async goto(url) {
    const loaded = this.once('Page.loadEventFired');
    await this.send('Page.navigate', { url });
    await loaded;
    await this.settle();
  }

  /** Runs `fn` (or an expression) in the page and returns its value. */
  async eval(fn, ...args) {
    const expression =
      typeof fn === 'function' ? `(${fn})(...${JSON.stringify(args)})` : String(fn);
    const { result, exceptionDetails } = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    });
    if (exceptionDetails) {
      throw new Error(`in the page: ${exceptionDetails.exception?.description || exceptionDetails.text}`);
    }
    return result.value;
  }

  /** Waits until `fn` returns something truthy in the page. */
  async waitFor(fn, { timeout = TIMEOUT, message = String(fn) } = {}, ...args) {
    const until = Date.now() + timeout;
    let last;
    while (Date.now() < until) {
      last = await this.eval(fn, ...args);
      if (last) return last;
      await sleep(50);
    }
    throw new Error(`timed out waiting for ${message} (last: ${JSON.stringify(last)})`);
  }

  /** Waits until htmx has no request in flight and the DOM is quiet. */
  async settle() {
    await this.waitFor(
      () => !document.querySelector('.htmx-request') && document.readyState === 'complete',
      { message: 'htmx to settle' },
    );
    await sleep(80);
  }

  /**
   * The centre of the first element matching `selector`, scrolled into view;
   * waits for it to exist and be visible (pages redraw parts of themselves).
   */
  async point(selector, { timeout = TIMEOUT } = {}) {
    const until = Date.now() + timeout;
    let box;
    for (;;) {
      box = await this.eval((s) => {
        const el = document.querySelector(s);
        if (!el) return null;
        el.scrollIntoView({ block: 'center', inline: 'center' });
        const r = el.getBoundingClientRect();
        return { x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width, h: r.height };
      }, selector);
      if (box && box.w && box.h) return box;
      if (Date.now() > until) break;
      await sleep(50);
    }
    throw new Error(box ? `${selector} isn't visible` : `no element ${selector}`);
  }

  /**
   * A real mouse click in the middle of the element, once it stands still (an
   * entrance animation can move it).
   *
   * #327: in CI, now and then the mouse events of `Input.dispatchMouseEvent`
   * never reach the page, although it is visible, focused, uncovered, and
   * `Runtime.evaluate` still runs in it. So the click checks the page saw a
   * `pointerdown` or `mousedown` (on whatever is on top there: a hidden input
   * over a chip and a disabled button count, as does a click that leaves the
   * page). If not, it brings the tab to the front and tries again. After three
   * tries it clicks through the DOM (`element.click()`) and says so on stderr,
   * so the run still tests the page and the log shows how often it happens.
   */
  async click(selector) {
    for (let attempt = 1; ; attempt++) {
      const { x, y } = await this.steadyPoint(selector);
      await this.eval(() => {
        window.__cdpPressed = false;
        const seen = () => (window.__cdpPressed = true);
        for (const type of ['pointerdown', 'mousedown']) {
          window.addEventListener(type, seen, { once: true, capture: true });
        }
      });
      for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
        await this.send('Input.dispatchMouseEvent', {
          type,
          x,
          y,
          button: type === 'mouseMoved' ? 'none' : 'left',
          buttons: type === 'mousePressed' ? 1 : 0,
          clickCount: type === 'mouseMoved' ? 0 : 1,
        });
      }
      await sleep(30);
      let arrived;
      try {
        arrived = (await this.eval(() => window.__cdpPressed)) !== false;
      } catch {
        arrived = true; // the page navigated away
      }
      if (arrived) return;
      if (attempt >= 3) {
        const frames = await this.eval(
          () => new Promise((done) => {
            const timer = setTimeout(() => done('no frame in 1 s'), 1000);
            requestAnimationFrame(() => {
              clearTimeout(timer);
              done('frames drawn');
            });
          }),
        );
        process.stderr.write(`cdp: the mouse events for ${selector} never reached the page (${frames}); clicking through the DOM\n`);
        await this.eval((s) => document.querySelector(s).click(), selector);
        await sleep(30);
        return;
      }
      await this.send('Page.bringToFront');
      await sleep(100 * attempt);
    }
  }

  /**
   * The middle of `selector` once it has stopped moving (two seconds at
   * most). It scrolls the element into view once, then waits for htmx: a
   * scroll can reveal an `hx-trigger="revealed"` element, whose request would
   * otherwise race the click's (and its session cookie win over the click's).
   */
  async steadyPoint(selector) {
    await this.point(selector);
    await this.settle();
    // In the window's coordinates, which is what the mouse events take: on
    // a phone-width page the visual viewport can sit away from the layout
    // one after a scroll, and layout coordinates then land elsewhere (#305).
    const rect = () =>
      this.eval((s) => {
        const r = document.querySelector(s).getBoundingClientRect();
        const vv = window.visualViewport;
        const x = r.left + r.width / 2;
        const y = r.top + r.height / 2;
        return vv ? { x: (x - vv.offsetLeft) * vv.scale, y: (y - vv.offsetTop) * vv.scale } : { x, y };
      }, selector);
    const until = Date.now() + 2000;
    let last = await rect();
    for (;;) {
      await sleep(50);
      const now = await rect();
      if ((Math.abs(last.x - now.x) < 0.5 && Math.abs(last.y - now.y) < 0.5) || Date.now() > until) return now;
      last = now;
    }
  }

  /** A click at a point of the window (e.g. an empty corner, to click "outside"). */
  async clickAt(x, y) {
    for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
      await this.send('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1 });
    }
    await sleep(30);
  }

  async hover(selector) {
    const { x, y } = await this.point(selector);
    await this.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
  }

  async focus(selector) {
    await this.eval((s) => document.querySelector(s).focus(), selector);
  }

  /** Focuses `selector`, optionally clears it, and types `text` as a person would. */
  async type(selector, text, { clear = false } = {}) {
    await this.focus(selector);
    if (clear) {
      await this.eval((s) => {
        const el = document.querySelector(s);
        el.select?.();
      }, selector);
      await this.press('Backspace');
    }
    for (const ch of text) {
      await this.send('Input.insertText', { text: ch });
    }
  }

  /** Presses a key: `Enter`, `Escape`, `ArrowDown`, `Tab`, `k`… (with `shift` or `ctrl` held when asked). */
  async press(key, { shift = false, ctrl = false } = {}) {
    const named = {
      Enter: { code: 'Enter', keyCode: 13, text: '\r' },
      Escape: { code: 'Escape', keyCode: 27 },
      Tab: { code: 'Tab', keyCode: 9 },
      Backspace: { code: 'Backspace', keyCode: 8 },
      Delete: { code: 'Delete', keyCode: 46 },
      ArrowDown: { code: 'ArrowDown', keyCode: 40 },
      ArrowUp: { code: 'ArrowUp', keyCode: 38 },
      ArrowLeft: { code: 'ArrowLeft', keyCode: 37 },
      ArrowRight: { code: 'ArrowRight', keyCode: 39 },
      Home: { code: 'Home', keyCode: 36 },
      End: { code: 'End', keyCode: 35 },
      PageDown: { code: 'PageDown', keyCode: 34 },
      PageUp: { code: 'PageUp', keyCode: 33 },
      ' ': { code: 'Space', keyCode: 32, text: ' ' },
      ',': { code: 'Comma', keyCode: 188, text: ',' },
    }[key] || { code: `Key${key.toUpperCase()}`, keyCode: key.toUpperCase().charCodeAt(0), text: key };
    const modifiers = (shift ? 8 : 0) | (ctrl ? 2 : 0);
    const base = { key, code: named.code, windowsVirtualKeyCode: named.keyCode, modifiers };
    // #327's family for keys: now and then the page never sees the key
    // (CI, a tab that is not in front). So check a `keydown` arrived; if not,
    // bring the tab to the front and send it again (three tries, then say so).
    for (let attempt = 1; ; attempt++) {
      await this.eval((k) => {
        window.__cdpKey = false;
        window.addEventListener('keydown', () => (window.__cdpKey = true), { once: true, capture: true });
      }, key);
      await this.send('Input.dispatchKeyEvent', { type: named.text ? 'keyDown' : 'rawKeyDown', ...base, text: named.text });
      await this.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
      await sleep(30);
      let arrived = false;
      for (let wait = 0; wait < 5 && !arrived; wait++) {
        try {
          arrived = (await this.eval(() => window.__cdpKey)) !== false;
        } catch {
          arrived = true; // the page navigated away
        }
        if (!arrived) await sleep(30);
      }
      if (arrived) return;
      if (attempt >= 3) {
        const state = await this.eval(() => ({
          hasFocus: document.hasFocus(),
          visibility: document.visibilityState,
          active: document.activeElement?.outerHTML.slice(0, 80),
          readyState: document.readyState,
        })).catch((e) => String(e));
        process.stderr.write(`cdp: the ${key} key never reached the page: ${JSON.stringify(state)}\n`);
        return;
      }
      // Bring the tab to the front and make it the active target, then
      // make sure the document counts as focused before sending again.
      await fetch(`http://127.0.0.1:${this.port}/json/activate/${this.target.id}`).catch(() => {});
      await this.send('Page.bringToFront');
      await sleep(100 * attempt);
    }
  }

  /** Text of the first match (trimmed), or null. */
  text(selector) {
    return this.eval((s) => document.querySelector(s)?.textContent.trim() ?? null, selector);
  }

  /** The id or name of the focused element. */
  focused() {
    return this.eval(() => {
      const el = document.activeElement;
      return el ? el.id || el.getAttribute('name') || el.tagName.toLowerCase() : null;
    });
  }

  async screenshot(path) {
    const { data } = await this.send('Page.captureScreenshot', { format: 'png' });
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, Buffer.from(data, 'base64'));
  }

  /** Fails when the page logged errors, threw, or broke its CSP. */
  assertClean({ allow = [] } = {}) {
    const unexpected = this.problems.filter((p) => !allow.some((pattern) => pattern.test(p)));
    if (unexpected.length) {
      throw new Error(`the page had problems:\n  ${unexpected.join('\n  ')}`);
    }
  }

  /** Closes the tab, so its connections (live reload, SSE) end too. */
  async close() {
    try {
      this.socket.close();
    } catch {}
    try {
      await fetch(`http://127.0.0.1:${this.port}/json/close/${this.target.id}`);
    } catch {}
  }
}

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
