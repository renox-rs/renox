// #33: an installable app (fixture /pwa, `App::pwa(..)`), under CSP=strict. The
// head links a valid manifest, the nonce-carrying script registers the service
// worker, a logged-in page is never stored by it, and with the server gone a
// page that was visited shows the offline page. The last test stops the server,
// so it stays last in this file.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

let browser;
let app;

before(async () => {
  app = await fixture({ env: { CSP: 'strict' } });
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

/**
 * Waits until the worker is active and controls the page. It polls the state
 * instead of awaiting `serviceWorker.ready` inside one evaluation: `ready`
 * resolves when the worker is activating, before `clients.claim()` has run,
 * and an evaluation parked on it cannot be cut short by the deadline. A page
 * that finished loading while the worker was still installing is controlled
 * only by the claim; if the worker is active and the page still is not, one
 * reload (a navigation under an active worker is always controlled) settles it.
 */
const controlled = async (page) => {
  const until = Date.now() + 10_000;
  let reloaded = false;
  let last;
  while (Date.now() < until) {
    last = await page.eval(async () => {
      const reg = await navigator.serviceWorker.getRegistration('/');
      return {
        controlled: !!navigator.serviceWorker.controller,
        installing: reg?.installing?.state,
        waiting: reg?.waiting?.state,
        active: reg?.active?.state,
      };
    });
    if (last.controlled) return;
    if (last.active === 'activated' && !reloaded) {
      reloaded = true;
      await page.goto(await page.eval(() => location.href));
    }
    await new Promise((done) => setTimeout(done, 50));
  }
  throw new Error(`timed out waiting for the service worker to control the page (last: ${JSON.stringify(last)})`);
};

/** Every URL in every cache the page can see, as paths. */
const cachedPaths = (page) =>
  page.eval(async () => {
    const paths = [];
    for (const name of await caches.keys()) {
      for (const request of await (await caches.open(name)).keys()) paths.push(new URL(request.url).pathname);
    }
    return paths;
  });

test('the head links a valid manifest, and the registration script carries the nonce', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/pwa`);
    const head = await page.eval(() => ({
      manifest: document.querySelector('link[rel=manifest]')?.getAttribute('href'),
      theme: document.querySelector('meta[name=theme-color]')?.content,
      touch: document.querySelector('link[rel=apple-touch-icon]')?.getAttribute('href'),
      nonced: [...document.querySelectorAll('script[nonce]')].some((s) => s.textContent.includes('serviceWorker')),
    }));
    assert.equal(head.manifest, '/manifest.webmanifest');
    assert.match(head.theme, /^#[0-9a-f]{6}$/i);
    assert.ok(head.touch.endsWith('icon-192.png'), head.touch);
    assert.ok(head.nonced, 'the registration script has a nonce');

    const response = await fetch(`${app.url}${head.manifest}`);
    assert.match(response.headers.get('content-type'), /application\/manifest\+json/);
    const manifest = await response.json();
    assert.equal(manifest.name, 'Browser fixture');
    assert.equal(manifest.display, 'standalone');
    assert.ok(manifest.start_url && manifest.theme_color && manifest.background_color);
    const sizes = manifest.icons.map((i) => i.sizes);
    assert.ok(sizes.includes('192x192') && sizes.includes('512x512'), JSON.stringify(manifest.icons));
    assert.ok(manifest.icons.some((i) => i.purpose === 'maskable'), 'a maskable icon');
    for (const icon of manifest.icons) {
      const res = await fetch(new URL(icon.src, app.url));
      assert.equal(res.status, 200, icon.src);
      assert.match(res.headers.get('content-type'), /image\/png/);
    }
    page.assertClean();
  }));

test('the service worker registers and installs under CSP=strict', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/pwa`);
    await controlled(page);
    const worker = await page.eval(async () => {
      const reg = await navigator.serviceWorker.getRegistration('/');
      return { scope: new URL(reg.scope).pathname, script: new URL(reg.active.scriptURL).pathname, state: reg.active.state };
    });
    assert.deepEqual(worker, { scope: '/', script: '/sw.js', state: 'activated' });
    assert.ok((await cachedPaths(page)).includes('/_renox/offline'), 'the offline page is precached');
    page.assertClean();
  }));

test('a logged-in page is not stored by the worker', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/pwa`);
    await controlled(page);
    await page.eval(async (email) => {
      const token = document.querySelector('meta[name="csrf-token"]').content;
      const body = new URLSearchParams({ _token: token, name: 'Pwa', email, password: 'password123', password_confirmation: 'password123' });
      await fetch('/register', { method: 'POST', body, redirect: 'manual' });
    }, `pwa${Date.now()}@example.com`);
    await page.goto(`${app.url}/pwa/account`);
    assert.equal(await page.text('#heading'), 'Private account page');
    const paths = await cachedPaths(page);
    assert.ok(!paths.some((p) => p.startsWith('/pwa')), `no page in the cache: ${paths}`);
  }));

test('a visited page shows the offline page when the server is gone, and a logged-in one does too', async () => {
  const page = await browser.page();
  try {
    await page.goto(`${app.url}/pwa`);
    await controlled(page);
    await app.stop();
    for (const path of ['/pwa', '/pwa/account']) {
      await page.goto(`${app.url}${path}`);
      assert.equal(await page.text('#offline'), 'You are offline', path);
      assert.equal(await page.eval(() => document.querySelector('#heading')), null, `${path} is not served from a cache`);
    }
  } finally {
    await page.close();
  }
});
