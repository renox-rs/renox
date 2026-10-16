// #42: passwordless login in a real browser: a magic link opened in the
// browser logs nobody in (a mail scanner would), its button does, and only
// once; a code sent by mail is typed on the verify page, and a wrong one is
// refused next to its field. The fixture's mails are read at /_renox/mail.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser, sleep } from './lib/cdp.mjs';
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

const unescape = (html) =>
  html.replaceAll('&lt;', '<').replaceAll('&gt;', '>').replaceAll('&quot;', '"').replaceAll('&amp;', '&');

/** The text of the newest mail with `subject`, once the queue has sent it. */
async function mailText(subject) {
  const until = Date.now() + 20_000;
  while (Date.now() < until) {
    const list = await (await fetch(`${app.url}/_renox/mail`)).text();
    const rows = [...list.matchAll(/href="\/_renox\/mail\/(\d+)">([^<]*)</g)];
    const row = rows.find(([, , title]) => title === subject);
    if (row) {
      const page = await (await fetch(`${app.url}/_renox/mail/${row[1]}`)).text();
      return unescape(page.match(/<h2>Text<\/h2><pre>([\s\S]*?)<\/pre>/)[1]);
    }
    await sleep(200);
  }
  throw new Error(`no "${subject}" mail within 20 s`);
}

const logout = (page) =>
  page.eval(() =>
    fetch('/logout', {
      method: 'POST',
      headers: { 'X-CSRF-Token': document.querySelector('meta[name="csrf-token"]').content },
    }),
  );

test('the login page lists the passwordless options', () => browser.with(async (page) => {
  await page.goto(`${app.url}/login`);
  assert.equal(await page.text('[data-passwordless=link] .rx-button__label'), 'Email me a login link');
  assert.equal(await page.text('[data-passwordless=code] .rx-button__label'), 'Log in with a code');
  page.assertClean();
}));

test('a magic link logs in only when its button is clicked, and only once', () => browser.with(async (page) => {
  const email = `link${Date.now()}@example.com`;
  await page.goto(`${app.url}/login`);
  const opened = page.once('Page.loadEventFired');
  await page.click('[data-passwordless=link]');
  await opened;
  await page.settle();
  await page.type('#rx-email', email);
  await page.click('form button[type=submit]');
  await page.waitFor(() => document.body.textContent.includes('a link is on its way'), {
    message: 'the "link is on its way" message',
  });

  const mail = await mailText('Your login link');
  const link = mail.match(/https?:\/\/\S+\/login\/link\/\S+/)[0];

  // Opening the link (as a mail scanner does) shows who it would log in.
  await page.goto(link);
  assert.equal(await page.text('[data-login-link-email]'), email);
  await page.goto(`${app.url}/login`);
  assert.equal(await page.eval(() => location.pathname), '/login', 'still logged out after opening it');

  // The button logs in.
  await page.goto(link);
  const loaded = page.once('Page.loadEventFired');
  await page.click('form button[type=submit]');
  await loaded;
  await page.goto(`${app.url}/login`);
  assert.notEqual(await page.eval(() => location.pathname), '/login', 'logged in: the login page sends them away');

  // The link is used up.
  await logout(page);
  await page.goto(link);
  assert.match(await page.text('body'), /invalid or has expired/);
  page.assertClean();
}));

test('a login code is typed on the verify page; a wrong one stays next to its field', () => browser.with(async (page) => {
  const email = `code${Date.now()}@example.com`;
  await page.goto(`${app.url}/login/code`);
  await page.type('#rx-identifier', email);
  const verify = page.once('Page.loadEventFired');
  await page.click('form button[type=submit]');
  await verify;
  await page.waitFor(() => location.pathname === '/login/code/verify');
  await page.settle();

  const mail = await mailText('Your login code');
  const code = mail.match(/\b(\d{6})\b/)[1];
  const wrong = code === '000000' ? '111111' : '000000';

  await page.type('#rx-code', wrong);
  await page.click('form button[type=submit]');
  await page.waitFor(() => document.querySelector('#rx-code')?.getAttribute('aria-invalid') === 'true', {
    message: 'the wrong code marked invalid',
  });
  assert.equal(await page.eval(() => location.pathname), '/login/code/verify');

  await page.type('#rx-code', code, { clear: true });
  const loaded = page.once('Page.loadEventFired');
  await page.click('form button[type=submit]');
  await loaded;
  await page.goto(`${app.url}/login`);
  assert.notEqual(await page.eval(() => location.pathname), '/login', 'logged in');
  page.assertClean({ allow: [/422/] });
}));
