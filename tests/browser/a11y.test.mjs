// #269: an accessibility smoke test. Every control on the bike shop's main
// pages (a customer's and the staff's) and the fixture's data grid pages
// has an accessible name (from Chrome's accessibility tree); the bike
// shop's /about/fields form is sent and its cart filled and checked out
// with the keyboard alone. (These drove examples crud, shop, grid and fields
// until #351.)

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { fixture, start } from './lib/app.mjs';

let browser;

before(async () => {
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
});

const CONTROLS = new Set([
  'button', 'link', 'textbox', 'searchbox', 'checkbox', 'radio', 'combobox', 'listbox', 'menuitem',
  'menuitemcheckbox', 'menuitemradio', 'tab', 'spinbutton', 'switch', 'slider', 'option',
]);

/** The controls in the accessibility tree, and those of them with no name. */
async function unnamed(page) {
  const { nodes } = await page.send('Accessibility.getFullAXTree');
  const controls = nodes.filter((n) => !n.ignored && CONTROLS.has(n.role?.value));
  const missing = controls
    .filter((n) => !String(n.name?.value ?? '').trim())
    .map((n) => `${n.role.value} (backend node ${n.backendDOMNodeId})`);
  return { count: controls.length, missing };
}

async function describeNodes(page, list) {
  const out = [];
  for (const item of list) {
    const id = Number(item.match(/node (\d+)/)[1]);
    try {
      const { outerHTML } = await page.send('DOM.getOuterHTML', { backendNodeId: id });
      out.push(`${item}: ${outerHTML.slice(0, 160)}`);
    } catch {
      out.push(item);
    }
  }
  return out;
}

async function submitAndLoad(page, selector) {
  const loaded = page.once('Page.loadEventFired');
  if (selector) await page.click(selector);
  else await page.press('Enter');
  await loaded;
  await page.settle();
}

async function logIn(page, url, email, password = 'password123') {
  await page.goto(`${url}/login`);
  // Already logged in (the browser keeps its cookies between tests).
  if (!(await page.eval(() => !!document.querySelector('#rx-email')))) return;
  await page.type('#rx-email', email);
  await page.type('#rx-password', password);
  await submitAndLoad(page, 'form button[type=submit]');
}

/** Tabs forward until `selector` has the focus (at most `max` presses). */
async function tabTo(page, selector, max = 60) {
  for (let i = 0; i < max; i++) {
    if (await page.eval((s) => document.activeElement?.matches(s), selector)) return;
    await page.press('Tab');
  }
  const at = await page.eval(() => document.activeElement?.outerHTML.slice(0, 120));
  throw new Error(`Tab never reached ${selector} (the focus is on ${at})`);
}

// Demo staff log in with a password only here (two-factor login is
// bikeshop-staff.test.mjs's).
const BIKESHOP = { binary: 'bikeshop', dir: 'examples/bikeshop', env: { BIKESHOP_STAFF_2FA: 'optional' } };
const pages = {
  'bikeshop (a customer)': { ...BIKESHOP, login: 'customer@bikeshop.test', password: 'password', paths: ['/', '/shop', '/cart', '/rent', '/service/book', '/plans', '/account', '/about/fields', '/about/htmx'] },
  'bikeshop (the owner)': { ...BIKESHOP, login: 'owner@bikeshop.test', password: 'password', paths: ['/staff', '/staff/stock', '/staff/reports/orders', '/admin', '/admin/products'] },
  'the data grid (fixture)': { fixture: true, login: 'demo@example.com', password: 'password', paths: ['/grid', '/grid/regions', '/grid/follow-up'] },
};

for (const [name, spec] of Object.entries(pages)) {
  describe(name, () => {
    let app;
    before(async () => {
      app = spec.fixture ? await fixture({ seed: true }) : await start(spec.binary, spec.dir, { seed: true, env: spec.env });
    });
    after(() => app?.stop());

    test('every control on its pages has a name', () =>
      browser.with(async (page) => {
        if (spec.login) await logIn(page, app.url, spec.login, spec.password);
        const problems = [];
        for (const path of spec.paths) {
          await page.goto(`${app.url}${path}`);
          const { count, missing } = await unnamed(page);
          assert.ok(count >= 3, `${path}: only ${count} controls in the tree`);
          if (missing.length) problems.push(`${path}:\n    ${(await describeNodes(page, missing)).join('\n    ')}`);
        }
        assert.deepEqual(problems, []);
      }));
  });
}

describe('with the keyboard alone', () => {
  let app;
  before(async () => {
    app = await start('bikeshop', 'examples/bikeshop', { seed: true });
    // Logged in from a tab of its own: in headless Chrome the tab that sent
    // the bike shop's login form gets no key presses afterwards (the cookie
    // is the browser's, so the tests' tabs are logged in too).
    await browser.with((page) => logIn(page, app.url, 'customer@bikeshop.test', 'password'));
  });
  after(() => app?.stop());

  test('the /about/fields form', () =>
    browser.with(async (page) => {
      await logIn(page, app.url, 'customer@bikeshop.test', 'password');
      await page.goto(`${app.url}/about/fields`);
      await page.eval(() => document.activeElement?.blur());
      await tabTo(page, '#sample-form [name=name]');
      await page.type('[name=name]', 'Keyboard bike');
      await tabTo(page, '#sample-form [name=stock]');
      await page.type('[name=stock]', '2', { clear: true });
      await tabTo(page, '#sample-form [name=weight_kg]');
      await page.type('[name=weight_kg]', '9.5', { clear: true });
      await tabTo(page, '#sample-form [name=price]');
      await page.type('[name=price]', '499', { clear: true });
      await submitAndLoad(page); // Enter in a field sends the form
      assert.match(await page.eval(() => document.querySelector('h1').textContent), /Keyboard bike/);
      page.assertClean();
    }));

  test('a bike into the cart, then the checkout', () =>
    browser.with(async (page) => {
      await logIn(page, app.url, 'customer@bikeshop.test', 'password');
      // A product in stock: a sold-out product's page has no cart button.
      await page.goto(`${app.url}/shop`);
      const products = await page.eval(() => [...new Set([...document.querySelectorAll('a[href^="/products/"]')].map((a) => a.getAttribute('href')))]);
      let found = false;
      for (const product of products) {
        await page.goto(`${app.url}${product}`);
        if (await page.eval(() => !!document.querySelector('form[action$="/cart"] button[type=submit]:not([disabled])'))) {
          found = true;
          break;
        }
      }
      assert.ok(found, `a product in stock among ${products.length}`);
      await page.eval(() => document.activeElement?.blur());
      await tabTo(page, 'form[action$="/cart"] button[type=submit]');
      await page.press('Enter');
      await page.settle();
      await page.goto(`${app.url}/cart`);
      await tabTo(page, 'a[href$="/checkout"]');
      await submitAndLoad(page);
      assert.equal(await page.eval(() => location.pathname), '/checkout');
      page.assertClean();
    }));
});
