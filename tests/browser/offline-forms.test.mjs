// #52: offline forms (fixture /offline-form, `<rx-form offline="visits">` posting to an
// `.offline()` route), under CSP=strict. With the network emulated away, posts are kept in
// the outbox (IndexedDB) and shown as waiting; back online they are replayed in recorded
// order, each applied once, also when the answer to one is lost on the way back. A file
// arrives intact; a 422 or 409 moves a post to "needs attention", where it can be reviewed,
// edited, resent or discarded; logout clears the outbox; a page without an offline form does
// not load the script.
//
// The fixture records what the server applied at /offline/visits.json (in memory, for the
// life of the app), so every test uses notes of its own and looks only at those.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser, sleep } from './lib/cdp.mjs';
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

// A failed fetch is logged by the browser as a console error; that is what offline looks like.
const OFFLINE_NOISE = [/ERR_INTERNET_DISCONNECTED/, /Failed to load resource/];

/** Emulates the network being gone (or back). */
async function network(page, online) {
  await page.send('Network.enable');
  await page.send('Network.emulateNetworkConditions', {
    offline: !online,
    latency: 0,
    downloadThroughput: -1,
    uploadThroughput: -1,
  });
  await page.waitFor((want) => navigator.onLine === want, { message: `navigator.onLine to be ${online}` }, online);
}

/** What the server applied whose note starts with `prefix`, in the order it applied them. */
async function applied(prefix) {
  const all = await (await fetch(`${app.url}/offline/visits.json`)).json();
  return all.filter((visit) => visit.note.startsWith(prefix));
}

/** The number of posts waiting in the device's outbox (any status). */
const stored = (page) =>
  page.eval(
    () =>
      new Promise((resolve, reject) => {
        const open = indexedDB.open('renox-offline', 1);
        open.onerror = () => reject(open.error);
        open.onupgradeneeded = () => {
          open.result.createObjectStore('items', { keyPath: 'id', autoIncrement: true });
          open.result.createObjectStore('meta');
        };
        open.onsuccess = () => {
          const count = open.result.transaction('items').objectStore('items').count();
          count.onsuccess = () => {
            open.result.close();
            resolve(count.result);
          };
        };
      }),
  );

const status = (page) => page.text('[data-rx-offline-panel="visits"] .rx-offline__status');

/** Types a note and submits the form as a person would. */
async function save(page, note) {
  await page.type('#note', note, { clear: true });
  await page.click('#save');
}

/** Waits until the status line matches. */
function statusIs(page, pattern) {
  return page.waitFor(
    (source) => new RegExp(source, 'i').test(document.querySelector('[data-rx-offline-panel="visits"] .rx-offline__status')?.textContent ?? ''),
    { message: `the status line to match ${pattern}` },
    pattern.source,
  );
}

/** Waits until the outbox holds `n` posts. */
async function outboxHolds(page, n) {
  const until = Date.now() + 15_000;
  let last;
  while (Date.now() < until) {
    last = await stored(page);
    if (last === n) return;
    await sleep(100);
  }
  assert.fail(`expected ${n} posts in the outbox, found ${last}`);
}

/** Opens the offline page and makes sure the outbox is empty. */
async function openForm(page) {
  await page.goto(`${app.url}/offline-form`);
  await page.waitFor(() => !!window.RenoxOffline, { message: 'renox-offline.js' });
  await page.eval(() => RenoxOffline.clear());
}

test('three posts made offline wait in order and are replayed in order, once each', () =>
  browser.with(async (page) => {
    await openForm(page);
    await network(page, false);
    for (const note of ['a-one', 'a-two', 'a-three']) {
      await save(page, note);
      await outboxHolds(page, ['a-one', 'a-two', 'a-three'].indexOf(note) + 1);
      assert.equal(await page.eval(() => document.querySelector('#note').value), '', 'the form is empty again');
    }
    await statusIs(page, /3 waiting to send/);
    assert.deepEqual(await applied('a-'), [], 'nothing reached the server');

    await network(page, true);
    await outboxHolds(page, 0);
    await statusIs(page, /everything was sent/);
    const visits = await applied('a-');
    assert.deepEqual(visits.map((v) => v.note), ['a-one', 'a-two', 'a-three']);
    assert.equal(new Set(visits.map((v) => v.client_id)).size, 1, 'one device');
    assert.ok(visits[0].client_id);
    const times = visits.map((v) => Date.parse(v.recorded_at));
    assert.ok(times[0] <= times[1] && times[1] <= times[2], 'recorded times follow the order');
    assert.ok(visits.every((v) => !v.skew_exceeded));
    page.assertClean({ allow: OFFLINE_NOISE });
  }));

