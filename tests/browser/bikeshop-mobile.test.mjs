// The bike shop on a phone (#323): one short bar on top (the brand, a search
// button, the cart, "About this page", the bell), a tab bar at the bottom whose last tab opens
// a menu panel, no page wider than the screen; on a wide screen the bar as
// before and no tab bar. Also the login page's demo accounts, which fill the
// form when tapped. Screenshots go to BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';
import { logInWithFetch } from './lib/bikeshop.mjs';

const STAFF_2FA_OPTIONAL = { BIKESHOP_STAFF_2FA: 'optional' };
const PHONE = { width: 390, height: 844, deviceScaleFactor: 1, mobile: true };

let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: STAFF_2FA_OPTIONAL });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

const visible = (page, selector) =>
  page.eval((s) => {
    const el = document.querySelector(s);
    if (!el) return false;
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0 && getComputedStyle(el).visibility !== 'hidden';
  }, selector);

/** Opens the menu panel from the tab bar and waits until it stops sliding. */
async function openMenu(page) {
  await page.click('.rx-tabbar [data-rx-open="site-menu"]');
  await page.waitFor(
    () => {
      const menu = document.querySelector('#site-menu');
      return menu?.open && menu.getAnimations({ subtree: true }).every((a) => a.playState !== 'running');
    },
    { message: 'the menu open' },
  );
}

/** Logs in as a seeded demo user (password `password`) through the login
 *  form's fields, sent from the page: the demo accounts' box has a test of its own
 *  below, so the others don't depend on it. */
async function logIn(page, email) {
  const status = await logInWithFetch(page, app.url, email);
  assert.ok(!status.endsWith(' /login'), `logged in as ${email} (${status})`);
}

