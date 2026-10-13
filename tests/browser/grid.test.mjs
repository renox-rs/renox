// #267: renox-grid.js on the fixture's /grid pages (480 seeded orders; the grid example's until #351): sorting and
// paging through the URL, search as you type, the column menu remembered,
// resizing and moving columns, row details, and the phone layout.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser, sleep } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

let browser;
let app;

before(async () => {
  app = await fixture({ seed: true });
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

const onGrid = (fn, options) =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/grid`);
    await fn(page);
    page.assertClean();
  }, options);

/** The text of each row's cell in column `key`, in order. */
const column = (page, key) =>
  page.eval(
    (k) => [...document.querySelectorAll(`tbody tr[data-grid-row] td[data-col="${k}"]`)].map((td) => td.textContent.trim()),
    key,
  );

/** The columns of the first row, in the order they're drawn (visible ones). */
const columnOrder = (page) =>
  page.eval(() =>
    [...document.querySelectorAll('tbody tr[data-grid-row]:first-of-type td[data-col]')]
      .filter((td) => getComputedStyle(td).display !== 'none')
      .map((td) => td.dataset.col),
  );

test('a heading sorts, again reverses, and the URL keeps it', () =>
  onGrid(async (page) => {
    await page.click('th[data-col="customer"] [data-grid-sort]');
    await page.waitFor(() => new URL(location.href).searchParams.get('sort') === 'customer', { message: 'sort in the URL' });
    await page.settle();
    const up = await column(page, 'customer');
    assert.deepEqual(up, [...up].sort((a, b) => a.localeCompare(b, 'en', { sensitivity: 'base' })));
    await page.click('th[data-col="customer"] [data-grid-sort]');
    await page.waitFor(() => new URL(location.href).searchParams.get('sort') === '-customer');
    await page.settle();
    const down = await column(page, 'customer');
    assert.notDeepEqual(down, up);
    // Back restores the sort the page had.
    await page.eval(() => history.back());
    await page.waitFor(() => new URL(location.href).searchParams.get('sort') === 'customer');
  }));

test('pages and rows per page', () =>
  onGrid(async (page) => {
    await page.click('[data-grid-page="2"]');
    await page.waitFor(() => new URL(location.href).searchParams.get('page') === '2');
    await page.settle();
    assert.equal(await page.eval(() => document.querySelector('[data-grid-page][aria-current="page"]').textContent.trim()), '2');
    await page.eval(() => {
      const select = document.querySelector('[data-grid-per-page]');
      select.value = '10';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await page.waitFor(() => document.querySelectorAll('tbody tr[data-grid-row]').length === 10, { message: '10 rows' });
  }));

test('search waits for the typing to stop and sends one request', () =>
  onGrid(async (page) => {
    const searches = [];
    page.on('Network.requestWillBeSent', (p) => {
      if (new URL(p.request.url).searchParams.has('search')) searches.push(p.request.url);
    });
    const before = await page.text('.rx-grid__count');
    await page.type('[data-grid-search]', 'ana');
    await page.waitFor(() => new URL(location.href).searchParams.get('search') === 'ana', { message: 'search in the URL' });
    await page.settle();
    assert.ok(searches.length <= 2, `${searches.length} requests for three letters`);
    assert.notEqual(await page.text('.rx-grid__count'), before);
  }));

test('hiding a column in the menu is remembered across visits', () =>
  onGrid(async (page) => {
    const before = await columnOrder(page);
    assert.ok(before.includes('city'), JSON.stringify(before));
    await page.click('[popovertarget="grid-orders-columns"]');
    await page.waitFor(() => document.querySelector('#grid-orders-columns').matches(':popover-open'));
    await page.click('[data-grid-toggle="city"]');
    await page.waitFor(async () => true);
    await page.waitFor(
      () => getComputedStyle(document.querySelector('tbody tr[data-grid-row] td[data-col="city"]')).display === 'none',
      { message: 'city hidden' },
    );
    await sleep(500); // the preference is saved in the background
    await page.goto(`${app.url}/grid`);
    assert.ok(!(await columnOrder(page)).includes('city'), 'still hidden after a reload');
    // Reset brings it back.
    await page.click('[popovertarget="grid-orders-columns"]');
    await page.click('[data-grid-reset]');
    // Two requests in turn (the DELETE, then the grid reloaded): waited for
    // the column itself, since the page can look settled between them.
    await page.waitFor(
      () => !!document.querySelector('tbody tr[data-grid-row]:first-of-type td[data-col="city"]') &&
        getComputedStyle(document.querySelector('tbody tr[data-grid-row]:first-of-type td[data-col="city"]')).display !== 'none',
      { message: 'city shown again after the reset' },
    );
    assert.ok((await columnOrder(page)).includes('city'));
  }));

test('a column is resized by its edge and moved from the menu', () =>
  onGrid(async (page) => {
    const width = () => page.eval(() => document.querySelector('th[data-col="customer"]').getBoundingClientRect().width);
    const before = await width();
    const edge = await page.point('th[data-col="customer"] [data-grid-resize]');
    await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: edge.x, y: edge.y });
    await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: edge.x, y: edge.y, button: 'left', clickCount: 1 });
    for (let dx = 10; dx <= 80; dx += 10) {
      await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: edge.x + dx, y: edge.y, button: 'left' });
    }
    await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: edge.x + 80, y: edge.y, button: 'left', clickCount: 1 });
    await page.waitFor(async () => true);
    const after = await width();
    assert.ok(after > before + 40, `wider: ${before} → ${after}`);

    // Move "City" one place to the left with the menu's arrows.
    const first = await columnOrder(page);
    const at = first.indexOf('city');
    await page.click('[popovertarget="grid-orders-columns"]');
    await page.click('[data-grid-pick="city"] [data-grid-move="-1"]');
    const until = Date.now() + 5000;
    let moved = first;
    while (Date.now() < until) {
      moved = await columnOrder(page);
      if (moved.indexOf('city') === at - 1) break;
      await sleep(100);
    }
    assert.equal(moved[at - 1], 'city', JSON.stringify(moved));
  }));

test('a row opens its details', () =>
  onGrid(async (page) => {
    await page.click('tbody tr[data-grid-row] [data-grid-expand]');
    await page.waitFor(() => document.querySelector('[data-grid-expand]').getAttribute('aria-expanded') === 'true');
    // The row's details (from its template) are drawn under it.
    await page.waitFor(() => !!document.querySelector('tbody .rx-grid__details'), { message: 'details shown' });
    assert.match(await page.text('tbody .rx-grid__details'), /Created/);
    await page.click('tbody tr[data-grid-row] [data-grid-expand]');
    await page.waitFor(() => !document.querySelector('tbody .rx-grid__details'), { message: 'details folded away' });
  }));

test('a phone shows the phone columns', () =>
  browser.with(
    async (page) => {
      await page.goto(`${app.url}/grid`);
      const shown = await page.eval(() =>
        [...document.querySelectorAll('thead th[data-col]')].filter((th) => getComputedStyle(th).display !== 'none').length,
      );
      assert.ok(shown <= 4, `${shown} columns on a phone`);
      page.assertClean();
    },
    { width: 390, height: 844 },
  ));
