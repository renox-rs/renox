// #269: examples/hello end to end in a browser under CSP=strict: a 422
// keeps the input, a post clears the form and lists the entry. (htmx-recipes'
// interactions moved to bikeshop-htmx.test.mjs and crud's live validation
// under CSP=strict to bikeshop-fields.test.mjs with #351.)

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

let browser;

before(async () => {
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
});

describe('the guestbook under CSP=strict', () => {
  let app;
  before(async () => {
    app = await start('hello', 'examples/hello', { env: { CSP: 'strict' } });
  });
  after(() => app?.stop());

  test('a 422 keeps the input; a post clears the form and lists the entry', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/`);
      await page.type('#rx-name', 'Ana');
      await page.type('#rx-message', 'Hi'); // too short (3 to 280)
      await page.click('form[hx-post] button[type=submit]');
      await page.waitFor(() => document.querySelector('#rx-message').getAttribute('aria-invalid') === 'true');
      assert.equal(await page.eval(() => document.querySelector('#rx-name').value), 'Ana', 'the input is kept');
      await page.type('#rx-message', 'Hello from a browser', { clear: true });
      await page.click('form[hx-post] button[type=submit]');
      await page.waitFor(() => document.querySelector('#entries').textContent.includes('Hello from a browser'));
      await page.waitFor(() => document.querySelector('#rx-message').value === '', { message: 'the form cleared' });
      page.assertClean({ allow: [/422/] });
    }));
});
