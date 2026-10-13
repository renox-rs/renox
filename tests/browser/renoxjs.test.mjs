// #264: renox.js in a real browser: the CSRF header on htmx requests (and
// a request refused without it, or with a token a login replaced), a 422
// placed next to its fields with focus on the first in page order and the
// typed input kept, errors without a slot, plain forms refilled after a
// redirect, live reload and analytics events.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
import { ROOT, fixture } from './lib/app.mjs';

let browser;
let app;

before(async () => {
  app = await fixture();
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

test('a 422 puts each error next to its field and focuses the first one in page order', () => browser.with(async (page) => {
  await page.goto(`${app.url}/form`);
  // `email` is wrong and `name` too long; alphabetical order would focus
  // email first, page order focuses name.
  await page.type('#rx-name', 'A name far too long');
  await page.type('#rx-email', 'not-an-email');
  await page.type('input[name=tags]', 'toolong');
  await page.click('#signup button[type=submit]');
  await page.waitFor(() => document.querySelectorAll('[aria-invalid="true"]').length >= 2, {
    message: 'fields marked invalid',
  });
  await page.settle();
  const state = await page.eval(() => ({
    nameInvalid: document.querySelector('#rx-name').getAttribute('aria-invalid'),
    emailInvalid: document.querySelector('#rx-email').getAttribute('aria-invalid'),
    nameError: document.querySelector('#rx-name-error')?.textContent.trim(),
    tagSlot: document.querySelector('#tag-slot').textContent.trim(),
    name: document.querySelector('#rx-name').value,
    email: document.querySelector('#rx-email').value,
    focused: document.activeElement?.id,
  }));
  assert.equal(state.nameInvalid, 'true');
  assert.equal(state.emailInvalid, 'true');
  assert.match(state.nameError, /may not be greater than 10|must not be greater than 10|10 characters/);
  // A `data-error-for` slot gets the error of a list item (`tags.0`).
  assert.match(state.tagSlot, /tags|5/i);
  // Nothing typed is lost: the form isn't reset or swapped.
  assert.equal(state.name, 'A name far too long');
  assert.equal(state.email, 'not-an-email');
  assert.equal(state.focused, 'rx-name');
  page.assertClean({ allow: [/422/] });

  // Fixed and sent again: the errors go and the toast says so.
  await page.type('#rx-name', 'Ana', { clear: true });
  await page.type('#rx-email', 'ana@example.com', { clear: true });
  await page.type('input[name=tags]', 'new', { clear: true });
  await page.click('#signup button[type=submit]');
  await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('Welcome, Ana.'), {
    message: 'the success toast',
  });
  const cleared = await page.eval(() => document.querySelectorAll('[aria-invalid="true"]').length);
  assert.equal(cleared, 0, 'aria-invalid is cleared after a successful submit');
}));

test('htmx requests carry the CSRF token (a POST without it would be refused)', () => browser.with(async (page) => {
  await page.goto(`${app.url}/form`);
  const sent = page.once('Network.requestWillBeSent', (p) => p.request.method === 'POST');
  await page.type('#rx-name', 'Bo');
  await page.type('#rx-email', 'bo@example.com');
  await page.click('#signup button[type=submit]');
  const { request } = await sent;
  const headers = Object.fromEntries(Object.entries(request.headers).map(([k, v]) => [k.toLowerCase(), v]));
  assert.ok(headers['x-csrf-token'], `the CSRF header: ${JSON.stringify(headers)}`);
  await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('Welcome, Bo.'));
  page.assertClean();
}));

test('a plain form comes back with its errors and old input, never the password', () => browser.with(async (page) => {
  await page.goto(`${app.url}/plain`);
  await page.type('#rx-name', 'Ana');
  await page.type('#rx-email', 'nope');
  await page.type('#rx-password', 'hunter2');
  const loaded = page.once('Page.loadEventFired');
  await page.click('#plain button[type=submit]');
  await loaded;
  await page.settle();
  const state = await page.eval(() => ({
    name: document.querySelector('#rx-name').value,
    email: document.querySelector('#rx-email').value,
    password: document.querySelector('#rx-password').value,
    summary: document.querySelector('[data-rx-error-summary]')?.textContent || '',
    invalid: document.querySelector('#rx-email').getAttribute('aria-invalid'),
  }));
  assert.equal(state.name, 'Ana');
  assert.equal(state.email, 'nope');
  assert.equal(state.password, '', 'passwords are never refilled');
  assert.match(state.summary, /email/i);
  assert.equal(state.invalid, 'true');
  page.assertClean();
}));

test('errors without a slot get a paragraph after their input; a list item finds its own', () => browser.with(async (page) => {
  await page.goto(`${app.url}/errors`);
  await page.type('#note', 'long');
  await page.type('#tag-1', 'ok');
  await page.type('#tag-2', 'toolong');
  await page.click('#notes button[type=submit]');
  await page.waitFor(() => document.querySelector('#note').getAttribute('aria-invalid') === 'true', {
    message: 'the note marked invalid',
  });
  const state = await page.eval(() => {
    const after = (id) => {
      const next = document.querySelector(id).nextElementSibling;
      return next && next.matches('p.error') ? [next.getAttribute('data-renox-error'), next.textContent] : null;
    };
    return {
      note: after('#note'),
      tag1: document.querySelector('#tag-1').getAttribute('aria-invalid'),
      tag2: after('#tag-2'),
      tag2Invalid: document.querySelector('#tag-2').getAttribute('aria-invalid'),
      focused: document.activeElement.id,
    };
  });
  assert.equal(state.note[0], 'note');
  assert.match(state.note[1], /3/);
  // `tags.1` is the second of the inputs named `tags`.
  assert.equal(state.tag1, null);
  assert.equal(state.tag2Invalid, 'true');
  assert.equal(state.tag2[0], 'tags.1');
  assert.equal(state.focused, 'note');
  // Fixed and sent again: the inserted paragraphs go.
  await page.type('#note', 'ok', { clear: true });
  await page.type('#tag-2', 'ok', { clear: true });
  await page.click('#notes button[type=submit]');
  await page.waitFor(() => document.querySelector('#notes-result').textContent.includes('ok saved'));
  assert.equal(await page.eval(() => document.querySelectorAll('[data-renox-error]').length), 0);
  page.assertClean({ allow: [/422/] });
}));