test('a replay whose answer is lost does not apply the post twice', () =>
  browser.with(async (page) => {
    await openForm(page);
    await network(page, false);
    await save(page, 'b-one');
    await outboxHolds(page, 1);
    await save(page, 'b-two');
    await outboxHolds(page, 2);

    // The server answers the first replayed post, but the answer never arrives.
    await page.eval(() => {
      const real = window.fetch;
      let lost = true;
      window.fetch = async (...args) => {
        const res = await real(...args);
        if (lost && args[1]?.method === 'POST' && String(args[0]).includes('/offline/visits')) {
          lost = false;
          throw new TypeError('Failed to fetch');
        }
        return res;
      };
    });
    await network(page, true);
    await page.waitFor(async () => (await (await fetch('/offline/visits.json')).json()).some((v) => v.note === 'b-one'), {
      message: 'the first post to reach the server',
    });
    // The replay stopped there: both posts are still on the device.
    await outboxHolds(page, 2);

    // Trying again sends the same key: the server answers from its store.
    for (let tries = 0; tries < 40 && (await stored(page)) > 0; tries++) {
      await page.eval(() => RenoxOffline.replay());
      await sleep(150);
    }
    await outboxHolds(page, 0);
    assert.deepEqual((await applied('b-')).map((v) => v.note), ['b-one', 'b-two'], 'each applied once');
    page.assertClean({ allow: OFFLINE_NOISE });
  }));

test('a file chosen offline arrives intact', () =>
  browser.with(async (page) => {
    await openForm(page);
    await network(page, false);
    const bytes = Array.from({ length: 4000 }, (_, i) => (i * 7 + (i >> 8)) & 255);
    await page.eval((bytes) => {
      const transfer = new DataTransfer();
      transfer.items.add(new File([new Uint8Array(bytes)], 'site.bin', { type: 'application/octet-stream' }));
      document.querySelector('#photo').files = transfer.files;
    }, bytes);
    await save(page, 'c-photo');
    await outboxHolds(page, 1);
    await network(page, true);
    await outboxHolds(page, 0);
    const [visit] = await applied('c-');
    assert.equal(visit.file_name, 'site.bin');
    assert.equal(visit.file_size, bytes.length);
    assert.equal(visit.file_sum, bytes.reduce((sum, b) => sum + b, 0));
  }));

test('a 422 on replay is kept for attention with the errors on the form, and can be edited and resent', () =>
  browser.with(async (page) => {
    await openForm(page);
    await network(page, false);
    await save(page, 'd-way too long for the twenty limit');
    await outboxHolds(page, 1);
    await save(page, 'd-fine');
    await outboxHolds(page, 2);
    await network(page, true);

    // The good post went through behind the bad one; the bad one needs attention.
    await statusIs(page, /one needs attention/);
    assert.deepEqual((await applied('d-')).map((v) => v.note), ['d-fine']);
    await outboxHolds(page, 1);
    const item = '[data-rx-offline-panel="visits"] .rx-offline__item';
    const problems = await page.eval(
      (s) => [...document.querySelectorAll(`${s} .rx-offline__problem`)].map((p) => p.textContent).join(' | '),
      item,
    );
    assert.match(problems, /20|long|characters/i, problems);

    await page.click(`${item} [data-rx-offline-act="review"]`);
    assert.equal(await page.eval(() => document.querySelector('#note').value), 'd-way too long for the twenty limit');
    assert.equal(await page.eval(() => document.querySelector('#note').getAttribute('aria-invalid')), 'true');
    assert.ok((await page.text('[data-error-for="note"]')).length > 0, 'the server error is on the form');

    await save(page, 'd-edited');
    await outboxHolds(page, 0);
    assert.deepEqual((await applied('d-')).map((v) => v.note), ['d-fine', 'd-edited']);
    assert.equal(await page.eval(() => document.querySelector('[data-rx-offline-panel="visits"] .rx-offline__list').hidden), true);
    page.assertClean({ allow: OFFLINE_NOISE });
  }));

