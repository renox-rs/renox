// #266: the other half of renox-ui.js: toasts (from htmx, from the page's
// script, with request actions, waiting for the next page), sheets, menus,
// tabs, tooltips, keyboard shortcuts, charts, the period filter, the
// navigation on phones and the notification bell, as a keyboard, mouse or
// touch user meets them.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
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

const onOverlays = (fn, allow = []) =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/overlays`);
    await fn(page);
    page.assertClean({ allow });
  });

test('a toast from htmx shows, keeps only safe links, and goes after its time', () =>
  onOverlays(async (page) => {
    await page.click('#toast-me');
    await page.waitFor(() => document.querySelector('.rx-toast--warning'), { message: 'the toast' });
    const toast = await page.eval(() => {
      const el = document.querySelector('.rx-toast--warning');
      return {
        text: el.textContent,
        links: [...el.querySelectorAll('a')].map((a) => a.getAttribute('href')),
      };
    });
    assert.match(toast.text, /Stock is low\./);
    // renox-ui.js drops a javascript: link that rode the HX-Trigger as data.
    assert.deepEqual(toast.links, ['/stock']);
    // `seconds(1)`: gone soon after (not hovered).
    await page.waitFor(() => !document.querySelector('.rx-toast--warning'), {
      timeout: 6000,
      message: 'the toast to go',
    });
  }));

test('a toast stays while hovered and its close button dismisses it', () =>
  onOverlays(async (page) => {
    await page.click('#toast-me');
    await page.waitFor(() => document.querySelector('.rx-toast--warning'));
    await page.hover('.rx-toast--warning');
    await sleep(1600);
    assert.ok(await page.eval(() => !!document.querySelector('.rx-toast--warning')), 'kept while hovered');
    await page.click('.rx-toast--warning .rx-toast__close');
    await page.waitFor(() => !document.querySelector('.rx-toast--warning'), { message: 'dismissed' });
  }));

test('an action sheet takes focus, keeps a 422 inside, closes on success and on Escape', () =>
  onOverlays(
    async (page) => {
      await page.click('[data-rx-open="trip-sheet"]');
      await page.waitFor(() => document.querySelector('#trip-sheet').open);
      assert.equal(await page.focused(), 'rx-from', 'the first field is focused');
      // Escape closes it and focus goes back to the button that opened it.
      await page.press('Escape');
      await page.waitFor(() => !document.querySelector('#trip-sheet').open);
      assert.ok(await page.eval(() => document.activeElement.matches('[data-rx-open="trip-sheet"]')));

      await page.click('[data-rx-open="trip-sheet"]');
      await page.waitFor(() => document.querySelector('#trip-sheet').open);
      await page.click('#trip-sheet button[type=submit]');
      await page.waitFor(() => document.querySelector('#rx-from').getAttribute('aria-invalid') === 'true');
      assert.ok(await page.eval(() => document.querySelector('#trip-sheet').open), 'a 422 stays in the sheet');
      await page.type('#rx-from', 'Bandung');
      await page.type('#rx-to', 'Jakarta');
      await page.click('#trip-sheet button[type=submit]');
      await page.waitFor(() => !document.querySelector('#trip-sheet').open, { message: 'closed on success' });
      await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('Bandung saved.'));
      // The form is reset for next time (as the dialog's close event runs).
      await page.waitFor(() => document.querySelector('#rx-from').value === '', { message: 'the form reset' });
    },
    [/422/],
  ));

test('a menu opens, moves with the arrow keys and closes on Escape', () =>
  onOverlays(async (page) => {
    await page.click('[aria-controls="more-menu"]');
    await page.waitFor(() => !document.querySelector('#more-menu').hidden);
    assert.equal(await page.eval(() => document.querySelector('[aria-controls="more-menu"]').getAttribute('aria-expanded')), 'true');
    // Opening focuses the first item; the arrows move between them.
    assert.equal(await page.eval(() => document.activeElement.textContent.trim()), 'Stock');
    await page.press('ArrowDown');
    assert.equal(await page.eval(() => document.activeElement.textContent.trim()), 'Home');
    await page.press('ArrowUp');
    assert.equal(await page.eval(() => document.activeElement.textContent.trim()), 'Stock');
    await page.press('Escape');
    await page.waitFor(() => document.querySelector('#more-menu').hidden);
    assert.ok(await page.eval(() => document.activeElement.matches('[aria-controls="more-menu"]')));
    // A click elsewhere closes it too.
    await page.click('[aria-controls="more-menu"]');
    await page.waitFor(() => !document.querySelector('#more-menu').hidden);
    await page.clickAt(1260, 880);
    await page.waitFor(() => document.querySelector('#more-menu').hidden);
  }));

test('tabs follow the arrow keys, Home and End', () =>
  onOverlays(async (page) => {
    const selected = () =>
      page.eval(() => ({
        tab: document.querySelector('[role=tab][aria-selected="true"]').id,
        panel: [...document.querySelectorAll('[role=tabpanel]')].find((p) => !p.hidden)?.id,
      }));
    await page.focus('#t-tab-one');
    await page.press('ArrowRight');
    assert.deepEqual(await selected(), { tab: 't-tab-two', panel: 't-panel-two' });
    await page.press('End');
    assert.deepEqual(await selected(), { tab: 't-tab-three', panel: 't-panel-three' });
    await page.press('Home');
    assert.deepEqual(await selected(), { tab: 't-tab-one', panel: 't-panel-one' });
    await page.click('#t-tab-three');
    assert.deepEqual(await selected(), { tab: 't-tab-three', panel: 't-panel-three' });
  }));

test('a tooltip shows on hover and on focus, inside the window', () =>
  onOverlays(async (page) => {
    await page.hover('#tipped');
    const tip = await page.waitFor(() => {
      const el = document.querySelector('[role=tooltip]');
      if (!el || el.hidden || !el.textContent.includes('More about this')) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top };
    });
    assert.ok(tip.left >= 0 && tip.right <= 1280 && tip.top >= 0, JSON.stringify(tip));
    // Keyboard focus (focus-visible) shows it at once.
    await page.hover('#typing');
    await page.waitFor(() => !document.querySelector('[role=tooltip]'), { message: 'hidden again' });
    // (the visible tab panel comes just before it)
    await page.focus('#t-panel-one');
    await page.press('Tab');
    assert.equal(await page.focused(), 'tipped');
    await page.waitFor(() => {
      const el = document.querySelector('[role=tooltip]');
      return el && !el.hidden && el.textContent.includes('More about this');
    });
  }));

test('a keyboard shortcut presses its button, but not while typing', () =>
  onOverlays(async (page) => {
    const pressed = () => page.eval(() => Number(document.querySelector('#shortcut').dataset.pressed || 0));
    await page.eval(() => document.activeElement?.blur());
    await page.press('k');
    assert.equal(await pressed(), 1);
    await page.type('#typing', 'k');
    assert.equal(await pressed(), 1, 'typing a k in a field');
    assert.equal(await page.eval(() => document.querySelector('#typing').value), 'k');
  }));

test('Renox.toast shows at the top, waits while focused, replaces by id and is dismissed by id', () =>
  onOverlays(async (page) => {
    const placed = await page.eval(() => {
      const el = Renox.toast({ kind: 'success', message: 'Copied', duration: 600, id: 'copy', actions: [{ label: 'Open', url: '/stock' }] });
      const region = el.closest('[data-renox-toasts]');
      const r = el.getBoundingClientRect();
      return { position: region.className, top: r.top, centre: Math.round(r.left + r.width / 2), half: Math.round(innerWidth / 2) };
    });
    assert.match(placed.position, /rx-toasts--top\b/);
    assert.ok(placed.top < 120 && Math.abs(placed.centre - placed.half) < 4, JSON.stringify(placed));
    // Focus inside it holds it past its 600 ms (a background tab has no
    // focus events without focus emulation).
    await page.send('Emulation.setFocusEmulationEnabled', { enabled: true });
    await page.eval(() => document.querySelector('[data-toast-id="copy"] a').focus());
    await sleep(1200);
    assert.ok(await page.eval(() => !!document.querySelector('[data-toast-id="copy"]')), 'kept while focused');
    // The same id replaces it; Renox.dismissToast removes it.
    await page.eval(() => Renox.toast({ kind: 'info', message: 'Copied again', id: 'copy', duration: 0 }));
    assert.deepEqual(
      await page.eval(() => [...document.querySelectorAll('[data-toast-id="copy"]')].map((t) => t.textContent.includes('Copied again'))),
      [true],
    );
    await page.eval(() => Renox.dismissToast('copy'));
    await page.waitFor(() => !document.querySelector('[data-toast-id="copy"]'), { message: 'dismissed by id' });
  }));

// The owner's call (#266): Escape closes a toast, the focused one or else the
// newest, unless something else Escape closes is open.
test('Escape closes the focused toast, else the newest, after any open menu', () =>
  onOverlays(async (page) => {
    const ids = () => page.eval(() => [...document.querySelectorAll('[data-renox-toast]:not([data-leaving])')].map((t) => t.getAttribute('data-toast-id')));
    await page.eval(() => {
      Renox.toast({ kind: 'info', message: 'First', id: 'one', duration: 0 });
      Renox.toast({ kind: 'info', message: 'Second', id: 'two', duration: 0 });
    });
    // Nothing focused in a toast: the newest goes.
    await page.press('Escape');
    await page.waitFor(() => !document.querySelector('[data-toast-id="two"]'), { message: 'the newest closed' });
    assert.deepEqual(await ids(), ['one']);
    // A menu open (by keyboard: the toast covers its button): Escape closes
    // the menu, not the toast.
    await page.focus('[aria-controls="more-menu"]');
    await page.press('Enter');
    await page.waitFor(() => !document.querySelector('#more-menu').hidden);
    await page.press('Escape');
    await page.waitFor(() => document.querySelector('#more-menu').hidden);
    assert.deepEqual(await ids(), ['one'], 'the toast stays while the menu takes Escape');
    // Focus in a toast: that one goes.
    await page.eval(() => Renox.toast({ kind: 'info', message: 'Third', id: 'three', duration: 0 }));
    await page.eval(() => document.querySelector('[data-toast-id="one"] [data-renox-dismiss]').focus());
    await page.press('Escape');
    await page.waitFor(() => !document.querySelector('[data-toast-id="one"]'), { message: 'the focused one closed' });
    assert.deepEqual(await ids(), ['three']);
  }));

test('a toast action sends its request; a failure says so; other sites are left out', () =>
  onOverlays(
    async (page) => {
      await page.eval(() => Renox.toast({
        kind: 'info', message: 'Archived', duration: 0,
        actions: [
          { label: 'Undo', url: '/undo', method: 'POST' },
          { label: 'Elsewhere', url: '//evil.example/x', method: 'POST' },
          { label: 'Odd', url: '/undo', method: 'GET' },
        ],
      }));
      const labels = await page.eval(() => [...document.querySelectorAll('.rx-toast__action')].map((a) => a.textContent));
      assert.deepEqual(labels, ['Undo'], 'only a request to this site, with a method that changes something');
      await page.click('[data-rx-request="/undo"]');
      await page.waitFor(() => [...document.querySelectorAll('.rx-toast')].some((t) => t.textContent.includes('Undone.')), {
        message: 'the answer’s toast',
      });
      await page.eval(() => Renox.toast({ kind: 'info', message: 'Try it', duration: 0, actions: [{ label: 'Retry', url: '/fails', method: 'POST' }] }));
      await page.click('[data-rx-request="/fails"]');
      await page.waitFor(() => document.querySelector('.rx-toast--error')?.textContent.includes("That didn't work. Try again."), {
        message: 'the failure toast',
      });
    },
    [/400/],
  ));

test('a toast sent with HxRedirect or HxRefresh shows on the next page', () =>
  onOverlays(async (page) => {
    let loaded = page.once('Page.loadEventFired');
    await page.click('#toast-redirect');
    await loaded;
    await page.waitFor(() => location.pathname === '/' && document.querySelector('.rx-toast')?.textContent.includes('Moved along.'), {
      message: 'the toast after the redirect',
    });
    await page.goto(`${app.url}/overlays`);
    loaded = page.once('Page.loadEventFired');
    await page.click('#toast-refresh');
    await loaded;
    await page.waitFor(() => document.querySelector('.rx-toast')?.textContent.includes('Fresh again.'), {
      message: 'the toast after the refresh',
    });
  }));

test('a sheet written as rx-* tags opens from its button and closes', () =>
  onOverlays(async (page) => {
    await page.click('[data-rx-open="tag-sheet"]');
    await page.waitFor(() => document.querySelector('#tag-sheet').open, { message: 'the tag sheet opens' });
    const info = await page.eval(() => {
      const d = document.querySelector('#tag-sheet');
      return {
        classes: d.className,
        title: d.querySelector('.rx-sheet__title').textContent,
        body: d.querySelector('#tag-sheet-body').textContent,
      };
    });
    assert.match(info.classes, /rx-sheet--sm/);
    assert.equal(info.title, 'From tags');
    assert.equal(info.body, 'Inside the tag sheet');
    await page.click('#tag-sheet [data-rx-close]');
    await page.waitFor(() => !document.querySelector('#tag-sheet').open, { message: 'the tag sheet closes' });
    await page.click('[data-rx-open="tag-sheet"]');
    await page.waitFor(() => document.querySelector('#tag-sheet').open);
    await page.press('Escape');
    await page.waitFor(() => !document.querySelector('#tag-sheet').open, { message: 'closed by Escape' });
  }));

test('a side sheet keeps focus inside, closes on its backdrop and gives focus back', () =>
  onOverlays(async (page) => {
    await page.click('[data-rx-open="side-sheet"]');
    await page.waitFor(() => document.querySelector('#side-sheet').open);
    // Once it has slid in.
    await page.waitFor(() => Math.abs(document.querySelector('#side-sheet').getBoundingClientRect().right - innerWidth) <= 2, {
      message: 'the sheet at the right edge',
    });
    const box = await page.eval(() => {
      const d = document.querySelector('#side-sheet');
      const r = d.getBoundingClientRect();
      return { classes: d.className, right: Math.round(r.right), height: Math.round(r.height), width: innerWidth, tall: innerHeight };
    });
    assert.match(box.classes, /rx-sheet--side/);
    assert.match(box.classes, /rx-sheet--lg/);
    assert.ok(Math.abs(box.right - box.width) <= 2 && box.height >= box.tall - 2, JSON.stringify(box));
    // Tab never leaves the sheet for the page behind it.
    for (let i = 0; i < 6; i++) {
      await page.press('Tab');
      const where = await page.eval(() => {
        const el = document.activeElement;
        return !el || el === document.body || document.querySelector('#side-sheet').contains(el);
      });
      assert.ok(where, `tab ${i + 1} stayed in the sheet`);
    }
    // A click on the backdrop (left of the side sheet) closes it.
    await page.clickAt(20, 450);
    await page.waitFor(() => !document.querySelector('#side-sheet').open, { message: 'closed by the backdrop' });
    assert.ok(await page.eval(() => document.activeElement.matches('[data-rx-open="side-sheet"]')));
  }));

test('a menu jumps with Home and End and says when it is closed', () =>
  onOverlays(async (page) => {
    const button = '[aria-controls="more-menu"]';
    await page.click(button);
    await page.waitFor(() => !document.querySelector('#more-menu').hidden);
    await page.press('End');
    assert.equal(await page.eval(() => document.activeElement.textContent.trim()), 'Home');
    await page.press('Home');
    assert.equal(await page.eval(() => document.activeElement.textContent.trim()), 'Stock');
    await page.press('Escape');
    await page.waitFor(() => document.querySelector('#more-menu').hidden);
    assert.equal(await page.eval((b) => document.querySelector(b).getAttribute('aria-expanded'), button), 'false');
    // ArrowUp on the button opens it at the last item.
    await page.focus(button);
    await page.press('ArrowUp');
    assert.equal(await page.eval(() => document.activeElement.textContent.trim()), 'Home');
  }));

test('tooltips share one element', () =>
  onOverlays(async (page) => {
    await page.hover('#tipped');
    await page.waitFor(() => document.querySelector('[role=tooltip]')?.textContent.includes('More about this'));
    await page.hover('#cell-tip');
    await page.waitFor(() => document.querySelector('[role=tooltip]')?.textContent.includes('long explanation'));
    assert.equal(await page.eval(() => document.querySelectorAll('[role=tooltip]').length), 1);
  }));

test('on a phone, a tooltip in a table neither widens the page nor leaves the screen', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/overlays`);
    // The keyboard reaches it (a phone has no hover).
    await page.eval(() => {
      const all = [...document.querySelectorAll('a[href], button, input, [tabindex]')].filter((el) => el.tabIndex >= 0 && el.getClientRects().length && !el.closest('dialog'));
      all[all.indexOf(document.querySelector('#cell-tip')) - 1].focus();
    });
    await page.press('Tab');
    assert.equal(await page.focused(), 'cell-tip');
    const shown = await page.waitFor(() => {
      const tip = document.querySelector('[role=tooltip]');
      if (!tip) return null;
      const r = tip.getBoundingClientRect();
      return { left: r.left, right: r.right, page: document.documentElement.scrollWidth, width: innerWidth };
    }, { message: 'the tooltip' });
    assert.ok(shown.left >= 0 && shown.right <= shown.width, JSON.stringify(shown));
    assert.ok(shown.page <= shown.width, `the page is ${shown.page}px wide on a ${shown.width}px screen`);
  }, { width: 390, height: 844 }));

