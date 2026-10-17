// #39: `<rx-transition-actions>` in a browser. A logged-in user opens a ticket
// and sees one button per transition it can run now; "Escalate" posts to the
// generic transition route and the page comes back showing the new state and
// its history line; "Close" asks for a reason first, and once the ticket is
// closed no button is left.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

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

test('available transitions are buttons that post to the transition route', () => browser.with(async (page) => {
  await page.goto(`${app.url}/form`);
  // A user, and a ticket (the page that makes one answers with its id).
  await page.eval(async (email) => {
    const token = document.querySelector('meta[name="csrf-token"]').content;
    const body = new URLSearchParams({ _token: token, name: 'Tess', email, password: 'password123', password_confirmation: 'password123' });
    await fetch('/register', { method: 'POST', body, redirect: 'manual' });
  }, `tess${Date.now()}@example.com`);
  // A login rotates the CSRF token: read the new one from a fresh page.
  await page.goto(`${app.url}/form`);
  const id = await page.eval(async () => {
    const token = document.querySelector('meta[name="csrf-token"]').content;
    const made = await fetch('/tickets', { method: 'POST', headers: { 'X-CSRF-Token': token } });
    return made.text();
  });
  assert.match(id, /^\d+$/, `a ticket was made: ${id}`);

  await page.goto(`${app.url}/tickets/${id}`);
  const names = () => page.eval(() => [...document.querySelectorAll('#actions [data-transition]')].map((f) => f.dataset.transition));
  assert.deepEqual(await names(), ['escalate', 'close']);
  assert.equal(await page.eval(() => document.getElementById('status').textContent), 'open');
  assert.equal(await page.eval(() => document.querySelectorAll('#actions input[name=reason]').length), 1, 'only "close" asks for a reason');

  await page.click('#actions [data-transition=escalate] button');
  await page.waitFor(() => document.getElementById('status').textContent === 'escalated', { message: 'the ticket was escalated' });
  assert.deepEqual(await names(), ['close'], 'escalate is gone, close is left');
  assert.match(await page.eval(() => document.getElementById('log').textContent), /Escalate: open → escalated/);

  // The reason field is required: the form does not post without it.
  await page.type('#actions input[name=reason]', 'customer left');
  await page.click('#actions [data-transition=close] button');
  await page.waitFor(() => document.getElementById('status').textContent === 'closed', { message: 'the ticket was closed' });
  assert.deepEqual(await names(), [], 'a closed ticket has no buttons');
  assert.match(await page.eval(() => document.getElementById('log').textContent), /customer left/);
}));