test('a POST without the token, or with one a login replaced, is refused', () => browser.with(async (page) => {
  await page.goto(`${app.url}/form`);
  const bare = await page.eval(async () => (await fetch('/signup', {
    method: 'POST',
    headers: { 'HX-Request': 'true', 'Content-Type': 'application/x-www-form-urlencoded' },
    body: 'name=Al&email=al%40example.com',
  })).status);
  assert.equal(bare, 419, 'no token');
  // Signing up (here, from the page's own script, as another tab would)
  // logs in and gives the session a new token; this page still has the old one.
  await page.eval(async (email) => {
    const token = document.querySelector('meta[name="csrf-token"]').content;
    const body = new URLSearchParams({ _token: token, name: 'Rota', email, password: 'password123', password_confirmation: 'password123' });
    await fetch('/register', { method: 'POST', body, redirect: 'manual' });
  }, `rota${Date.now()}@example.com`);
  const answered = page.once('Network.responseReceived', (p) => p.response.url.endsWith('/signup'));
  await page.type('#rx-name', 'Bo');
  await page.type('#rx-email', 'bo@example.com');
  await page.click('#signup button[type=submit]');
  assert.equal((await answered).response.status, 419, 'the page from before the login');
  page.assertClean({ allow: [/419/] });
}));

test('analytics events reach gtag from an htmx answer, a page, and after a redirect', () => browser.with(async (page) => {
  await page.send('Page.addScriptToEvaluateOnNewDocument', {
    source: 'window.__events = []; window.gtag = function () { window.__events.push([...arguments]); };',
  });
  const seen = (name) => page.waitFor((n) => window.__events.find((e) => e[1] === n), { message: name }, name);
  await page.goto(`${app.url}/errors`);
  await page.click('#track');
  const signup = await seen('signup');
  assert.deepEqual(signup, ['event', 'signup', { plan: 'pro' }]);
  await page.goto(`${app.url}/tracked`);
  assert.deepEqual(await seen('page_seen'), ['event', 'page_seen', { page: 'tracked' }]);
  // An event queued before a redirect shows on the page after it.
  await page.goto(`${app.url}/track-then-go`);
  assert.equal(await page.eval(() => location.pathname), '/');
  await seen('went');
  page.assertClean();
}));

test('live reload: the stream opens, a template change reloads, and Back reopens it', async () => {
  // A copy of the views: this test edits one.
  const views = mkdtempSync(join(tmpdir(), 'renox-views-'));
  cpSync(join(ROOT, 'tests/browser/fixture/views'), views, { recursive: true });
  const live = await fixture({ env: { VIEWS_PATH: views } });
  try {
    await browser.with(async (page) => {
      const streams = [];
      page.on('Network.requestWillBeSent', (p) => p.request.url.includes('/_renox/live') && streams.push(p.request.url));
      await page.send('Page.addScriptToEvaluateOnNewDocument', {
        source: "addEventListener('pageshow', (e) => { window.__persisted = e.persisted; });",
      });
      // Each close of the live stream is counted in sessionStorage, which
      // outlives the page.
      await page.send('Page.addScriptToEvaluateOnNewDocument', {
        source: `const close = EventSource.prototype.close;
          EventSource.prototype.close = function () {
            if (String(this.url).includes('/_renox/live')) {
              sessionStorage.setItem('liveClosed', String(Number(sessionStorage.getItem('liveClosed') || 0) + 1));
            }
            return close.call(this);
          };`,
      });
      await page.goto(`${live.url}/`);
      const until = Date.now() + 5000;
      while (!streams.length && Date.now() < until) await sleep(50);
      assert.ok(streams.length >= 1, 'the page opened /_renox/live');

      const reloaded = page.once('Page.loadEventFired', () => true, 20_000);
      writeFileSync(join(views, 'home.html'), '{% extends "layout.html" %}{% block content %}<h1>Changed</h1>{% endblock %}');
      await reloaded;
      await page.waitFor(() => document.querySelector('h1')?.textContent === 'Changed', { message: 'the new template' });

      // Away and Back: the page left closes its stream (pagehide); restored
      // from the back/forward cache, it opens a new one (pageshow).
      const before = streams.length;
      const closedBefore = await page.eval(() => Number(sessionStorage.getItem('liveClosed') || 0));
      await page.goto(`${live.url}/plain`);
      const closed = await page.eval(() => Number(sessionStorage.getItem('liveClosed') || 0));
      assert.equal(closed, closedBefore + 1, 'the page left closed its stream on pagehide');
      await page.eval(() => history.back());
      await page.waitFor(() => location.pathname === '/' && window.__persisted !== undefined, { message: 'back on /' });
      const persisted = await page.eval(() => window.__persisted);
      const opened = Date.now() + 5000;
      while (streams.length < before + 2 && Date.now() < opened) await sleep(50);
      // /plain opened one; the page shown again opened another (from the
      // cache or loaded anew).
      assert.ok(streams.length >= before + 2, `streams: ${streams.length - before} more (persisted: ${persisted})`);
      assert.equal(persisted, true, 'restored from the back/forward cache');
    });
  } finally {
    await live.stop();
    rmSync(views, { recursive: true, force: true });
  }
});
