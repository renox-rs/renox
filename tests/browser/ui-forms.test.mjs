// #265: the form half of renox-ui.js in a real browser: busy buttons (and
// after Back), live validation, revealable and copyable fields, file
// fields, show_when, tags, the searchable select (local, required, and with
// options from the server), the date picker, the repeater, the wizard, and
// a button disabled with a reason.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
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

/** Runs `fn` on a fresh /widgets page, closed afterwards whatever happens. */
const onWidgets = (fn) =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/widgets`);
    await fn(page);
    page.assertClean({ allow: [/422/] });
  });

test('a regular form is sent once however often it is clicked; an htmx one shows it is busy', () =>
  onWidgets(async (page) => {
    const sent = [];
    page.on('Network.requestWillBeSent', (p) => p.request.method === 'POST' && sent.push(p.request.url));
    // htmx: the button is marked busy while the request runs.
    await page.click('#busy');
    await page.waitFor(() => document.querySelector('#busy').getAttribute('aria-busy') === 'true', {
      message: 'the htmx button marked busy',
    });
    await page.waitFor(() => !document.querySelector('#busy').hasAttribute('aria-busy'), { message: 'busy cleared' });
    // A regular form: a second click while it's sending is ignored (both
    // clicks in one go, before the page can change).
    const loaded = page.once('Page.loadEventFired');
    const busy = await page.eval(() => {
      const button = document.querySelector('#plain-busy-button');
      button.click();
      const marked = button.getAttribute('aria-busy');
      button.click();
      return marked;
    });
    assert.equal(busy, 'true', 'the regular button is marked busy');
    await loaded;
    assert.equal(sent.filter((u) => u.endsWith('/slow-plain')).length, 1, JSON.stringify(sent));
  }));

test('a password can be shown and hidden again', () => onWidgets(async (page) => {
  await page.click('[data-rx-reveal="rx-secret"]');
  assert.deepEqual(
    await page.eval(() => [document.querySelector('#rx-secret').type, document.querySelector('[data-rx-reveal]').getAttribute('aria-pressed')]),
    ['text', 'true'],
  );
  await page.click('[data-rx-reveal="rx-secret"]');
  assert.deepEqual(
    await page.eval(() => [document.querySelector('#rx-secret').type, document.querySelector('[data-rx-reveal]').getAttribute('aria-pressed')]),
    ['password', 'false'],
  );
}));

test('show_when shows a group and disables it while hidden', () => onWidgets(async (page) => {
  const state = () =>
    page.eval(() => {
      const group = document.querySelector('[data-rx-show-when="method"]');
      return { hidden: group.hidden || getComputedStyle(group).display === 'none', disabled: group.disabled };
    });
  assert.deepEqual(await state(), { hidden: true, disabled: true });
  await page.click('label[for="rx-method-2"]');
  await page.waitFor(() => !document.querySelector('[data-rx-show-when="method"]').disabled);
  assert.deepEqual(await state(), { hidden: false, disabled: false });
  // A disabled group's fields aren't sent with the form.
  await page.click('label[for="rx-method"]');
  const sent = await page.eval(() => [...new FormData(document.querySelector('#fields')).keys()]);
  assert.ok(!sent.includes('address'), JSON.stringify(sent));
}));

test('tags: Enter and comma add, duplicates are refused, Backspace removes', () => onWidgets(async (page) => {
  const tags = () => page.eval(() => [...document.querySelectorAll('[data-rx-tags="labels"] .rx-tag input[type=hidden]')].map((i) => i.value));
  assert.deepEqual(await tags(), ['new']);
  await page.type('#rx-labels', 'sale');
  await page.press('Enter');
  await page.type('#rx-labels', 'gift');
  await page.press(',');
  await page.type('#rx-labels', 'NEW');
  await page.press('Enter');
  assert.deepEqual(await tags(), ['new', 'sale', 'gift']);
  await page.press('Backspace');
  assert.deepEqual(await tags(), ['new', 'sale']);
}));

test('the searchable select filters, picks with the keyboard and keeps required', () => onWidgets(async (page) => {
  await page.waitFor(() => document.querySelector('#rx-size-search'));
  // Required still holds: the native select is only hidden.
  assert.equal(await page.eval(() => document.querySelector('#rx-size').checkValidity()), false);
  await page.type('#rx-size-search', 'lar');
  const shown = await page.waitFor(() => {
    const options = [...document.querySelectorAll('.rx-combobox__option')].filter((o) => !o.hidden && o.offsetParent);
    return options.length ? options.map((o) => o.textContent.trim()) : null;
  });
  assert.deepEqual(shown, ['Large', 'Extra large']);
  // The first match is highlighted: Enter picks it; ArrowDown moves on.
  await page.press('ArrowDown');
  await page.press('ArrowUp');
  await page.press('Enter');
  await page.waitFor(() => document.querySelector('#rx-size').value !== '');
  assert.equal(await page.eval(() => document.querySelector('#rx-size').value), 'l');
  assert.equal(await page.eval(() => document.querySelector('#rx-size').checkValidity()), true);
}));

test('the date picker opens beside its field and picks a day with the keyboard', () => onWidgets(async (page) => {
  await page.click('[popovertarget="rx-on-calendar"]');
  await page.waitFor(() => document.querySelector('#rx-on-calendar').matches(':popover-open'));
  const inView = await page.eval(() => {
    const r = document.querySelector('#rx-on-calendar').getBoundingClientRect();
    return r.top >= 0 && r.left >= 0 && r.bottom <= innerHeight + 1 && r.right <= innerWidth + 1;
  });
  assert.ok(inView, 'the calendar stays inside the window');
  // The calendar is focused on the field's day; one day on, then pick it.
  await page.waitFor(() => !!document.querySelector('calendar-date'));
  await sleep(200);
  await page.eval(() => {
    const cal = document.querySelector('calendar-date');
    cal.focus();
  });
  await page.press('ArrowRight');
  await page.press('Enter');
  await page.waitFor(() => document.querySelector('#rx-on').value !== '2026-10-02', { timeout: 5000 });
  assert.equal(await page.eval(() => document.querySelector('#rx-on').value), '2026-10-03');
}));

test('repeater rows are added, renumbered after a removal, within their limits', () => onWidgets(async (page) => {
  const names = () =>
    page.eval(() => [...document.querySelectorAll('#rx-lines [data-rx-row] input')].map((i) => i.name));
  await page.click('#rx-lines [data-rx-row-add]');
  await page.click('#rx-lines [data-rx-row-add]');
  assert.equal((await names()).length, 6, 'three rows of two fields');
  // At the maximum (3), adding is off.
  assert.ok(await page.eval(() => document.querySelector('#rx-lines [data-rx-row-add]').disabled));
  // Remove the first: the others are renumbered from 0.
  await page.click('#rx-lines [data-rx-row] [data-rx-row-remove]');
  assert.deepEqual(await names(), ['lines[0][name]', 'lines[0][qty]', 'lines[1][name]', 'lines[1][qty]']);
  // A 422 on the second row lands on that row's field.
  await page.type('[name="lines[1][name]"]', 'Tea');
  await page.click('#order button[type=submit]');
  await page.waitFor(() => document.querySelector('[name="lines[1][qty]"]').getAttribute('aria-invalid') === 'true', {
    message: 'the second row marked invalid',
  });
}));

test('the wizard checks a step before the next and lands on the step with an error', () => onWidgets(async (page) => {
  const step = () => page.eval(() => document.querySelector('#w').getAttribute('data-rx-step-index'));
  assert.equal(await step(), '0');
  // Next with the step's required field empty stays on the step.
  await page.click('#w [data-rx-wizard-next]');
  await sleep(200);
  assert.equal(await step(), '0');
  await page.type('#rx-from', 'Bandung');
  // Enter moves to the next step instead of sending the form.
  await page.press('Enter');
  await page.waitFor(() => document.querySelector('#w').getAttribute('data-rx-step-index') === '1');
  assert.equal(await page.text('#trip-result'), '');
  await page.type('#rx-to', 'Jakarta');
  await page.click('#w [data-rx-wizard-submit]');
  await page.waitFor(() => document.querySelector('#trip-result').textContent.includes('Bandung to Jakarta'));
}));

test('after Back, a regular form that was sent is ready again', () => browser.with(async (page) => {
  await page.send('Page.addScriptToEvaluateOnNewDocument', {
    source: "addEventListener('pageshow', (e) => { window.__persisted = e.persisted; });",
  });
  await page.goto(`${app.url}/widgets`);
  await page.eval(() => { window.__first = true; });
  // The form is sent as far as renox-ui.js is concerned (it marks the form
  // sending and its button busy), but the browser never submits it: a
  // listener on window runs after renox-ui.js's on document and cancels the
  // submission. So nothing races the navigation below (a real request did on
  // slow CI runners, and holding it with CDP's Fetch stalled the navigation).
  // Then the page is left: Chrome keeps a page in its back/forward cache only
  // after a navigation it started itself, not after the form's own.
  await page.eval(() => window.addEventListener('submit', (event) => event.preventDefault()));
  const busy = await page.eval(() => {
    const button = document.querySelector('#plain-busy-button');
    button.click();
    return button.getAttribute('aria-busy');
  });
  assert.equal(busy, 'true');
  assert.equal(await page.eval(() => document.querySelector('#plain-busy').hasAttribute('data-rx-sending')), true);
  await page.goto(`${app.url}/stock`);
  await page.eval(() => history.back());
  await page.waitFor(() => window.__first === true, { message: 'the first page shown again' });
  const state = await page.eval(() => ({
    persisted: window.__persisted,
    busy: document.querySelector('#plain-busy-button').getAttribute('aria-busy'),
    sending: document.querySelector('#plain-busy').hasAttribute('data-rx-sending'),
  }));
  assert.equal(state.persisted, true, 'from the back/forward cache');
  assert.equal(state.busy, null);
  assert.equal(state.sending, false);
}));

test('live validation: nothing for an untouched field, other errors stay, a fix clears', () => browser.with(async (page) => {
  await page.goto(`${app.url}/form`);
  const checked = [];
  page.on('Network.requestWillBeSent', (p) => {
    const header = Object.entries(p.request.headers).find(([k]) => k.toLowerCase() === 'x-renox-validate');
    if (header) checked.push(header[1]);
  });
  // Through the email field without typing: not checked.
  await page.focus('#rx-email');
  await page.focus('#rx-name');
  await sleep(300);
  assert.deepEqual(checked, []);
  await page.type('#rx-name', 'A name far too long');
  await page.focus('#rx-email');
  await page.waitFor(() => document.querySelector('#rx-name').getAttribute('aria-invalid') === 'true', { message: 'name checked' });
  await page.type('#rx-email', 'nope');
  await page.focus('#rx-name');
  await page.waitFor(() => document.querySelector('#rx-email').getAttribute('aria-invalid') === 'true', { message: 'email checked' });
  // The name's error is still there.
  assert.equal(await page.eval(() => document.querySelector('#rx-name').getAttribute('aria-invalid')), 'true');
  assert.deepEqual([...new Set(checked)].sort(), ['email', 'name']);
  // Typing a fix clears the error (after a short pause), without leaving.
  await page.type('#rx-name', 'Ana', { clear: true });
  await page.waitFor(() => !document.querySelector('#rx-name').hasAttribute('aria-invalid'), { message: 'the fix accepted' });
  assert.equal(await page.eval(() => document.querySelector('#rx-email').getAttribute('aria-invalid')), 'true');
}));

test('a copyable field copies its value and says so', () => onWidgets(async (page) => {
  // The clipboard itself is the browser's: record what is written to it.
  await page.eval(() => {
    window.__copied = null;
    navigator.clipboard.writeText = (text) => { window.__copied = text; return Promise.resolve(); };
  });
  await page.click('[data-rx-copy="rx-code"]');
  await page.waitFor(() => window.__copied === 'RX-42');
  await page.waitFor(() => document.querySelector('[data-rx-copy="rx-code"]').hasAttribute('data-rx-done'));
  assert.match(await page.text('[data-rx-copy="rx-code"] [aria-live]'), /copied/i);
}));

test('a file field lists the chosen files with their sizes, and clears', () => onWidgets(async (page) => {
  const dir = mkdtempSync(join(tmpdir(), 'renox-files-'));
  try {
    writeFileSync(join(dir, 'menu.txt'), 'hello');
    writeFileSync(join(dir, 'big.bin'), Buffer.alloc(3 * 1024));
    const { root } = await page.send('DOM.getDocument');
    const { nodeId } = await page.send('DOM.querySelector', { nodeId: root.nodeId, selector: '#rx-photos' });
    await page.send('DOM.setFileInputFiles', { nodeId, files: [join(dir, 'menu.txt'), join(dir, 'big.bin')] });
    const listed = await page.waitFor(() => {
      const items = [...document.querySelectorAll('[data-rx-file-list] .rx-file__item')];
      return items.length === 2 ? items.map((i) => [i.querySelector('.rx-file__name').textContent, i.querySelector('.rx-file__size').textContent]) : null;
    });
    assert.deepEqual(listed, [['menu.txt', '5 B'], ['big.bin', '3 KB']]);
    assert.equal(await page.eval(() => document.querySelector('#rx-photos').multiple), true);
    await page.eval(() => {
      const input = document.querySelector('#rx-photos');
      input.value = '';
      input.dispatchEvent(new Event('change', { bubbles: true }));
    });
    assert.equal(await page.eval(() => document.querySelectorAll('[data-rx-file-list] li').length), 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}));

test('the date picker names the month it shows and tells the field it changed', () => onWidgets(async (page) => {
  await page.eval(() => {
    window.__changes = 0;
    document.querySelector('#rx-on').addEventListener('change', () => window.__changes++);
  });
  await page.click('[popovertarget="rx-on-calendar"]');
  await page.waitFor(() => document.querySelector('#rx-on-calendar').matches(':popover-open'));
  const heading = () => page.eval(() => document.querySelector('#rx-on-calendar .rx-calendar__heading').textContent);
  assert.equal(await heading(), 'October 2026');
  await page.waitFor(() => !!document.querySelector('calendar-date'));
  await sleep(200);
  await page.eval(() => document.querySelector('calendar-date').focus());
  await page.press('PageDown');
  await page.waitFor(() => document.querySelector('#rx-on-calendar .rx-calendar__heading').textContent === 'November 2026', {
    message: 'the next month named',
  });
  await page.press('Enter');
  await page.waitFor(() => document.querySelector('#rx-on').value === '2026-11-02', { message: 'the day picked' });
  assert.equal(await page.eval(() => window.__changes), 1, 'one change event on the field');
  assert.equal(await page.eval(() => document.querySelector('#rx-on-calendar').matches(':popover-open')), false);
}));

// The visit picker: 7 and 8 October can't be chosen, nor any Sunday.
test('the date picker greys out disabled days and the arrow keys step over them', () => onWidgets(async (page) => {
  const cal = '#rx-visit-calendar calendar-date';
  const focused = () => page.eval((sel) => String(document.querySelector(sel).focusedDate), cal);
  await page.click('[popovertarget="rx-visit-calendar"]');
  await page.waitFor(() => document.querySelector('#rx-visit-calendar').matches(':popover-open'));
  await page.waitFor(() => typeof document.querySelector('#rx-visit-calendar calendar-date').isDateDisallowed === 'function');
  // Shown as disabled: the listed days and the Sundays of October 2026 (4, 11, 18, 25).
  await sleep(200);
  const disabled = await page.eval((sel) => {
    const month = document.querySelector(sel).querySelector('calendar-month');
    return [...month.shadowRoot.querySelectorAll('button[aria-disabled="true"]')]
      .filter((b) => !(b.getAttribute('part') || '').includes('outside'))
      .map((b) => b.getAttribute('aria-label'));
  }, cal);
  assert.deepEqual(disabled, ['October 4', 'October 7', 'October 8', 'October 11', 'October 18', 'October 25']);
  // From Tuesday the 6th, one step right skips the 7th and the 8th.
  await page.eval((sel) => document.querySelector(sel).focus(), cal);
  await sleep(100);
  await page.press('ArrowRight');
  await page.waitFor(() => String(document.querySelector('#rx-visit-calendar calendar-date').focusedDate) === '2026-10-09', {
    message: 'the 9th focused',
  });
  // Saturday the 10th, then Sunday the 11th is skipped to Monday the 12th.
  await page.press('ArrowRight');
  await page.press('ArrowRight');
  await page.waitFor(() => String(document.querySelector('#rx-visit-calendar calendar-date').focusedDate) === '2026-10-12', {
    message: 'Sunday skipped',
  });
  // Back a week: Monday the 5th is open.
  await page.press('ArrowUp');
  await page.waitFor(() => String(document.querySelector('#rx-visit-calendar calendar-date').focusedDate) === '2026-10-05');
  await page.press('Enter');
  await page.waitFor(() => document.querySelector('#rx-visit').value === '2026-10-05', { message: 'the 5th picked' });
  assert.equal(await focused(), '2026-10-05');
}));

test('a disabled day typed into the date picker is refused with the error under the field', () => onWidgets(async (page) => {
  await page.type('#rx-visit', '2026-10-11', { clear: true });
  assert.equal(await page.eval(() => document.querySelector('#rx-visit').getAttribute('aria-invalid')), 'true');
  assert.equal(await page.eval(() => document.querySelector('#rx-visit').validity.customError), true);
  assert.match(await page.text('#rx-visit-error'), /That day can't be chosen/);
  assert.equal(await page.eval(() => document.querySelector('#fields').checkValidity()), false, 'the form is not sent');
  // An open day clears it.
  await page.type('#rx-visit', '2026-10-12', { clear: true });
  assert.equal(await page.eval(() => document.querySelector('#rx-visit').hasAttribute('aria-invalid')), false);
  assert.equal(await page.eval(() => document.querySelector('#rx-visit').validity.customError), false);
  assert.equal(await page.text('#rx-visit-error'), '');
}));

test('a hidden show_when group is left out of what the form sends', () => onWidgets(async (page) => {
  await page.click('#ship-send');
  await page.waitFor(() => document.querySelector('#ship-result').textContent !== '');
  assert.equal(await page.text('#ship-result'), 'via=pickup');
  await page.click('label[for="rx-via-2"]');
  await page.waitFor(() => !document.querySelector('[data-rx-show-when="via"]').disabled);
  await page.type('#ship-street', 'Main St');
  await page.click('#ship-send');
  await page.waitFor(() => document.querySelector('#ship-result').textContent.includes('street'));
  assert.equal(await page.text('#ship-result'), 'via=courier&street=Main St');
}));

test('a repeater keeps its minimum and removes one row from the middle', () => onWidgets(async (page) => {
  const rows = () =>
    page.eval(() => [...document.querySelectorAll('#rx-lines [data-rx-row]')].map((row) => {
      const name = row.querySelector('input');
      return [name.name, name.value];
    }));
  // One row, the minimum: it can't be removed.
  assert.equal(await page.eval(() => document.querySelector('#rx-lines [data-rx-row-remove]').disabled), true);
  await page.click('#rx-lines [data-rx-row-add]');
  await page.type('[name="lines[1][name]"]', 'Tea');
  await page.click('#rx-lines [data-rx-row-add]');
  await page.type('[name="lines[2][name]"]', 'Cocoa');
  await page.click('#rx-lines [data-rx-row]:nth-of-type(2) [data-rx-row-remove]');
  assert.deepEqual(await rows(), [['lines[0][name]', 'Coffee'], ['lines[1][name]', 'Cocoa']]);
}));

test('a searchable select gets its options from the server, adds one and renames it', () => onWidgets(async (page) => {
  await page.waitFor(() => document.querySelector('#rx-category-search'));
  await page.type('#rx-category-search', 'co');
  const found = await page.waitFor(() => {
    const options = [...document.querySelectorAll('#rx-category-listbox .rx-combobox__option')].filter((o) => !o.hidden);
    const labels = options.map((o) => o.textContent.trim());
    return labels.includes('Cocoa') && !labels.includes('Tea') ? labels : null;
  }, { message: 'the server’s matches' });
  // Opening asked for everything; typing asks the server for the matches.
  assert.deepEqual(found, ['Coffee', 'Cocoa', 'Add “co”']);
  await page.eval(() => [...document.querySelectorAll('#rx-category-listbox .rx-combobox__option')].find((o) => o.textContent.trim() === 'Cocoa').click());
  await page.waitFor(() => document.querySelector('#rx-category').value === '3', { message: 'Cocoa chosen' });

  // Something new: "Add …" posts it and chooses it.
  await page.type('#rx-category-search', 'Juice', { clear: true });
  await page.waitFor(() => document.querySelector('.rx-combobox__option--create'), { message: 'the add option' });
  await page.eval(() => document.querySelector('.rx-combobox__option--create').click());
  const added = await page.waitFor(() => {
    const select = document.querySelector('#rx-category');
    const chosen = select.options[select.selectedIndex];
    return chosen && chosen.textContent === 'Juice' ? chosen.value : null;
  }, { message: 'Juice added and chosen' });
  assert.ok(Number(added) > 3, added);

  // Renamed with the pencil: Enter saves (a PUT), the option follows.
  await page.click('#remote .rx-combobox__edit');
  await page.type('#rx-category-search', 'Juice & Smoothies', { clear: true });
  await page.press('Enter');
  await page.waitFor(() => {
    const select = document.querySelector('#rx-category');
    return select.options[select.selectedIndex]?.textContent === 'Juice & Smoothies';
  }, { message: 'renamed' });
  const listed = await page.eval(async () => (await (await fetch('/categories?q=smooth')).json()).map((o) => o.label));
  assert.deepEqual(listed, ['Juice & Smoothies']);
}));

test('a required searchable select stops the form until something is picked', () => onWidgets(async (page) => {
  const sent = [];
  page.on('Network.requestWillBeSent', (p) => p.request.url.endsWith('/picked') && sent.push(p.request.url));
  await page.click('#pick-send');
  await sleep(300);
  assert.equal(sent.length, 0, 'not sent');
  assert.equal(await page.focused(), 'rx-flavour-search', 'the search box is focused');
  await page.type('#rx-flavour-search', 'mi');
  await page.press('Enter');
  await page.waitFor(() => document.querySelector('#rx-flavour').value === 'mint');
  await page.click('#pick-send');
  await page.waitFor(() => document.querySelector('#pick-result').textContent === 'picked');
}, ));

test('a wizard in a sheet goes back to the step whose field the server refused', () => browser.with(async (page) => {
  await page.goto(`${app.url}/overlays`);
  await page.click('[data-rx-open="wizard-sheet"]');
  await page.waitFor(() => document.querySelector('#wizard-sheet').open);
  const step = () => page.eval(() => document.querySelector('#ws').getAttribute('data-rx-step-index'));
  // The first step has nothing the browser requires: Next goes on.
  await page.click('#ws [data-rx-wizard-next]');
  await page.waitFor(() => document.querySelector('#ws').getAttribute('data-rx-step-index') === '1');
  await page.type('#ws-to', 'Jakarta');
  await page.click('#ws [data-rx-wizard-submit]');
  // The server requires `from`: back on the first step, focused there.
  await page.waitFor(() => document.querySelector('#ws').getAttribute('data-rx-step-index') === '0', {
    message: 'back on the first step',
  });
  assert.equal(await step(), '0');
  assert.equal(await page.eval(() => document.querySelector('#ws-from').getAttribute('aria-invalid')), 'true');
  assert.equal(await page.focused(), 'ws-from');
  assert.ok(await page.eval(() => document.querySelector('#wizard-sheet').open), 'still in the sheet');
  page.assertClean({ allow: [/422/] });
}));

test('a button disabled with a reason is reached by keyboard, says why and does nothing', () => browser.with(async (page) => {
  await page.goto(`${app.url}/overlays`);
  await page.focus('#toast-refresh');
  await page.press('Tab');
  assert.equal(await page.focused(), 'publish');
  const tip = await page.waitFor(() => {
    const el = document.querySelector('[role=tooltip]');
    return el && !el.hidden ? el.textContent : null;
  }, { message: 'the reason shown' });
  assert.match(tip, /Add a title first/);
  await page.click('#publish');
  // Its shortcut (j) is ignored too.
  await page.eval(() => document.activeElement.blur());
  await page.press('j');
  assert.equal(await page.eval(() => document.querySelector('#publish').dataset.pressed), undefined);
  assert.equal(await page.eval(() => document.querySelector('#publish').getAttribute('aria-disabled')), 'true');
}));

// #349: charts, the wizard, the repeater and tags are modules renox-ui.js
// loads when it finds them, here in what htmx swaps in, twice: each module
// once, its listeners bound once.
test('parts swapped in by htmx load once and work', () => browser.with(async (page) => {
  await page.goto(`${app.url}/parts`);
  const modules = () => page.eval(() => [...document.querySelectorAll('script[type=module][src*="/_renox/ui-"]')]
    .map((s) => s.getAttribute('src').replace(/^\/_renox\/ui-([a-z]+)-[0-9a-f]+\.js$/, '$1')).sort());
  assert.deepEqual(await modules(), [], 'a page without them loads none');
  for (let i = 0; i < 2; i++) {
    await page.eval(() => { document.querySelector('#more-box').textContent = ''; });
    await page.click('#more');
    await page.waitFor(() => document.querySelector('#more-box [data-rx-wizard][data-rx-ready]'), { message: 'the wizard set up' });
    await page.settle();
  }
  assert.deepEqual(await modules(), ['chart', 'repeater', 'tags', 'wizard']);

  // Tags inside the wizard's first step: Enter makes a tag, it doesn't go on.
  await page.type('#rx-labels', 'sale');
  await page.press('Enter');
  const state = () => page.eval(() => ({
    tags: [...document.querySelectorAll('[data-rx-tags="labels"] .rx-tag input[type=hidden]')].map((el) => el.value),
    step: document.querySelector('[data-rx-wizard]').getAttribute('data-rx-step-index'),
    rows: document.querySelectorAll('[data-rx-repeater] [data-rx-row]').length,
  }));
  assert.deepEqual(await state(), { tags: ['sale'], step: '0', rows: 1 });
  await page.click('[data-rx-wizard-next]');
  await page.waitFor(() => document.querySelector('[data-rx-wizard]').getAttribute('data-rx-step-index') === '1', { message: 'the next step' });
  // One click, one row (a listener bound twice would add two).
  await page.click('[data-rx-row-add]');
  assert.deepEqual(await state(), { tags: ['sale'], step: '1', rows: 2 });

  // The chart answers the keyboard.
  await page.eval(() => document.querySelector('#parts-chart .rx-chart__plot').focus());
  assert.equal(await page.eval(() => document.querySelector('#parts-chart .rx-chart__tip').hidden), false);
  page.assertClean();
}));
