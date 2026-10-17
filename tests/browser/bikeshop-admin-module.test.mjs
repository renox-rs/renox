// The bike shop's admin resource that a module contributes (#31): the
// suppliers come from the `purchasing` module, not from the panel's own list,
// yet they show in the admin navigation under their group and their list
// page opens.

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

async function logIn(page, app, email) {
  await page.send('Network.clearBrowserCookies');
  await page.goto(`${app.url}/login`);
  await page.type('#rx-email', email);
  await page.type('#rx-password', 'password');
  await page.click('form button[type=submit]');
  await page.waitFor(() => location.pathname !== '/login', { message: 'logged in' });
}

describe('bikeshop admin: a resource from a module', () => {
  let app;
  before(async () => {
    app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { BIKESHOP_STAFF_2FA: 'optional' } });
  });
  after(() => app?.stop());

  test('the suppliers are in the navigation and their list opens', () =>
    browser.with(async (page) => {
      await logIn(page, app, 'owner@bikeshop.test');
      await page.goto(`${app.url}/admin/products`);
      assert.ok(
        await page.eval(() => !!document.querySelector('a[href="/admin/suppliers"]')),
        'the navigation links to the contributed resource',
      );
      assert.ok((await page.text('body')).includes('Purchasing'), 'under its navigation group');
      await page.click('a[href="/admin/suppliers"]');
      await page.waitFor(() => location.pathname === '/admin/suppliers', { message: 'the list page' });
      assert.ok(await page.eval(() => !!document.querySelector('a[href="/admin/suppliers/create"]')));
      page.assertClean();
    }));
});