test('charts: a crosshair follows the pointer, scatter picks the nearest point, keys move it', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/charts`);
    const plot = (id) => page.eval((i) => {
      const r = document.querySelector(`#${i} .rx-chart__plot`).getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height };
    }, id);
    const tip = (id) => page.eval((i) => {
      const t = document.querySelector(`#${i} .rx-chart__tip`);
      return t.hidden ? null : t.textContent;
    }, id);
    const move = (x, y) => page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });

    let box = await plot('line');
    await move(box.left + box.width * (2 / 3), box.top + box.height / 2);
    await page.waitFor(() => !document.querySelector('#line .rx-chart__tip').hidden);
    assert.match(await tip('line'), /^Wed3/);
    assert.equal(await page.eval(() => document.querySelector('#line .rx-chart__cross').hidden), false);

    box = await plot('bar');
    await move(box.left + box.width * 0.25, box.top + box.height / 2);
    await page.waitFor(() => !document.querySelector('#bar .rx-chart__tip').hidden);
    assert.match(await tip('bar'), /^Coffee4/);

    const point = await page.eval(() => {
      const r = document.querySelector('#scatter .rx-chart__point[data-index="1"]').getBoundingClientRect();
      return { x: r.left + r.width / 2 + 3, y: r.top + r.height / 2 + 3 };
    });
    await move(point.x, point.y);
    await page.waitFor(() => !document.querySelector('#scatter .rx-chart__tip').hidden);
    assert.match(await tip('scatter'), /Large/);

    // A bubble chart picks the nearest bubble the same way, near the small one.
    await page.eval(() => document.querySelector('#bubble').scrollIntoView({ block: 'center' }));
    await page.settle();
    const bubble = await page.eval(() => {
      const r = document.querySelector('#bubble .rx-chart__point[data-index="0"]').getBoundingClientRect();
      return { x: r.left + r.width / 2 + 4, y: r.top + r.height / 2 - 4 };
    });
    await move(bubble.x, bubble.y);
    await page.waitFor(() => !document.querySelector('#bubble .rx-chart__tip').hidden);
    assert.match(await tip('bubble'), /Few/);

    // The keyboard: focus shows the last label, the arrows and Home move,
    // Escape hides.
    await move(1, 1);
    await page.focus('#line .rx-chart__plot');
    await page.waitFor(() => !document.querySelector('#line .rx-chart__tip').hidden);
    assert.match(await tip('line'), /^Thu8/);
    await page.press('ArrowLeft');
    assert.match(await tip('line'), /^Wed3/);
    await page.press('Home');
    assert.match(await tip('line'), /^Mon1/);
    await page.press('Escape');
    assert.equal(await tip('line'), null);
  }));

