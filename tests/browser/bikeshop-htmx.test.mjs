// /about/htmx in a browser (#351; these were the htmx-recipes example's tests in
// examples.test.mjs and examples-flows.test.mjs): the modal adds a row and
// the count follows out of band, a repeat is retargeted onto its row with a
// toast, a row is ticked, renamed inline (Escape cancels) and deleted from
// its menu after hx-confirm, the infinite scroll adds each bike once, Alpine
// tabs filter without a request, and toasts survive HX-Refresh and
// HX-Redirect. All under CSP=strict, so the Alpine logic must live in
// `Alpine.data`. Screenshots go to BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { CSP: 'strict' } });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

const onPage = (fn, options) =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/about/htmx`);
    await page.waitFor(() => !!window.Alpine);
    await fn(page);
    page.assertClean({ allow: [/422/] });
  }, options);

const openCount = (page) => page.eval(() => parseInt(document.querySelector('#open-count').textContent, 10) || 0);
const firstRow = (page) => page.eval(() => document.querySelector('#checklist li[id^="item-"]').id);

describe('/about/htmx under CSP=strict', () => {
  test('the page fits a phone and a desktop, light and dark', async () => {
    for (const [size, options] of [['desktop', undefined], ['phone', PHONE]]) {
      for (const scheme of ['light', 'dark']) {
        await browser.with(async (page) => {
          await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: scheme }] });
          await page.goto(`${app.url}/about/htmx`);
          assert.ok(await page.eval(() => document.documentElement.scrollWidth <= window.innerWidth), 'no sideways scrolling');
          await sleep(400);
          await shot(page, `htmx-${size}-${scheme}`);
          page.assertClean();
        }, options);
      }
    }
  });

  test('the modal form adds a row at the top and updates the count out of band', () =>
    onPage(async (page) => {
      const before = await openCount(page);
      // Its keyboard shortcut opens it.
      await page.eval(() => document.activeElement?.blur());
      await page.press('n');
      await page.waitFor(() => document.querySelector('#new-item').open);
      await page.click('#new-item button[type=submit]');
      await page.waitFor(() => document.querySelector('#new-title').getAttribute('aria-invalid') === 'true');
      assert.ok(await page.eval(() => document.querySelector('#new-item').open), 'the 422 stays in the sheet');
      await page.type('#new-title', 'Fill the bottle');
      await page.click('#new-item button[type=submit]');
      await page.waitFor(() => !document.querySelector('#new-item').open);
      await page.waitFor(() => document.querySelector('#checklist li')?.textContent.includes('Fill the bottle'));
      await page.waitFor((b) => parseInt(document.querySelector('#open-count').textContent, 10) === b + 1, {}, before);
    }));

  test('an item already on the list: the answer replaces its row (HX-Retarget, HX-Reswap) with a toast', () =>
    onPage(async (page) => {
      const first = await page.eval(() => {
        const li = document.querySelector('#checklist li[id^="item-"]');
        return { id: li.id, title: li.querySelector('.rx-list__main').textContent.trim() };
      });
      const rows = () => page.eval(() => document.querySelectorAll('#checklist li[id^="item-"]').length);
      const before = await rows();
      await page.eval(() => document.activeElement?.blur());
      await page.press('n');
      await page.waitFor(() => document.querySelector('#new-item').open);
      await page.type('#new-title', first.title);
      await page.eval((i) => (document.getElementById(i).dataset.probe = 'old'), first.id);
      await page.click('#new-item button[type=submit]');
      await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('already on the list'));
      await page.waitFor((i) => document.getElementById(i) && !document.getElementById(i).dataset.probe, { message: 'the row swapped in place' }, first.id);
      assert.equal(await rows(), before, 'no second row');
    }));

  test('an item is ticked in place, renamed inline and deleted from its menu', () =>
    onPage(async (page) => {
      const id = await page.eval(() => document.querySelector('#checklist li input[type=checkbox]:not(:checked)').closest('li').id);
      const before = await openCount(page);
      await page.click(`#${id} input[type=checkbox]`);
      await page.waitFor((b) => parseInt(document.querySelector('#open-count').textContent, 10) === b - 1, {}, before);
      await page.settle();

      // Escape cancels an edit without a PATCH.
      const patches = [];
      page.on('Network.requestWillBeSent', (p) => p.request.method === 'PATCH' && patches.push(p.request.url));
      const title = await page.text(`#${id} .rx-list__main`);
      const dblclick = (i) => document.querySelector(`#${i} .rx-list__main`).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
      // A row htmx just swapped in listens once htmx has processed it.
      const ready = (i) => !!document.querySelector(`#${i} .rx-list__main`)?.['htmx-internal-data'];
      await page.waitFor(ready, { message: 'the row ready for htmx' }, id);
      await page.eval(dblclick, id);
      await page.waitFor((i) => !!document.querySelector(`#${i} input[name=title]`), {}, id);
      await page.type(`#${id} input[name=title]`, 'Never saved', { clear: true });
      await page.press('Escape');
      await page.waitFor((i) => !!document.querySelector(`#${i} .rx-list__main`), { message: 'the row back' }, id);
      assert.equal(await page.text(`#${id} .rx-list__main`), title);
      assert.deepEqual(patches, []);

      // Double-click the title: a form; Enter saves.
      await page.waitFor(ready, { message: 'the row ready for htmx' }, id);
      await page.eval(dblclick, id);
      await page.waitFor((i) => !!document.querySelector(`#${i} input[name=title]`), {}, id);
      await page.type(`#${id} input[name=title]`, 'Renamed item', { clear: true });
      await page.press('Enter');
      await page.waitFor((i) => document.querySelector(`#${i} .rx-list__main`)?.textContent.includes('Renamed item'), {}, id);

      // Delete from the menu (hx-confirm's dialog is accepted).
      await page.click(`#${id} [aria-haspopup="menu"]`);
      await page.click(`#${id} .rx-menu__item--danger`);
      await page.waitFor((i) => !document.querySelector(`#${i}`), { message: 'the row removed' }, id);
      assert.deepEqual(page.dialogs.map((d) => d.message), ['Delete “Renamed item”?']);
      await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('deleted'));
    }));

  test('scrolling to the end loads the next bikes once each', () =>
    onPage(async (page) => {
      const links = () => page.eval(() => [...document.querySelectorAll('#bikes li a[href^="/products/"]')].map((a) => a.getAttribute('href')));
      const first = await links();
      await page.eval(() => document.querySelector('#bikes [hx-trigger="revealed"]').scrollIntoView());
      await page.waitFor((n) => document.querySelectorAll('#bikes li a[href^="/products/"]').length > n, {}, first.length);
      await page.settle();
      const all = await links();
      assert.equal(new Set(all).size, all.length, 'no bike twice');
    }));

  test('Alpine tabs filter the rows without a request', () =>
    onPage(async (page) => {
      const requests = [];
      page.on('Network.requestWillBeSent', (p) => requests.push(p.request.url));
      const visible = () => page.eval(() => [...document.querySelectorAll('#checklist li[id^="item-"]')].filter((li) => li.offsetParent).length);
      const all = await visible();
      await page.click('[role=tab]:nth-of-type(3)'); // Done
      await sleep(100);
      assert.ok((await visible()) < all);
      assert.equal(await page.eval(() => document.querySelector('[role=tab]:nth-of-type(3)').getAttribute('aria-selected')), 'true');
      await page.click('[role=tab]:nth-of-type(1)'); // All
      await sleep(100);
      assert.equal(await visible(), all);
      assert.deepEqual(requests, []);
    }));

  test('toasts come with a refreshed page (HX-Refresh) and a redirect (HX-Redirect)', () =>
    onPage(async (page) => {
      const id = await firstRow(page);
      if (!(await page.eval((i) => document.querySelector(`#${i} input[type=checkbox]`).checked, id))) {
        await page.click(`#${id} input[type=checkbox]`);
        await page.settle();
      }
      let loaded = page.once('Page.loadEventFired');
      await page.eval(() => document.querySelector('form[hx-post$="/about/htmx/clear-done"] button').click());
      await loaded;
      await page.settle();
      await page.waitFor(() => /cleared/.test(document.querySelector('.rx-toast')?.textContent || ''), { message: 'the toast after HX-Refresh' });
      // Ready: off to the shop, with its toast.
      loaded = page.once('Page.loadEventFired');
      await page.click('form[hx-post$="/about/htmx/ready"] button');
      await loaded;
      await page.settle();
      assert.equal(await page.eval(() => location.pathname), '/shop');
      await page.waitFor(() => /good ride/.test(document.querySelector('.rx-toast')?.textContent || ''), { message: 'the toast after HX-Redirect' });
    }));

  test('the live component version adds, ticks, renames and deletes (rx-click, rx-model)', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/about/htmx/live`);
      await page.waitFor(() => !!window.RenoxLive);
      const titles = () => page.eval(() => [...document.querySelectorAll('#checklist li .rx-list__main')].map((e) => e.textContent.trim()));
      const rowOf = (title) => page.eval((t) => [...document.querySelectorAll('#checklist li')].find((li) => li.textContent.includes(t))?.id, title);

      // Add: an empty title is refused under the field, then it is added.
      await page.click('form[rx-submit=add] button[type=submit]');
      await page.waitFor(() => document.querySelector('#new-title').getAttribute('aria-invalid') === 'true');
      await page.type('#new-title', 'Fill the bottle');
      await page.click('form[rx-submit=add] button[type=submit]');
      await page.waitFor(() => document.querySelector('#checklist li')?.textContent.includes('Fill the bottle'));
      const open = () => page.eval(() => parseInt(document.querySelector('#open-count').textContent, 10));
      const before = await open();
      let id = await rowOf('Fill the bottle');

      // Tick it: the count follows.
      await page.click(`#${id} input[type=checkbox]`);
      await page.waitFor((b) => parseInt(document.querySelector('#open-count').textContent, 10) === b - 1, {}, before);

      // Rename it inline.
      await page.click(`#${id} button[rx-click^="edit"]`);
      await page.waitFor((i) => !!document.querySelector(`#${i} input[name=edit_title]`), {}, id);
      await page.type(`#${id} input[name=edit_title]`, 'Fill both bottles', { clear: true });
      await page.click(`#${id} button[type=submit]`);
      await page.waitFor(() => document.querySelector('#checklist')?.textContent.includes('Fill both bottles'));

      // Delete it.
      id = await rowOf('Fill both bottles');
      await page.click(`#${id} button[rx-click^="delete"]`);
      await page.waitFor(() => !document.querySelector('#checklist').textContent.includes('Fill both bottles'));
      assert.ok(!(await titles()).includes('Fill both bottles'));
      page.assertClean({ allow: [/422/] });
    }));
});