describe('bikeshop on a phone', () => {
  test('one short bar, the search behind a button, tabs at the bottom', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.send('Emulation.setDeviceMetricsOverride', PHONE);
      await page.goto(`${app.url}/shop`);
      const bar = await page.eval(() => document.querySelector('.rx-navbar').getBoundingClientRect().height);
      assert.ok(bar < 70, `the bar is one row (${bar}px)`);
      for (const hidden of ['.rx-navbar__links', '[aria-controls="language-menu"]', '#nav-search']) {
        assert.equal(await visible(page, hidden), false, `${hidden} is out of the bar`);
      }
      assert.ok(await visible(page, '#nav-cart'), 'the cart stays');
      assert.ok(await visible(page, '.rx-navbar [data-rx-open="about-page"]'), '"About this page" stays');
      assert.ok(await visible(page, '.rx-tabbar'), 'the tab bar shows');
      assert.equal(await page.eval(() => document.querySelector('.rx-tabbar [aria-current=page]')?.textContent.trim()), 'Shop');
      // Scrolled to the end, the footer sits above the tab bar, not under it.
      const [footerBottom, barTop] = await page.eval(() => {
        window.scrollTo(0, document.documentElement.scrollHeight);
        return [document.querySelector('.bs-footer').getBoundingClientRect().bottom, document.querySelector('.rx-tabbar').getBoundingClientRect().top];
      });
      assert.ok(footerBottom <= barTop, `the footer clears the tab bar (${footerBottom} > ${barTop})`);
      await page.eval(() => window.scrollTo(0, 0));
      await shot(page, 'mobile-01-shop');

      // The search button opens the box under the bar and focuses it; Escape closes it.
      await page.click('[data-rx-search-toggle]');
      await page.waitFor(() => document.activeElement?.id === 'nav-search', { message: 'the search box focused' });
      assert.equal(await page.eval(() => document.querySelector('[data-rx-search-toggle]').getAttribute('aria-expanded')), 'true');
      await shot(page, 'mobile-02-search');
      await page.press('Escape');
      await page.waitFor(() => !document.querySelector('.rx-navbar').classList.contains('rx-navbar--searching'), { message: 'the search box closed' });
      assert.equal(await page.eval(() => document.activeElement?.hasAttribute('data-rx-search-toggle')), true, 'the focus goes back to the button');

      // The menu tab: the other links, the language and logging in.
      await openMenu(page);
      const menu = await page.text('#site-menu');
      for (const part of ['Service plans', 'English', 'Español', 'Log in', 'Register']) {
        assert.ok(menu.includes(part), `the menu has "${part}"`);
      }
      await shot(page, 'mobile-03-menu');
      // The language from the menu.
      await page.click('#site-menu form:nth-of-type(2) button');
      // Mid-load the document may have no root element yet, hence the `?.`.
      await page.waitFor(() => document.documentElement?.lang === 'es', { message: 'Spanish' });
      assert.equal(await page.eval(() => document.querySelector('.rx-tabbar__label').textContent.trim()), 'Inicio');
      await openMenu(page);
      await page.click('#site-menu form:nth-of-type(1) button');
      await page.waitFor(() => document.documentElement?.lang === 'en', { message: 'English again' });
    }));

  test('logged in: the bell in the bar, the account in the menu', () =>
    browser.with(async (page) => {
      await page.send('Emulation.setDeviceMetricsOverride', PHONE);
      await logIn(page, 'customer@bikeshop.test');
      await page.goto(`${app.url}/`);
      assert.ok(await visible(page, '.rx-navbar [data-rx-bell]'), 'the bell stays in the bar');
      assert.equal(await visible(page, '[aria-controls="account-menu"]'), false, 'the account menu is in the panel');
      await openMenu(page);
      const menu = await page.text('#site-menu');
      for (const part of ['Account', 'My plans', 'Log out']) assert.ok(menu.includes(part), `the menu has "${part}"`);
      await shot(page, 'mobile-04-menu-customer');
    }));

  test('no page is wider than the phone', () =>
    browser.with(async (page) => {
      await page.send('Emulation.setDeviceMetricsOverride', PHONE);
      await logIn(page, 'owner@bikeshop.test');
      // The pages that once were: a timeline, a role matrix, long feature names and code.
      const paths = ['/', '/shop', '/rent?store=1', '/about/pages', '/about/data', '/staff/roles', '/plans', '/workshop/book'];
      for (const path of paths) {
        await page.goto(`${app.url}${path}`);
        const [scroll, width] = await page.eval(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
        assert.ok(scroll <= width + 1, `${path} fits the phone (${scroll} > ${width})`);
      }
    }));

  test('a wide screen keeps the bar and has no tab bar', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.goto(`${app.url}/shop`);
      assert.ok(await visible(page, '.rx-navbar__links'), 'the links in the bar');
      assert.ok(await visible(page, '#nav-search'), 'the search box in the bar');
      assert.ok(await visible(page, '[aria-controls="language-menu"]'), 'the language in the bar');
      assert.equal(await visible(page, '.rx-tabbar'), false, 'no tab bar');
      assert.equal(await visible(page, '[data-rx-search-toggle]'), false, 'no search button');
    }));
});

describe('bikeshop demo accounts', () => {
  test('the login page lists them and a tap fills the form', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.goto(`${app.url}/login`);
      const box = await page.text('.bs-demo-logins');
      assert.ok(box.includes('password'), 'the password is shown');
      for (const email of ['customer@bikeshop.test', 'owner@bikeshop.test', 'manager.north@bikeshop.test']) {
        assert.ok(box.includes(email), `${email} is listed`);
      }
      await shot(page, 'mobile-05-login-demo');
      await page.click('[data-bs-demo-email="manager.north@bikeshop.test"]');
      assert.deepEqual(
        await page.eval(() => [document.querySelector('[name=email]').value, document.querySelector('[name=password]').value]),
        ['manager.north@bikeshop.test', 'password'],
      );
      await page.click('form button[type=submit]');
      await page.waitFor(() => location.pathname !== '/login', { message: 'logged in' });
      // Only on the login page.
      await page.send('Network.clearBrowserCookies');
      await page.goto(`${app.url}/register`);
      assert.equal(await page.eval(() => !!document.querySelector('.bs-demo-logins')), false, 'not on the register page');
    }));
});