test('the period filter’s custom range sends both dates', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/charts`);
    await page.click('.rx-period__toggle');
    await page.waitFor(() => document.querySelector('[data-rx-period]').open);
    await page.type('#rx-period-from', '2026-09-01');
    await page.type('#rx-period-to', '2026-09-30');
    const loaded = page.once('Page.loadEventFired');
    await page.click('.rx-period__form button[type=submit]');
    await loaded;
    const query = new URLSearchParams(await page.eval(() => location.search));
    assert.equal(query.get('period'), 'custom');
    assert.equal(query.get('from'), '2026-09-01');
    assert.equal(query.get('to'), '2026-09-30');
  }));

test('on a phone, the navbar’s links and the sidebar become a row that scrolls', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/nav`);
    const nav = await page.eval(() => {
      const links = document.querySelector('.rx-navbar__links');
      return { scrolls: links.scrollWidth > links.clientWidth, overflow: getComputedStyle(links).overflowX, page: document.documentElement.scrollWidth, width: innerWidth };
    });
    assert.ok(nav.scrolls && /auto|scroll/.test(nav.overflow), JSON.stringify(nav));
    assert.ok(nav.page <= nav.width, JSON.stringify(nav));
    await page.goto(`${app.url}/shell`);
    const bar = await page.eval(() => {
      const r = document.querySelector('.rx-sidebar').getBoundingClientRect();
      return { top: r.top, width: r.width, height: r.height, page: document.documentElement.scrollWidth, screen: innerWidth };
    });
    assert.ok(bar.top <= 1 && bar.width >= bar.screen - 1 && bar.height < 200, JSON.stringify(bar));
    const links = await page.eval(() => {
      const row = document.querySelector('.rx-sidebar__links');
      return { scrolls: row.scrollWidth > row.clientWidth, overflow: getComputedStyle(row).overflowX };
    });
    assert.ok(links.scrolls && /auto|scroll/.test(links.overflow), JSON.stringify(links));
    assert.ok(bar.page <= bar.screen, JSON.stringify(bar));
  }, { width: 390, height: 844 }));

