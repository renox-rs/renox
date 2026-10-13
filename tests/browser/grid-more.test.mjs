// #267, the rest of renox-grid.js on the fixture's /grid pages: phone and desktop
// columns, frozen columns while scrolling, a URL without defaults, pins from
// the menu, a heading dragged, a width reset, the date range calendar,
// groups, copy buttons, the advanced filter, polling, exports, two grids on
// one page; then, logged in: preferences kept in the database, cells and
// rows edited in place, rows put in order, selection with bulk and row
// actions behind the grid's own confirmation dialog.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
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

// `remember()`: the grid keeps its filters in the session, so each test
// starts from a cleared state (`state=1` alone).
const onGrid = (fn, options) =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/grid?state=1`);
    await fn(page);
    page.assertClean({ allow: [/422/] });
  }, options);

const visible = (page, selector) =>
  page.eval(
    (s) => [...document.querySelectorAll(s)].filter((el) => getComputedStyle(el).display !== 'none').map((el) => el.dataset.col),
    selector,
  );

/** Waits for the grid to be redrawn after the request `act` starts. */
async function redraw(page, act) {
  await page.eval(() => {
    document.querySelector('form.rx-grid')._probe = true;
  });
  await act();
  await page.waitFor(() => !document.querySelector('form.rx-grid')?._probe, { message: 'the grid redrawn' });
  await page.settle();
}

const params = (page) => page.eval(() => [...new URLSearchParams(location.search).keys()].sort());

describe('as a guest', () => {
  test('the URL keeps only what differs from the defaults', () =>
    onGrid(async (page) => {
      await redraw(page, () => page.click('th[data-col="customer"] [data-grid-sort]'));
      // `state` marks a grid that remembers its filters; no `page=1`, empty
      // filters, `match=all` or the default rows per page.
      assert.deepEqual(await params(page), ['sort', 'state']);
      await redraw(page, () =>
        page.eval(() => {
          const select = document.querySelector('[data-grid-per-page]');
          select.value = '50';
          select.dispatchEvent(new Event('change', { bubbles: true }));
        }),
      );
      assert.deepEqual(await params(page), ['per_page', 'sort', 'state']);
    }));

  test('a desktop shows every column, a phone the phone columns as cards', async () => {
    await onGrid(async (page) => {
      const shown = await visible(page, 'thead th[data-col]');
      for (const key of ['number', 'customer', 'city', 'region', 'ordered_on', 'total']) {
        assert.ok(shown.includes(key), `${key} on a desktop: ${shown}`);
      }
    });
    await onGrid(
      async (page) => {
        const card = await page.eval(() => {
          const row = document.querySelector('tbody tr[data-id]');
          return {
            cards: document.querySelector('form.rx-grid').classList.contains('rx-grid--cards'),
            display: getComputedStyle(row).display,
            city: document.querySelector('tbody tr[data-id] td[data-col="city"]').classList.contains('rx-grid__off'),
            number: document.querySelector('tbody tr[data-id] td[data-col="number"]').classList.contains('rx-grid__off'),
          };
        });
        assert.ok(card.cards);
        assert.notEqual(card.display, 'table-row', 'a row is a card');
        assert.ok(card.city, 'a desktop column is off');
        assert.ok(!card.number, 'a phone column is on');
      },
      { width: 390, height: 844 },
    );
  });

  test('frozen columns stay put while the rows scroll sideways', () =>
    onGrid(async (page) => {
      const where = () =>
        page.eval(() => {
          const scroll = document.querySelector('.rx-grid__scroll');
          const left = document.querySelector('tbody tr[data-id] td[data-col="number"]').getBoundingClientRect().left;
          const right = document.querySelector('tbody tr[data-id] td[data-col="actions"]').getBoundingClientRect().right;
          return { left, right, edge: scroll.getBoundingClientRect().right, max: scroll.scrollWidth - scroll.clientWidth };
        });
      const before = await where();
      assert.ok(before.max > 100, 'the table is wider than the window');
      await page.eval(() => {
        const scroll = document.querySelector('.rx-grid__scroll');
        scroll.scrollLeft = 400;
        scroll.dispatchEvent(new Event('scroll'));
      });
      await sleep(100);
      const after = await where();
      assert.ok(Math.abs(after.left - before.left) <= 1, `left column moved: ${before.left} → ${after.left}`);
      assert.ok(Math.abs(after.right - before.right) <= 1, `right column moved: ${before.right} → ${after.right}`);
      const edges = await page.eval(() => ({
        scrolled: document.querySelector('form.rx-grid').classList.contains('rx-grid--scrolled-left'),
        left: document.querySelector('tbody tr[data-id] td[data-col="number"]').classList.contains('rx-grid__edge-left'),
        right: document.querySelector('tbody tr[data-id] td[data-col="actions"]').classList.contains('rx-grid__edge-right'),
      }));
      assert.deepEqual(edges, { scrolled: true, left: true, right: true }, 'the edges get their shadows');
    }));

  test('the menu pins a column to the right edge, and reset unpins it', () =>
    onGrid(async (page) => {
      await page.click('[popovertarget="grid-orders-columns"]');
      await redraw(page, () =>
        page.eval(() => {
          const pick = document.querySelector('[data-grid-pin="city"]');
          pick.value = 'right';
          pick.dispatchEvent(new Event('change', { bubbles: true }));
        }),
      );
      const pinned = await page.eval(() => ({
        pin: document.querySelector('thead th[data-col="city"]').dataset.pin,
        offset: document.querySelector('tbody tr[data-id] td[data-col="city"]').style.right,
      }));
      assert.equal(pinned.pin, 'right');
      assert.match(pinned.offset, /px$/);
      // And one to the left edge.
      await page.waitFor(() => document.querySelector('#grid-orders-columns').matches(':popover-open'));
      await redraw(page, () =>
        page.eval(() => {
          const pick = document.querySelector('[data-grid-pin="customer"]');
          pick.value = 'left';
          pick.dispatchEvent(new Event('change', { bubbles: true }));
        }),
      );
      const left = await page.eval(() => ({
        pin: document.querySelector('thead th[data-col="customer"]').dataset.pin,
        offset: document.querySelector('tbody tr[data-id] td[data-col="customer"]').style.left,
      }));
      assert.equal(left.pin, 'left');
      assert.match(left.offset, /px$/);
      // The menu opens again after the reload; reset brings the defaults back.
      await page.waitFor(() => document.querySelector('#grid-orders-columns').matches(':popover-open'));
      await redraw(page, () => page.click('[data-grid-reset]'));
      assert.equal(await page.eval(() => document.querySelector('thead th[data-col="city"]').dataset.pin), undefined);
    }));

  test('a heading dragged onto another moves its column; a double click resets a width', () =>
    onGrid(async (page) => {
      const order = () => visible(page, 'tbody tr[data-id]:first-of-type td[data-col]');
      const first = await order();
      assert.ok(first.indexOf('city') > first.indexOf('customer'));
      // Both headings measured at once, without scrolling in between.
      const [from, to] = await page.eval(() =>
        ['city', 'customer'].map((k) => {
          const r = document.querySelector(`thead th[data-col="${k}"]`).getBoundingClientRect();
          return { x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width };
        }),
      );
      const drop = to.x - to.w / 4; // the left half of "Name": before it
      await redraw(page, async () => {
        await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: from.x, y: from.y });
        await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: from.x, y: from.y, button: 'left', clickCount: 1 });
        for (let i = 1; i <= 10; i++) {
          await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: from.x + ((drop - from.x) * i) / 10, y: to.y, button: 'left' });
        }
        await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: drop, y: to.y, button: 'left', clickCount: 1 });
      });
      const moved = await order();
      assert.equal(moved.indexOf('city'), moved.indexOf('customer') - 1, JSON.stringify(moved));
      assert.equal(new URLSearchParams(await page.eval(() => location.search)).get('sort'), null, 'the drag did not sort');

      // Widen a column by its edge, then a double click on the edge resets it.
      const width = () => page.eval(() => document.querySelector('thead th[data-col="region"]').style.width);
      await page.focus('thead th[data-col="region"] [data-grid-resize]');
      await page.press('ArrowRight', { shift: true });
      const widened = await width();
      assert.match(widened, /px$/);
      // Both are kept: after a reload the column is where it was dropped,
      // and as wide as it was made.
      await sleep(800); // the width is saved a moment after the last change
      await page.goto(`${app.url}/grid?state=1`);
      const reloaded = await order();
      assert.equal(reloaded.indexOf('city'), reloaded.indexOf('customer') - 1, JSON.stringify(reloaded));
      assert.equal(await width(), widened, 'the width came back');
      await page.eval(() =>
        document
          .querySelector('thead th[data-col="region"] [data-grid-resize]')
          .dispatchEvent(new MouseEvent('dblclick', { bubbles: true })),
      );
      assert.equal(await width(), '');
      // Back to the defaults for the next tests.
      await page.click('[popovertarget="grid-orders-columns"]');
      await redraw(page, () => page.click('[data-grid-reset]'));
    }));

  test('a date range picked on the calendar filters and shows as a chip', () =>
    onGrid(async (page) => {
      await page.hover('thead th[data-col="ordered_on"]');
      await page.click('thead th[data-col="ordered_on"] [popovertarget="grid-orders-f-ordered_on"]');
      await page.waitFor(() => document.querySelector('#grid-orders-f-ordered_on').matches(':popover-open'));
      // Two days clicked with the mouse: the start and the end.
      for (const n of [2, 9]) {
        const { x, y } = await page.eval((i) => {
          const days = document.querySelector('#grid-orders-f-ordered_on calendar-month').shadowRoot.querySelectorAll('button[part~="day"]:not([disabled])');
          const r = days[i].getBoundingClientRect();
          return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
        }, n);
        await page.clickAt(x, y);
      }
      const picked = await page.waitFor(() => {
        const pop = document.querySelector('#grid-orders-f-ordered_on');
        const from = pop.querySelector('[data-grid-date="from"]').value;
        const to = pop.querySelector('[data-grid-date="to"]').value;
        return from && to ? { from, to } : null;
      }, { message: 'both dates filled from the calendar' });
      assert.ok(picked.from < picked.to, JSON.stringify(picked));
      await redraw(page, () => page.click('#grid-orders-f-ordered_on button[type=submit]'));
      const query = new URLSearchParams(await page.eval(() => location.search));
      assert.equal(query.get('from.ordered_on'), picked.from);
      assert.equal(query.get('to.ordered_on'), picked.to);
      assert.match(await page.eval(() => [...document.querySelectorAll('.rx-grid__chip')].map((c) => c.textContent).join('|')), /Ordered/);
    }));

  test('search as you type sends exactly one request once the typing stops', () =>
    onGrid(async (page) => {
      const searches = [];
      page.on('Network.requestWillBeSent', (p) => {
        if (p.request.headers['HX-Request'] && new URL(p.request.url).searchParams.has('search')) searches.push(p.request.url);
      });
      await redraw(page, () => page.type('[data-grid-search]', 'customer 1'));
      await sleep(500);
      assert.equal(searches.length, 1, searches.join('\n'));
      assert.equal(new URL(searches[0]).searchParams.get('search'), 'customer 1');
    }));

  test('sorting and filtering emit events on the document; paging emits neither', () =>
    onGrid(async (page) => {
      await page.eval(() => {
        window._rxEvents = [];
        for (const name of ['sorted', 'filtered', 'selected'])
          document.addEventListener('rx:grid:' + name, (e) => window._rxEvents.push({ name, detail: e.detail }));
      });
      const events = () => page.eval(() => window._rxEvents);
      await redraw(page, () => page.eval(() => document.querySelector('th [data-grid-sort]').click()));
      const sorted = (await events()).filter((e) => e.name === 'sorted');
      assert.equal(sorted.length, 1, JSON.stringify(await events()));
      assert.ok(sorted[0].detail.sort, JSON.stringify(sorted[0]));
      await redraw(page, () => page.type('[data-grid-search]', 'customer 1'));
      await sleep(500);
      const filtered = (await events()).filter((e) => e.name === 'filtered');
      assert.equal(filtered.length, 1, JSON.stringify(await events()));
      assert.match(filtered[0].detail.query, /search=customer\+1/);
      await page.goto(`${app.url}/grid?state=1`);
      await page.eval(() => {
        window._rxEvents = [];
        for (const name of ['sorted', 'filtered'])
          document.addEventListener('rx:grid:' + name, (e) => window._rxEvents.push({ name, detail: e.detail }));
      });
      await redraw(page, () => page.eval(() => document.querySelector('[data-grid-page]:not([disabled])').click()));
      assert.deepEqual(await events(), [], 'paging emits neither');
    }));

  test('groups fold and unfold', () =>
    onGrid(async (page) => {
      await redraw(page, () =>
        page.eval(() => {
          const pick = document.querySelector('select[name="group"]');
          pick.value = 'region';
          pick.dispatchEvent(new Event('change', { bubbles: true }));
        }),
      );
      const group = await page.eval(() => document.querySelector('tr[data-grid-group]').getAttribute('data-grid-group'));
      const hidden = (g) =>
        page.eval((id) => [...document.querySelectorAll(`[data-in-group="${id}"]`)].every((row) => row.hidden), g);
      assert.equal(await hidden(group), false);
      await page.click(`tr[data-grid-group="${group}"] [data-grid-fold]`);
      assert.equal(await hidden(group), true, 'folded');
      assert.equal(await page.eval((g) => document.querySelector(`tr[data-grid-group="${g}"] [data-grid-fold]`).getAttribute('aria-expanded'), group), 'false');
      await page.click(`tr[data-grid-group="${group}"] [data-grid-fold]`);
      assert.equal(await hidden(group), false, 'unfolded');
    }));

  test('a copy button copies its cell', () =>
    onGrid(async (page) => {
      await page.eval(() => {
        window._copied = [];
        Object.defineProperty(navigator, 'clipboard', {
          value: { writeText: (text) => (window._copied.push(text), Promise.resolve()) },
          configurable: true,
        });
      });
      const number = await page.eval(() => document.querySelector('tbody tr[data-id] td[data-col="number"] [data-grid-copy]').getAttribute('data-grid-copy'));
      await page.click('tbody tr[data-id] td[data-col="number"] [data-grid-copy]');
      await page.waitFor(() => document.querySelector('.rx-grid__copy--done'), { message: 'the button says it copied' });
      assert.deepEqual(await page.eval(() => window._copied), [number]);
    }));

  test('the advanced filter adds, changes and removes rules; the URL keeps them', () =>
    onGrid(async (page) => {
      await page.click('[popovertarget="grid-orders-advanced"]');
      await page.click('[data-grid-add-rule]');
      await page.click('[data-grid-add-rule]');
      assert.equal(await page.eval(() => document.querySelectorAll('[data-grid-rules] [data-grid-rule]').length), 2);
      // The first rule on Status: its operators and an options list for the value.
      const value = await page.eval(() => {
        const rule = document.querySelector('[data-grid-rules] [data-grid-rule]');
        const column = rule.querySelector('[data-grid-rule-column]');
        column.value = 'status';
        column.dispatchEvent(new Event('change', { bubbles: true }));
        const op = rule.querySelector('[data-grid-rule-op]');
        op.value = 'is';
        op.dispatchEvent(new Event('change', { bubbles: true }));
        const v = rule.querySelector('[data-grid-rule-value]');
        v.value = 'paid';
        return { tag: v.tagName, ops: [...op.options].map((o) => o.value) };
      });
      assert.equal(value.tag, 'SELECT');
      assert.ok(value.ops.includes('is_not') && !value.ops.includes('gt'), JSON.stringify(value.ops));
      // The second rule removed; "any" instead of "all".
      await page.click('[data-grid-rules] [data-grid-rule]:nth-of-type(2) [data-grid-remove-rule]');
      await page.eval(() => {
        document.querySelector('select[name="match"]').value = 'any';
      });
      await redraw(page, () => page.click('#grid-orders-advanced button[type=submit]'));
      const query = new URLSearchParams(await page.eval(() => location.search));
      assert.equal(query.get('r.0.c'), 'status');
      assert.equal(query.get('r.0.o'), 'is');
      assert.equal(query.get('r.0.v'), 'paid');
      assert.equal(query.get('match'), 'any');
      assert.equal(query.get('r.1.c'), null);
      const statuses = await page.eval(() => [...document.querySelectorAll('tbody td[data-col="status"]')].map((td) => td.textContent.trim()));
      assert.ok(statuses.length && statuses.every((s) => s === 'Paid'), JSON.stringify(statuses));
      assert.match(await page.eval(() => document.querySelector('.rx-grid__chips').textContent), /1 rule/);
      // The chip's ✕ clears them.
      await redraw(page, () => page.click('.rx-grid__chip [data-grid-clear-rules]'));
      assert.equal(new URLSearchParams(await page.eval(() => location.search)).get('r.0.c'), null);
    }));

  test('polling refreshes the table, but not while a menu is open', () =>
    browser.with(async (page) => {
      // Thirty seconds becomes a third of one.
      await page.send('Page.addScriptToEvaluateOnNewDocument', {
        source: `(() => { const set = window.setInterval; window.setInterval = (fn, ms, ...a) => set(fn, ms >= 30000 ? ms / 100 : ms, ...a); })();`,
      });
      const polls = [];
      page.on('Network.requestWillBeSent', (p) => {
        if (p.request.headers['HX-Request'] && p.request.method === 'GET') polls.push(Date.now());
      });
      await page.goto(`${app.url}/grid?state=1`);
      await page.waitFor(() => true);
      const start = polls.length;
      await sleep(1500);
      assert.ok(polls.length - start >= 2, `${polls.length - start} refreshes in 1.5 s`);
      await page.settle();
      await page.click('[popovertarget="grid-orders-columns"]');
      await page.waitFor(() => document.querySelector('#grid-orders-columns').matches(':popover-open'));
      await sleep(200);
      const open = polls.length;
      await sleep(1200);
      assert.equal(polls.length, open, 'no refresh while the menu is open');
      page.assertClean();
    }));

  test('exports download from the menu: CSV, Excel and a print page', () =>
    onGrid(async (page) => {
      const dir = mkdtempSync(join(tmpdir(), 'renox-downloads-'));
      try {
        await page.send('Page.setDownloadBehavior', { behavior: 'allow', downloadPath: dir });
        const download = async (label) => {
          const done = page.once('Page.downloadProgress', (p) => p.state === 'completed', 20_000);
          if (!(await page.eval(() => document.querySelector('#grid-orders-export').matches(':popover-open')))) {
            await page.click('[popovertarget="grid-orders-export"]');
          }
          await page.waitFor(() => document.querySelector('#grid-orders-export').matches(':popover-open'));
          await page.eval((l) => [...document.querySelectorAll('#grid-orders-export a')].find((a) => a.textContent.trim() === l).click(), label);
          await done;
        };
        await download('CSV');
        await download('Excel');
        const files = readdirSync(dir).sort();
        const csv = files.find((f) => f.endsWith('.csv'));
        const xlsx = files.find((f) => f.endsWith('.xlsx'));
        assert.ok(csv && xlsx, JSON.stringify(files));
        const text = readFileSync(join(dir, csv), 'utf8').replace(/^﻿/, '');
        assert.match(text.split('\n')[0], /^Order,/);
        assert.equal(text.trim().split('\n').length, 481, 'every order, not one page');
        assert.equal(readFileSync(join(dir, xlsx)).subarray(0, 2).toString(), 'PK');
        // The print page opens in a tab of its own: its address answers a table.
        const print = await page.eval(() => [...document.querySelectorAll('#grid-orders-export a')].find((a) => a.target === '_blank').href);
        const res = await fetch(print);
        assert.equal(res.status, 200);
        assert.match(await res.text(), /<table/);
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    }));

  test('two grids on one page page and sort apart', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/grid/follow-up`);
      const current = (id) => page.eval((g) => document.querySelector(`#${g} [data-grid-page][aria-current="page"]`).textContent.trim(), id);
      const first = await page.eval(() => document.querySelector('#grid-largest tbody tr[data-id]').getAttribute('data-id'));
      await page.click('#grid-unpaid [data-grid-page="2"]');
      await page.waitFor(() => new URLSearchParams(location.search).get('unpaid.page') === '2');
      await page.settle();
      assert.equal(await current('grid-unpaid'), '2');
      assert.equal(await current('grid-largest'), '1');
      assert.equal(await page.eval(() => document.querySelector('#grid-largest tbody tr[data-id]').getAttribute('data-id')), first);
      // Sorting the second keeps the first one's page.
      await page.click('#grid-largest th[data-col="number"] [data-grid-sort]');
      await page.waitFor(() => new URLSearchParams(location.search).get('largest.sort') === 'number');
      await page.settle();
      assert.equal(new URLSearchParams(await page.eval(() => location.search)).get('unpaid.page'), '2');
      assert.equal(await current('grid-unpaid'), '2');
      page.assertClean();
    }));
});

