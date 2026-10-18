// #41: `<rx-custom-fields>` in a browser. The fixture's /parties/new form has
// the fields an administrator defined (passport no., segment, visa expiry):
// a form missing the required one comes back with the old input and the error
// on the right input; a valid one is saved, and /parties shows the custom
// values as entries and filters on the segment through a JSON path.
//
// #71: the Tax id field is sensitive. It is an empty password input in the
// form, it is stored sealed, /parties shows it as the mask to a guest, and the
// demo user (who holds the `custom_fields.reveal` ability) sees the value.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

let browser;
let app;

before(async () => {
  app = await fixture({ seed: true }); // the seed holds the demo user
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

async function submit(page, selector) {
  const loaded = page.once('Page.loadEventFired');
  await page.click(selector);
  await loaded;
  await page.settle();
}

/** Sets a select's or a date input's value the way a user's choice does. */
function setValue(page, selector, value) {
  return page.eval((sel, val) => {
    const el = document.querySelector(sel);
    el.value = val;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }, selector, value);
}

async function addParty(page, { name, passport, segment, visa }) {
  await page.goto(`${app.url}/parties/new`);
  await page.type('[name=name]', name);
  if (passport) await page.type('[name="extra[passport_no]"]', passport);
  if (segment) await setValue(page, '[name="extra[segment]"]', segment);
  if (visa) await setValue(page, '[name="extra[visa_expiry]"]', visa);
  await submit(page, '#party button[type=submit]');
}

test('the form draws an input per custom field, named extra[key]', () => browser.with(async (page) => {
  await page.goto(`${app.url}/parties/new`);
  const names = await page.eval(() => [...document.querySelectorAll('#custom [name]')].map((e) => e.name));
  assert.deepEqual(names, ['extra[passport_no]', 'extra[segment]', 'extra[visa_expiry]', 'extra[tax_id]']);
  assert.equal(await page.eval(() => document.querySelector('[name="extra[passport_no]"]').maxLength), 10);
  assert.equal(await page.eval(() => document.querySelector('[name="extra[visa_expiry]"]').type), 'date');
  // A sensitive field is an empty password input that says "unchanged".
  assert.equal(await page.eval(() => document.querySelector('[name="extra[tax_id]"]').type), 'password');
  assert.equal(await page.eval(() => document.querySelector('[name="extra[tax_id]"]').value), '');
  assert.equal(await page.eval(() => document.querySelector('[name="extra[tax_id]"]').placeholder), 'unchanged');
  page.assertClean();
}));

test('a missing required value comes back with the old input and the error on its input', () => browser.with(async (page) => {
  await page.goto(`${app.url}/parties/new`);
  await page.type('[name=name]', 'Nadia');
  await setValue(page, '[name="extra[segment]"]', 'corporate');
  await submit(page, '#party button[type=submit]');
  assert.equal(await page.eval(() => location.pathname), '/parties/new');
  assert.equal(await page.eval(() => document.querySelector('[name=name]').value), 'Nadia');
  assert.equal(await page.eval(() => document.querySelector('[name="extra[segment]"]').value), 'corporate');
  assert.equal(await page.eval(() => document.querySelector('[name="extra[passport_no]"]').getAttribute('aria-invalid')), 'true');
  assert.equal(await page.eval(() => document.querySelector('[name="extra[segment]"]').hasAttribute('aria-invalid')), false);
}));

test('saved values show as entries and the list filters on a custom field', () => browser.with(async (page) => {
  await addParty(page, { name: 'Ruth', passport: 'P100', segment: 'retail', visa: '2030-05-17' });
  await addParty(page, { name: 'Corin', passport: 'P200', segment: 'corporate' });
  await page.goto(`${app.url}/parties`);
  const names = () => page.eval(() => [...document.querySelectorAll('#parties .party')].map((li) => li.dataset.name));
  assert.deepEqual(await names(), ['Ruth', 'Corin']);
  const ruth = await page.eval(() => document.querySelector('#parties .party').textContent);
  assert.match(ruth, /Passport no\./);
  assert.match(ruth, /P100/);
  assert.match(ruth, /retail/);

  await setValue(page, '#filter [name=segment]', 'corporate');
  await submit(page, '#filter button[type=submit]');
  assert.deepEqual(await names(), ['Corin']);
  assert.equal(await page.eval(() => document.querySelector('#filter [name=segment]').value), 'corporate');

  await setValue(page, '#filter [name=segment]', 'retail');
  await submit(page, '#filter button[type=submit]');
  assert.deepEqual(await names(), ['Ruth']);
  page.assertClean();
}));

test('a sensitive value is masked for a guest and revealed to the viewer who holds the ability', () => browser.with(async (page) => {
  const secret = 'TX-FAKE-4471';
  await page.goto(`${app.url}/parties/new`);
  await page.type('[name=name]', 'Tess');
  await page.type('[name="extra[passport_no]"]', 'P300');
  await page.type('[name="extra[tax_id]"]', secret);
  await submit(page, '#party button[type=submit]');
  assert.equal(await page.eval(() => location.pathname), '/parties');

  // A guest sees the mask: the value and the sealed token are nowhere in the page.
  const entry = () => page.eval(() => {
    const li = [...document.querySelectorAll('#parties .party')].find((e) => e.dataset.name === 'Tess');
    return li.textContent;
  });
  const guest = await entry();
  assert.match(guest, /Tax id/);
  assert.match(guest, /••••••/);
  assert.match(guest, /P300/);
  const html = await page.eval(() => document.documentElement.outerHTML);
  assert.ok(!html.includes(secret), 'the plain value is not in the page');
  assert.ok(!html.includes('rx-sealed'), 'the sealed token is not in the page');

  // The demo user holds the ability.
  await page.goto(`${app.url}/login`);
  await page.type('#rx-email', 'demo@example.com');
  await page.type('#rx-password', 'password');
  await submit(page, 'form button[type=submit]');
  await page.goto(`${app.url}/parties`);
  const revealed = await entry();
  assert.match(revealed, new RegExp(secret));
  assert.ok(!revealed.includes('••••••'));

  // Even then the form never holds the stored value.
  await page.goto(`${app.url}/parties/new`);
  assert.equal(await page.eval(() => document.querySelector('[name="extra[tax_id]"]').value), '');
  page.assertClean();
}));