test('on a desktop, the sidebar is a column down the side', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/shell`);
    const side = await page.eval(() => {
      const r = document.querySelector('.rx-sidebar').getBoundingClientRect();
      return { left: r.left, width: r.width, height: r.height, tall: innerHeight };
    });
    assert.ok(side.left <= 1 && side.width < 400 && side.height >= side.tall - 2, JSON.stringify(side));
  }));

test('the bell counts a new notification live in every tab, toasts it, and marks all read', async () => {
  // A user for this browser (the bell is for logged-in users).
  await browser.with(async (page) => {
    await page.goto(`${app.url}/form`);
    await page.eval(async (email) => {
      const token = document.querySelector('meta[name="csrf-token"]').content;
      const body = new URLSearchParams({ _token: token, name: 'Bell', email, password: 'password123', password_confirmation: 'password123' });
      await fetch('/register', { method: 'POST', body, redirect: 'manual' });
    }, `bell${Date.now()}@example.com`);
  });
  const other = await browser.page();
  try {
    await other.goto(`${app.url}/inbox`);
    await browser.with(async (page) => {
      await page.goto(`${app.url}/inbox`);
      const badge = (p) => p.eval(() => {
        const b = document.querySelector('[data-rx-bell-count]');
        return b.hidden ? '' : b.textContent;
      });
      assert.equal(await badge(page), '');
      // Both tabs have their stream open before anything is sent.
      await sleep(500);
      await page.click('#notify');
      await page.waitFor(() => document.querySelector('[data-rx-bell-count]').textContent === '1' && !document.querySelector('[data-rx-bell-count]').hidden, {
        message: 'the badge counted it',
      });
      await page.waitFor(() => document.querySelector('[data-toast-id^="rx-notification-"]')?.textContent.includes('Hello there'), {
        message: 'the toast',
      });
      await other.waitFor(() => document.querySelector('[data-rx-bell-count]').textContent === '1', { message: 'the other tab' });
      // The panel lists it; "Mark all as read" empties the badge.
      await page.click('.rx-bell__button');
      await page.waitFor(() => document.querySelector('[data-rx-notifications]'), { message: 'the panel' });
      assert.match(await page.eval(() => document.querySelector('[data-rx-notifications]').textContent), /Hello there/);
      await page.eval(() => {
        const form = [...document.querySelectorAll('[data-rx-notification-form]')].find((f) => f.action.endsWith('/read-all'));
        form.querySelector('button').click();
      });
      await page.waitFor(() => document.querySelector('[data-rx-bell-count]').hidden, { message: 'none unread' });
      await other.waitFor(() => document.querySelector('[data-rx-bell-count]').hidden, { message: 'the other tab, read' });
    });
  } finally {
    await other.close();
  }
});

// The owner's call (#266): the bell's stream comes back by itself after the
// server restarts, on the same page.
test('the bell reconnects after the server restarts', async () => {
  // Debug off: with it, live reload would reload the page on the new boot.
  const own = await fixture({ env: { APP_DEBUG: 'false' } });
  try {
    await browser.with(async (page) => {
      await page.goto(`${own.url}/form`);
      await page.eval(async (email) => {
        const token = document.querySelector('meta[name="csrf-token"]').content;
        const body = new URLSearchParams({ _token: token, name: 'Bell', email, password: 'password123', password_confirmation: 'password123' });
        await fetch('/register', { method: 'POST', body, redirect: 'manual' });
      }, `restart${Date.now()}@example.com`);
      const streams = [];
      page.on('Network.requestWillBeSent', (p) => p.request.url.includes('/notifications/stream') && streams.push(Date.now()));
      await page.goto(`${own.url}/inbox`);
      await page.eval(() => { window.__same = true; });
      await page.waitFor(() => true);
      const until = Date.now() + 5000;
      while (!streams.length && Date.now() < until) await sleep(50);
      assert.ok(streams.length >= 1, 'the stream opened');

      await own.restart();
      // The page opens its stream again by itself (no reload).
      const before = streams.length;
      const again = Date.now() + 15_000;
      while (streams.length <= before && Date.now() < again) await sleep(100);
      assert.ok(streams.length > before, 'the stream came back after the restart');
      assert.equal(await page.eval(() => window.__same), true, 'the same page, not reloaded');
      // And it carries what happens on the new server.
      await sleep(500);
      await page.click('#notify');
      await page.waitFor(() => document.querySelector('[data-rx-bell-count]').textContent === '1' && !document.querySelector('[data-rx-bell-count]').hidden, {
        message: 'the badge counted it through the new stream',
        timeout: 15_000,
      });
    });
  } finally {
    await own.stop();
  }
});

// The owner's call (#266): a page shown again by Back from the back/forward
// cache has its sheets and menus closed, as after any other way back.
test('after Back, an open sheet and an open menu are closed', () => browser.with(async (page) => {
  await page.send('Page.addScriptToEvaluateOnNewDocument', {
    source: "addEventListener('pageshow', (e) => { window.__persisted = e.persisted; });",
  });
  const leaveAndComeBack = async () => {
    // A navigation Chrome starts itself, so the page goes to the cache.
    await page.goto(`${app.url}/stock`);
    await page.eval(() => history.back());
    await page.waitFor(() => window.__kept === true, { message: 'the overlays page shown again' });
    assert.equal(await page.eval(() => window.__persisted), true, 'from the back/forward cache');
  };

  await page.goto(`${app.url}/overlays`);
  await page.eval(() => { window.__kept = true; });
  await page.click('[data-rx-open="trip-sheet"]');
  await page.waitFor(() => document.querySelector('#trip-sheet').open);
  await page.type('#rx-from', 'Lisbon');
  await leaveAndComeBack();
  // `close` (which resets the form) is its own task: wait for it.
  await page.waitFor(() => !document.querySelector('#trip-sheet').open && document.querySelector('#rx-from').value === '', {
    message: 'the sheet closed and its form reset',
  }).catch(() => {});
  const sheet = await page.eval(() => ({
    open: document.querySelector('#trip-sheet').open,
    from: document.querySelector('#rx-from').value,
  }));
  assert.equal(sheet.open, false, 'the sheet is closed');
  assert.equal(sheet.from, '', 'and its form starts fresh, as after Escape');

  await page.click('[aria-controls="more-menu"]');
  await page.waitFor(() => !document.querySelector('#more-menu').hidden);
  await leaveAndComeBack();
  const menu = await page.eval(() => ({
    hidden: document.querySelector('#more-menu').hidden,
    expanded: document.querySelector('[aria-controls="more-menu"]').getAttribute('aria-expanded'),
  }));
  assert.deepEqual(menu, { hidden: true, expanded: 'false' });
  page.assertClean();
}));
