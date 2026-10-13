// #269, the rest: examples/hello's main flows in a browser: the guestbook
// refusing a text file named .png and switching language. (The other
// examples' flows went with #351: htmx-recipes to bikeshop-htmx, fields to
// bikeshop-fields, a team's own host to bikeshop-stores, two-factor login to
// bikeshop-accounts; the kit's wizard, repeater, combobox, charts and the
// bell are ui-forms, ui-overlays and renoxjs on the fixture; checkout, the
// admin panel and plans are bikeshop-sales, -staff and -plans.)

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

let browser;

before(async () => {
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
});

/** Submits the form `button` is in and waits for the next page. */
async function submitAndLoad(page, button) {
  const loaded = page.once('Page.loadEventFired');
  await page.click(button);
  await loaded;
  await page.settle();
}

describe('the guestbook', () => {
  let app;
  let dir;
  before(async () => {
    app = await start('hello', 'examples/hello');
    dir = mkdtempSync(join(tmpdir(), 'renox-upload-'));
  });
  after(async () => {
    await app?.stop();
    rmSync(dir, { recursive: true, force: true });
  });

  test('a text file named .png is refused as a photo, and nothing is posted', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/`);
      const fake = join(dir, 'holiday.png');
      writeFileSync(fake, 'not a picture at all');
      const { root } = await page.send('DOM.getDocument', { depth: 0 });
      const { nodeId } = await page.send('DOM.querySelector', { nodeId: root.nodeId, selector: 'input[type=file][name=photo]' });
      await page.send('DOM.setFileInputFiles', { nodeId, files: [fake] });
      await page.type('#rx-name', 'Ana');
      await page.type('#rx-message', 'With a picture, supposedly');
      const entries = await page.eval(() => document.querySelector('#entries').textContent);
      await page.click('form[hx-post] button[type=submit]');
      await page.waitFor(() => document.querySelector('input[type=file][name=photo]').getAttribute('aria-invalid') === 'true', { message: 'the photo refused' });
      assert.match(await page.eval(() => document.querySelector('[data-error-for="photo"], #rx-photo-error, .error')?.textContent || ''), /image/i);
      assert.equal(await page.eval(() => document.querySelector('#entries').textContent), entries, 'nothing posted');
      page.assertClean({ allow: [/422/] });
    }));

  test('the language menu switches the page to Spanish and back', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/`);
      const english = await page.eval(() => document.querySelector('h1').textContent);
      await page.click('[aria-controls="language-menu"], #language-menu-button, [popovertarget="language-menu"]');
      await submitAndLoad(page, 'a[href$="/language/es"]');
      assert.equal(await page.eval(() => document.documentElement.lang), 'es');
      assert.notEqual(await page.eval(() => document.querySelector('h1').textContent), english);
      await page.goto(`${app.url}/language/en`);
      assert.equal(await page.eval(() => document.documentElement.lang), 'en');
      page.assertClean();
    }));
});
