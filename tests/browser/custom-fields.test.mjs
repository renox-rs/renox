// #41: `<rx-custom-fields>` in a browser. The fixture's /parties/new form has
// the fields an administrator defined (passport no., segment, visa expiry):
// a form missing the required one comes back with the old input and the error
// on the right input; a valid one is saved, and /parties shows the custom
// values as entries and filters on the segment through a JSON path.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
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
  assert.deepEqual(names, ['extra[passport_no]', 'extra[segment]', 'extra[visa_expiry]']);
  assert.equal(await page.eval(() => document.querySelector('[name="extra[passport_no]"]').maxLength), 10);
  assert.equal(await page.eval(() => document.querySelector('[name="extra[visa_expiry]"]').type), 'date');
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