test('a 409 on replay can be discarded', () =>
  browser.with(async (page) => {
    await openForm(page);
    await network(page, false);
    await save(page, 'conflict');
    await outboxHolds(page, 1);
    await network(page, true);
    await statusIs(page, /needs attention/);
    const item = '[data-rx-offline-panel="visits"] .rx-offline__item';
    assert.match(await page.text(item), /Already recorded elsewhere/);
    await page.click(`${item} [data-rx-offline-act="discard"]`);
    await outboxHolds(page, 0);
    assert.equal(await page.eval(() => document.querySelectorAll('.rx-offline__item').length), 0);
    assert.deepEqual(await applied('conflict'), []);
    page.assertClean({ allow: OFFLINE_NOISE });
  }));

test('online, validation errors are shown on the form and nothing is queued', () =>
  browser.with(async (page) => {
    await openForm(page);
    await save(page, 'e-this note is far too long to be kept');
    await page.waitFor(() => document.querySelector('[data-error-for="note"]').textContent.trim().length > 0, {
      message: 'the error on the form',
    });
    assert.equal(await page.eval(() => document.querySelector('#note').getAttribute('aria-invalid')), 'true');
    assert.equal(await stored(page), 0);
    assert.deepEqual(await applied('e-'), []);

    await save(page, 'e-ok');
    await page.waitFor(async () => (await (await fetch('/offline/visits.json')).json()).some((v) => v.note === 'e-ok'), {
      message: 'the corrected post to arrive',
    });
    assert.equal(await stored(page), 0);
  }));

test('logging out clears the outbox', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/offline-form`);
    await page.eval(async (email) => {
      const token = document.querySelector('meta[name="csrf-token"]').content;
      const body = new URLSearchParams({ _token: token, name: 'Field', email, password: 'password123', password_confirmation: 'password123' });
      await fetch('/register', { method: 'POST', body, redirect: 'manual' });
    }, `field${Date.now()}@example.com`);
    await openForm(page);
    // Posts that fail to leave the device, with the page still online.
    await page.eval(() => {
      const real = window.fetch;
      window.fetch = (...args) =>
        String(args[0]).includes('/offline/visits') ? Promise.reject(new TypeError('Failed to fetch')) : real(...args);
    });
    await save(page, 'f-one');
    await outboxHolds(page, 1);
    await save(page, 'f-two');
    await outboxHolds(page, 2);

    await page.click('#logout button');
    await page.waitFor(() => location.pathname !== '/offline-form', { message: 'the logout to go through' });
    // The same origin, another page: the database is empty.
    await page.goto(`${app.url}/offline-form`);
    await page.waitFor(() => !!window.RenoxOffline, { message: 'renox-offline.js' });
    assert.equal(await stored(page), 0);
    assert.deepEqual(await applied('f-'), []);
  }));

test('the script is on pages with an offline form only, and runs under the strict CSP', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/form`);
    const plain = await page.eval(() => ({
      scripts: [...document.scripts].filter((s) => s.src.includes('/_renox/offline-')).length,
      api: typeof window.RenoxOffline,
    }));
    assert.deepEqual(plain, { scripts: 0, api: 'undefined' });
    page.assertClean();

    await page.goto(`${app.url}/offline-form`);
    const offline = await page.eval(() => {
      const scripts = [...document.scripts].filter((s) => s.src.includes('/_renox/offline-'));
      return { scripts: scripts.length, inline: scripts.some((s) => !s.src), api: typeof window.RenoxOffline };
    });
    assert.deepEqual(offline, { scripts: 1, inline: false, api: 'object' });
    page.assertClean();
  }));
