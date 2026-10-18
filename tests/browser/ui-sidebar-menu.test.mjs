// #37: `<rx-sidebar menu="staff"/>` (fixture /menu). Links that modules and the
// app add to a menu show up in the sidebar: grouped under a section heading,
// the hand-written link after them, a restricted link hidden from a visitor,
// the current page marked with aria-current (a child page keeps its parent
// marked), and a click on a link goes there and moves the mark.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

const DESKTOP = { width: 1280, height: 800 };

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

/** The sidebar as [kind, text] pairs, in order, and which links are current. */
const sidebar = (page) =>
  page.eval(() => {
    const parts = [...document.querySelectorAll('.rx-sidebar__section, .rx-sidebar__link')];
    return {
      order: parts.map((el) => [el.classList.contains('rx-sidebar__section') ? 'section' : 'link', el.textContent.trim()]),
      current: [...document.querySelectorAll('.rx-sidebar [aria-current]')].map((a) => a.textContent.trim()),
    };
  });

describe('the sidebar drawn from a menu', () => {
  test('groups, order, the hand-written link and no restricted link', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/menu`);
      const { order, current } = await sidebar(page);
      assert.deepEqual(order, [
        ['link', 'Overview'],
        ['section', 'Billing'],
        ['link', 'Reports'],
        ['link', 'Widgets'],
        ['link', 'By hand'],
      ]);
      assert.deepEqual(current, ['Overview']);
      page.assertClean();
    }, DESKTOP));

  test('a child page keeps its section marked, and a click moves the mark', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/menu/reports/7`);
      assert.deepEqual((await sidebar(page)).current, ['Reports']);
      await page.click('.rx-sidebar a[href="/menu"]');
      await page.waitFor(() => location.pathname === '/menu', { message: 'the overview page' });
      assert.deepEqual((await sidebar(page)).current, ['Overview']);
      await page.click('.rx-sidebar a[href="/menu/reports"]');
      await page.waitFor(() => location.pathname === '/menu/reports', { message: 'the reports page' });
      assert.deepEqual((await sidebar(page)).current, ['Reports']);
      page.assertClean();
    }, DESKTOP));
});