describe('logged in', () => {
  before(() =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/login`);
      await page.type('#rx-email', 'demo@example.com');
      await page.type('#rx-password', 'password');
      const loaded = page.once('Page.loadEventFired');
      await page.click('form button[type=submit]');
      await loaded;
    }),
  );

  test('a hidden column is kept in the database: it outlives the session', () =>
    onGrid(async (page) => {
      await page.click('[popovertarget="grid-orders-columns"]');
      await page.click('[data-grid-toggle="city"]');
      await sleep(500); // saved in the background
      // A new session: logged out and in again.
      await page.eval(() =>
        fetch('/logout', {
          method: 'POST',
          headers: { 'X-CSRF-Token': document.querySelector('meta[name="csrf-token"]').content },
        }),
      );
      await page.goto(`${app.url}/login`);
      await page.type('#rx-email', 'demo@example.com');
      await page.type('#rx-password', 'password');
      const loaded = page.once('Page.loadEventFired');
      await page.click('form button[type=submit]');
      await loaded;
      await page.goto(`${app.url}/grid?state=1`);
      assert.ok(!(await visible(page, 'thead th[data-col]')).includes('city'), 'still hidden');
      await page.click('[popovertarget="grid-orders-columns"]');
      await redraw(page, () => page.click('[data-grid-reset]'));
      assert.ok((await visible(page, 'thead th[data-col]')).includes('city'));
    }));

  const cell = (key) => `tbody tr[data-id]:first-of-type td[data-col="${key}"]`;
  const editCell = (page, key) =>
    page.eval((s) => document.querySelector(s).dispatchEvent(new MouseEvent('dblclick', { bubbles: true })), cell(key));

  test('polling waits while a cell is being edited', () =>
    browser.with(async (page) => {
      // Thirty seconds becomes a third of one, as in the guest's test.
      await page.send('Page.addScriptToEvaluateOnNewDocument', {
        source: `(() => { const set = window.setInterval; window.setInterval = (fn, ms, ...a) => set(fn, ms >= 30000 ? ms / 100 : ms, ...a); })();`,
      });
      const polls = [];
      const urls = [];
      page.on('Network.requestWillBeSent', (p) => {
        if (p.request.headers['HX-Request'] && p.request.method === 'GET') {
          polls.push(Date.now());
          urls.push(p.request.url);
        }
      });
      await page.goto(`${app.url}/grid?state=1`);
      await page.settle();
      await editCell(page, 'customer');
      await page.waitFor((s) => !!document.querySelector(`${s} [data-grid-input]`), { message: 'a cell being edited' }, cell('customer'));
      await page.type(`${cell('customer')} [data-grid-input]`, 'Half typed', { clear: true });
      await sleep(200);
      const editing = polls.length;
      await sleep(1200);
      // A poll sent just before the edit began must not redraw the table
      // under it: what was typed is still there.
      const kept = await page.eval((s) => document.querySelector(`${s} [data-grid-input]`)?.value ?? null, cell('customer'));
      assert.equal(kept, 'Half typed', 'the edit survives the polling');
      assert.equal(polls.length, editing, `no refresh while a cell is edited: ${JSON.stringify(urls.slice(editing))}`);
      // Once the edit ends, polling goes on.
      await page.press('Escape');
      await sleep(1200);
      assert.ok(polls.length > editing, 'refreshes again after the edit');
      page.assertClean();
    }));

  test('a cell edits in place: Enter saves, Escape cancels, a 422 shows in the cell', () =>
    onGrid(async (page) => {
      const patches = [];
      page.on('Network.requestWillBeSent', (p) => {
        if (p.request.method === 'PATCH') patches.push(p.request.url);
      });
      const before = await page.text(`${cell('customer')} .rx-grid__val, ${cell('customer')}`);
      // Escape: the old content, nothing sent.
      await editCell(page, 'customer');
      await page.type(`${cell('customer')} [data-grid-input]`, 'Not this', { clear: true });
      await page.press('Escape');
      assert.equal(await page.eval((s) => !!document.querySelector(`${s} [data-grid-input]`), cell('customer')), false);
      assert.equal(patches.length, 0);
      // Enter: saved, the toast says so, and the page shows it.
      await editCell(page, 'customer');
      await page.type(`${cell('customer')} [data-grid-input]`, 'Grid Browser', { clear: true });
      await redraw(page, () => page.press('Enter'));
      assert.equal(patches.length, 1);
      assert.match(await page.text(cell('customer')), /Grid Browser/);
      assert.notEqual(await page.text(cell('customer')), before);
      await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('saved'));
      // A value the server refuses: the error stays in the cell.
      await editCell(page, 'items');
      await page.type(`${cell('items')} [data-grid-input]`, '0', { clear: true });
      await page.press('Enter');
      await page.waitFor((s) => document.querySelector(`${s} [data-grid-input]`)?.getAttribute('aria-invalid') === 'true', { message: 'the input marked invalid' }, cell('items'));
      assert.match(await page.text(`${cell('items')} .error`), /between 1 and 999/);
    }));

  test('a whole row edits in place and saves its cells together', () =>
    onGrid(async (page) => {
      const row = 'tbody tr[data-id]:nth-of-type(2)';
      await page.click(`${row} [data-grid-edit-row]`);
      await page.waitFor((r) => document.querySelector(r).classList.contains('rx-grid__row--editing'), {}, row);
      await page.type(`${row} td[data-col="customer"] [data-grid-input]`, 'Row Editor', { clear: true });
      await page.type(`${row} td[data-col="items"] [data-grid-input]`, '7', { clear: true });
      const id = await page.eval((r) => document.querySelector(r).dataset.id, row);
      await redraw(page, () => page.click(`${row} [data-grid-save-row]`));
      const saved = await page.eval((i) => {
        const tr = document.querySelector(`tbody tr[data-id="${i}"]`);
        return { customer: tr.querySelector('td[data-col="customer"]').textContent, items: tr.querySelector('td[data-col="items"]').textContent.trim() };
      }, id);
      assert.match(saved.customer, /Row Editor/);
      assert.equal(saved.items, '7');
    }));

  test('rows sorted by # are put in order by dragging and with the keyboard', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/grid?state=1&sort=position`);
      const ids = () => page.eval(() => [...document.querySelectorAll('tbody tr[data-id]')].slice(0, 3).map((tr) => tr.dataset.id));
      const saved = () => page.once('Network.responseReceived', (p) => p.response.url.endsWith('/grid/orders/reorder'));
      const [a, b] = await ids();
      // The first row's grip with the keyboard: one down.
      await page.focus('tbody tr[data-id]:first-of-type [data-grid-drag]');
      let done = saved();
      await page.press('ArrowDown');
      assert.equal((await done).response.status, 204);
      assert.deepEqual((await ids()).slice(0, 2), [b, a]);
      // Dragged back above the other.
      const grip = await page.point(`tbody tr[data-id="${a}"] [data-grid-drag]`);
      const top = await page.point(`tbody tr[data-id="${b}"]`);
      done = saved();
      await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: grip.x, y: grip.y });
      await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: grip.x, y: grip.y, button: 'left', clickCount: 1 });
      for (let y = grip.y; y >= top.y - top.h / 4; y -= 4) {
        await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: grip.x, y, button: 'left' });
      }
      await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: grip.x, y: top.y - top.h / 4, button: 'left', clickCount: 1 });
      await done;
      // Read back from the server.
      await page.goto(`${app.url}/grid?state=1&sort=position`);
      assert.deepEqual((await ids()).slice(0, 2), [a, b]);
      page.assertClean();
    }));

  test('bulk actions take the page or every match, behind the grid dialog', () =>
    onGrid(async (page) => {
      await page.click('[data-grid-select-all]');
      const count = () => page.text('[data-grid-bulk] [data-grid-count]');
      assert.equal(await count(), String(await page.eval(() => document.querySelectorAll('[data-grid-select]').length)));
      await page.click('[data-grid-select-matching]');
      const total = await page.eval(() => document.querySelector('[data-grid-select-matching]').getAttribute('data-total'));
      assert.equal(await count(), total);
      // A dangerous one asks first, in the grid's own dialog.
      const requests = [];
      page.on('Network.requestWillBeSent', (p) => {
        if (p.request.url.includes('/grid/orders/bulk/')) requests.push(p.request.url);
      });
      await page.eval(() => [...document.querySelectorAll('[data-grid-bulk-action]')].find((b) => b.textContent.trim() === 'Delete').click());
      await page.waitFor(() => document.querySelector('dialog[data-grid-dialog]').open);
      assert.match(await page.text('[data-grid-dialog-text]'), /Delete the selected orders/);
      await page.click('[data-grid-dialog-cancel]');
      await sleep(200);
      assert.deepEqual(requests, [], 'cancelled: nothing sent');
      assert.deepEqual(page.dialogs, [], 'no window.confirm');
      // Without a question: every matching order marked shipped.
      await redraw(page, () => page.eval(() => [...document.querySelectorAll('[data-grid-bulk-action]')].find((b) => b.textContent.includes('shipped')).click()));
      await page.waitFor((t) => document.querySelector('.rx-toast')?.textContent.includes(`${t} orders marked shipped`), {}, total);
    }));

  test('selecting rows emits selected with the ids; select all matching says all', () =>
    onGrid(async (page) => {
      await page.eval(() => {
        window._rxEvents = [];
        document.addEventListener('rx:grid:selected', (e) => window._rxEvents.push(e.detail));
      });
      const ids = await page.eval(() => [...document.querySelectorAll('tbody tr[data-id]')].slice(0, 2).map((r) => r.dataset.id));
      for (const id of ids) await page.click(`tbody tr[data-id="${id}"] [data-grid-select]`);
      const events = await page.eval(() => window._rxEvents);
      assert.deepEqual(events.at(-1), { ids, all: false });
      await page.click('[data-grid-select-all]');
      await page.click('[data-grid-select-matching]');
      const last = (await page.eval(() => window._rxEvents)).at(-1);
      assert.equal(last.all, true);
    }));

  test('an action that asks for input opens its sheet with the selection', () =>
    onGrid(async (page) => {
      const ids = await page.eval(() => [...document.querySelectorAll('tbody tr[data-id]')].slice(0, 2).map((r) => r.dataset.id));
      for (const id of ids) await page.click(`tbody tr[data-id="${id}"] [data-grid-select]`);
      const requests = [];
      page.on('Network.requestWillBeSent', (p) => {
        if (p.request.url.includes('/grid/orders/bulk/note')) requests.push(p.request);
      });
      await page.eval(() => [...document.querySelectorAll('[data-grid-bulk-action]')].find((b) => b.textContent.trim() === 'Add note').click());
      // The sheet, not the grid's dialog, and nothing sent yet.
      await page.waitFor(() => document.getElementById('orders-note').open);
      assert.equal(await page.eval(() => document.querySelector('dialog[data-grid-dialog]').open), false);
      assert.deepEqual(requests, []);
      assert.equal(await page.eval(() => document.querySelector('#orders-note input[name="ids"]').value), ids.join(','));
      assert.match(await page.eval(() => document.querySelector('#orders-note form').getAttribute('hx-post')), /\/grid\/orders\/bulk\/note/);
      // Blank: the server's error shows in the sheet, which stays open.
      await page.click('#orders-note button[type="submit"]');
      await page.waitFor(() => /required/i.test(document.querySelector('#orders-note').textContent));
      assert.equal(await page.eval(() => document.getElementById('orders-note').open), true);
      // Typed: it closes, the toast says what happened, the grid has the note.
      await page.type('#orders-note input[name="note"]', 'Checked by Ana');
      await page.click('#orders-note button[type="submit"]');
      await page.waitFor(() => !document.getElementById('orders-note').open);
      await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('2 orders noted: Checked by Ana'));
      const bodies = requests.map((r) => r.postData ?? '');
      assert.ok(bodies.some((b) => b.includes('note=Checked')), bodies.join('|'));
    }));

  test("a row's menu deletes it after the grid asks", () =>
    onGrid(async (page) => {
      const id = await page.eval(() => document.querySelector('tbody tr[data-id]').dataset.id);
      const number = await page.eval(() => document.querySelector('tbody tr[data-id] td[data-col="number"]').textContent.trim());
      await page.click('tbody tr[data-id]:first-of-type [popovertarget^="grid-orders-a-"]');
      await page.eval(() => [...document.querySelectorAll('[data-grid-action]')].find((b) => b.offsetParent && b.textContent.trim() === 'Delete').click());
      await page.waitFor(() => document.querySelector('dialog[data-grid-dialog]').open);
      assert.match(await page.text('[data-grid-dialog-text]'), /Delete this order\?/);
      await redraw(page, () => page.click('[data-grid-dialog-ok]'));
      await page.waitFor((n) => document.querySelector('.rx-toast')?.textContent.includes(`${n} deleted`), {}, number);
      assert.equal(await page.eval((i) => !!document.querySelector(`tbody tr[data-id="${i}"]`), id), false);
    }));
});
