// #268: editors.js (renox-editors) on the bike shop's /about/fields form
// (the fields example's product form until #351): Trix keeps its hidden input in
// step, the Markdown toolbar and preview, CodeJar keeps its textarea in step,
// and each library loads only when needed. The form needs a login: the
// seeded customer's.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser, sleep } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

let browser;
let app;

before(async () => {
  app = await start('bikeshop', 'examples/bikeshop', { seed: true });
  browser = await Browser.launch();
  // Logged in from a tab of its own: in headless Chrome the tab that sent
  // the bike shop's login form gets no key presses afterwards (the cookie
  // is the browser's, so the tests' tabs are logged in too).
  await browser.with((page) => logIn(page));
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

/** Logs the seeded customer in, unless this browser already is. */
async function logIn(page) {
  await page.goto(`${app.url}/login`);
  if (!(await page.eval(() => !!document.querySelector('#rx-email')))) return;
  await page.type('#rx-email', 'customer@bikeshop.test');
  await page.type('#rx-password', 'password');
  const loaded = page.once('Page.loadEventFired');
  await page.click('form button[type=submit]');
  await loaded;
  await page.settle();
}

const onForm = (fn) =>
  browser.with(async (page) => {
    await logIn(page);
    const loaded = [];
    page.on('Network.requestWillBeSent', (p) => loaded.push(new URL(p.request.url).pathname));
    await page.goto(`${app.url}/about/fields`);
    // The bike shop's sections slide in (Motion); start once they're still.
    await page.settle();
    await page.waitFor(() => [...document.querySelectorAll('[data-bs-reveal] > *')].every((el) => getComputedStyle(el).opacity === '1'));
    await fn(page, loaded);
    page.assertClean();
  });

test('the Markdown toolbar wraps the selection and the preview is drawn by the server', () =>
  onForm(async (page) => {
    await page.type('#rx-description', 'strong coffee');
    await page.eval(() => {
      const area = document.querySelector('#rx-description');
      area.setSelectionRange(0, 6);
    });
    await page.click('[data-md="bold"]');
    assert.equal(await page.eval(() => document.querySelector('#rx-description').value), '**strong** coffee');
    // Undo keeps working (execCommand), so Ctrl+Z gives the old text back.
    await page.click('[data-md-preview]');
    await page.waitFor(() => document.querySelector('#rx-description-preview').innerHTML.includes('<strong>strong</strong>'), {
      message: 'the preview',
    });
    assert.equal(await page.eval(() => document.querySelector('[data-md-preview]').getAttribute('aria-pressed')), 'true');
    await page.click('[data-md-preview]');
    assert.ok(await page.eval(() => document.querySelector('#rx-description-preview').hidden));
  }));

test('the rich text editor writes into its hidden field and takes no files', () =>
  onForm(async (page, loaded) => {
    await page.waitFor(() => !!document.querySelector('trix-editor')?.editor, { message: 'Trix ready' });
    assert.ok(loaded.some((p) => p.includes('/_renox/editors/trix-')), 'Trix was loaded for the page');
    await page.click('trix-editor');
    await page.type('trix-editor', 'Fresh beans');
    await page.waitFor(() => document.querySelector('#rx-details').value.includes('Fresh beans'));
    // Bold from the toolbar, with the keyboard. (Trix places the cursor
    // after the typed text a moment later; select once it has.)
    await sleep(300);
    await page.eval(() => document.querySelector('trix-editor').editor.setSelectedRange([0, 5]));
    await page.focus('#rx-details-toolbar [data-trix-attribute="bold"]');
    await page.press('Enter');
    await page.waitFor(() => document.querySelector('#rx-details').value.includes('<strong>Fresh</strong>'));
    assert.equal(
      await page.eval(() => document.querySelector('#rx-details-toolbar [data-trix-attribute="bold"]').getAttribute('aria-pressed')),
      'true',
    );
    // Files are refused: nothing to attach to.
    const accepted = await page.eval(() => {
      const event = new Event('trix-file-accept', { cancelable: true, bubbles: true });
      document.querySelector('trix-editor').dispatchEvent(event);
      return !event.defaultPrevented;
    });
    assert.equal(accepted, false);
  }));

test('the code editor keeps its textarea in step and lets Escape leave', () =>
  onForm(async (page, loaded) => {
    await page.waitFor(() => !!document.querySelector('[data-rx-code-editor] [contenteditable]'), { message: 'CodeJar ready' });
    assert.ok(loaded.some((p) => p.includes('/_renox/editors/codejar-')), 'CodeJar was loaded');
    await page.click('[data-rx-code-editor] [contenteditable]');
    // Typing needs the editor focused (#321): a click that came before CodeJar
    // was ready, or a focus taken by something else, left the keys nowhere.
    await page.waitFor(() => document.activeElement?.matches('[data-rx-code-editor] [contenteditable]'), { message: 'the code editor has the focus' });
    await page.type('[data-rx-code-editor] [contenteditable]', '{"grind": "fine"}');
    await page.waitFor(() => document.querySelector('#rx-settings').value.includes('"grind"'));
    // Highlighted by Prism.
    await page.waitFor(() => !!document.querySelector('[data-rx-code-editor] .token'), { message: 'Prism highlighted the code' });
    // Escape, then Tab, leaves the editor (keyboard users aren't trapped).
    await page.press('Escape');
    await page.press('Tab');
    // The focus moves on a moment after the key (#321): wait for it.
    await page.waitFor(() => !document.activeElement?.closest('[data-rx-code-editor]'), { message: 'Tab left the code editor' });
  }));

// ---------- #268: the rest ----------

/** Ctrl (Cmd on a Mac) with a letter. */
async function ctrl(page, key) {
  const base = { key, code: `Key${key.toUpperCase()}`, windowsVirtualKeyCode: key.toUpperCase().charCodeAt(0), modifiers: 2 };
  await page.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...base });
  await page.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
  await sleep(30);
}

test('a sample saved through every editor, cleaned by the server, its settings highlighted', () =>
  onForm(async (page) => {
    await page.waitFor(() => !!document.querySelector('trix-editor')?.editor && !!document.querySelector('[data-rx-code-editor] [contenteditable]'));
    await page.type('#rx-name', 'Editor bike');
    await page.type('#rx-stock', '3', { clear: true });
    await page.type('#rx-weight_kg', '0.25', { clear: true });
    await page.type('#rx-price', '12.50', { clear: true });
    await page.type('#rx-description', 'Dark and **smooth**');
    await page.click('trix-editor');
    await page.type('trix-editor', 'Roasted on Monday');
    await page.click('[data-rx-code-editor] [contenteditable]');
    await page.type('[data-rx-code-editor] [contenteditable]', '{"grind": ');
    // A broken setting: the form comes back with the error on the code editor.
    let loaded = page.once('Page.loadEventFired');
    await page.click('#sample-form button[type=submit]');
    await loaded;
    await page.settle();
    await page.waitFor(() => !!document.querySelector('[data-rx-code-editor] [contenteditable]'));
    assert.equal(await page.eval(() => document.querySelector('[data-rx-code-editor] .rx-editor__code').getAttribute('aria-invalid')), 'true');
    assert.equal(await page.eval(() => document.querySelector('#rx-name').value), 'Editor bike', 'the old input is back');
    // Fixed; and HTML the page didn't make put into the rich text field.
    await page.eval(() => {
      const code = document.querySelector('[data-rx-code-editor] [contenteditable]');
      code.focus();
      document.execCommand('selectAll');
    });
    await page.type('[data-rx-code-editor] [contenteditable]', '{"grind": "fine"}');
    await page.waitFor(() => document.querySelector('#rx-settings').value === '{"grind": "fine"}');
    await page.eval(() => {
      document.querySelector('#rx-details').value =
        '<p><strong>Kept</strong></p><img src="x" onerror="alert(1)"><script>alert(2)</script>';
    });
    loaded = page.once('Page.loadEventFired');
    await page.click('#sample-form button[type=submit]');
    await loaded;
    await page.settle();
    // Saved: open its page.
    const show = await page.eval(() => location.pathname.replace(/\/edit$/, ''));
    await page.goto(`${app.url}${show}`);
    const details = await page.eval(() => [...document.querySelectorAll('.rx-prose')].map((p) => p.innerHTML).join(''));
    assert.match(details, /<strong>Kept<\/strong>/);
    assert.doesNotMatch(details, /onerror|<script/);
    await page.waitFor(() => document.querySelector('code[data-rx-highlight]')?.hasAttribute('data-rx-highlighted'), { message: 'the code entry highlighted' });
    assert.ok(await page.eval(() => !!document.querySelector('code[data-rx-highlight] .token')));
  }));

test('each library loads only where a page needs it', () =>
  browser.with(async (page) => {
    const loaded = [];
    page.on('Network.requestWillBeSent', (p) => loaded.push(new URL(p.request.url).pathname));
    await logIn(page);
    // A page without a form: no editor at all.
    await page.goto(`${app.url}/about/pages`);
    await sleep(300);
    assert.ok(!loaded.some((p) => /trix-|codejar-|prism-/.test(p)), loaded.join('\n'));
    // A sample's page (saved by the test before): a highlighted code
    // entry, so Prism, not Trix.
    await page.goto(`${app.url}/about/fields`);
    const href = await page.eval(() => document.querySelector('a[href^="/about/fields/"]:not([href$="/edit"])').getAttribute('href'));
    loaded.length = 0;
    await page.goto(`${app.url}${href.replace(/\/edit$/, '')}`);
    await sleep(300);
    assert.ok(!loaded.some((p) => p.includes('trix-')), 'no Trix on a page without rich text');
    page.assertClean();
  }));

test('the rich text editor: Space on its toolbar, its label, no files', () =>
  onForm(async (page) => {
    await page.waitFor(() => !!document.querySelector('trix-editor')?.editor);
    // A click on the label focuses the editor.
    await page.click('[data-rx-rich] > .rx-label');
    assert.equal(await page.eval(() => document.activeElement.tagName), 'TRIX-EDITOR');
    await page.type('trix-editor', 'Calm');
    await sleep(300);
    await page.eval(() => document.querySelector('trix-editor').editor.setSelectedRange([0, 4]));
    await page.focus('#rx-details-toolbar [data-trix-attribute="italic"]');
    await page.press(' ');
    await page.waitFor(() => document.querySelector('#rx-details').value.includes('<em>Calm</em>'), { message: 'italic from Space' });
    assert.equal(
      await page.eval(() => document.querySelector('#rx-details-toolbar [data-trix-attribute="italic"]').getAttribute('aria-pressed')),
      'true',
      'aria-pressed follows the selection',
    );
    // A file inserted (as a paste or a drop would) isn't attached.
    await page.eval(() => document.querySelector('trix-editor').editor.insertFile(new File(['x'], 'notes.txt', { type: 'text/plain' })));
    await sleep(100);
    assert.equal(await page.eval(() => document.querySelector('trix-editor').editor.getDocument().getAttachments().length), 0);
  }));

test('Markdown: links with the address selected, lists on each line, undo, raw HTML as text, back to Write when required', () =>
  onForm(async (page) => {
    const area = '#rx-description';
    await page.type(area, 'Renox');
    await page.eval((a) => document.querySelector(a).setSelectionRange(0, 5), area);
    await page.click('[data-md="link"]');
    const link = await page.eval((a) => {
      const el = document.querySelector(a);
      return { value: el.value, selected: el.value.slice(el.selectionStart, el.selectionEnd) };
    }, area);
    assert.deepEqual(link, { value: '[Renox](https://)', selected: 'https://' });
    // Undo takes the link away again.
    await ctrl(page, 'z');
    assert.equal(await page.eval((a) => document.querySelector(a).value, area), 'Renox');
    // Lists: every selected line.
    await page.type(area, 'one\ntwo\nthree', { clear: true });
    await page.eval((a) => document.querySelector(a).setSelectionRange(0, 13), area);
    await page.click('[data-md="numbers"]');
    assert.equal(await page.eval((a) => document.querySelector(a).value, area), '1. one\n2. two\n3. three');
    // Raw HTML in the preview is text, as the server's `markdown` filter shows it.
    await page.type(area, '<b>bold?</b>', { clear: true });
    await page.click('[data-md-preview]');
    await page.waitFor(() => document.querySelector('#rx-description-preview').textContent.includes('<b>bold?</b>'), { message: 'raw HTML shown as text' });
    assert.equal(await page.eval(() => document.querySelector('#rx-description-preview b')), null);
    // A required field left empty while the preview shows: back to Write.
    await page.eval((a) => {
      const el = document.querySelector(a);
      el.value = '';
      el.required = true;
      el.form.reportValidity();
    }, area);
    assert.equal(await page.eval(() => document.querySelector('#rx-description-preview').hidden), true);
    assert.equal(await page.eval((a) => document.querySelector(a).hidden, area), false);
  }));

test('the code editor: its label, errors shown on it, a form reset, and the events live validation hears', () =>
  onForm(async (page) => {
    await page.waitFor(() => !!document.querySelector('[data-rx-code-editor] [contenteditable]'));
    const heard = [];
    await page.eval(() => {
      window._heard = [];
      for (const type of ['input', 'focusout']) {
        document.addEventListener(type, (e) => {
          if (['settings', 'details'].includes(e.target.name)) window._heard.push(`${e.target.name}:${type}`);
        });
      }
    });
    await page.click('#rx-settings-label');
    assert.ok(await page.eval(() => document.activeElement.classList.contains('rx-editor__code')), 'the label focuses the editor');
    await page.type('[data-rx-code-editor] [contenteditable]', '{}');
    await page.click('#rx-name'); // leaving the editor
    await page.click('trix-editor');
    await page.type('trix-editor', 'x');
    await page.click('#rx-name');
    heard.push(...(await page.waitFor(() => window._heard.length >= 4 && window._heard)));
    for (const event of ['settings:input', 'settings:focusout', 'details:input', 'details:focusout']) {
      assert.ok(heard.includes(event), `${event} in ${heard}`);
    }
    // An error put on the field (live validation, a 422) shows on the editor.
    await page.eval(() => document.querySelector('#rx-settings').setAttribute('aria-invalid', 'true'));
    await page.waitFor(() => document.querySelector('.rx-editor__code').getAttribute('aria-invalid') === 'true');
    await page.eval(() => document.querySelector('#rx-settings').removeAttribute('aria-invalid'));
    await page.waitFor(() => !document.querySelector('.rx-editor__code').hasAttribute('aria-invalid'));
    // The form's reset empties it again.
    await page.eval(() => document.querySelector('#rx-settings').form.reset());
    await page.waitFor(() => document.querySelector('.rx-editor__code').textContent === '', { message: 'the editor reset' });
  }));

test('editors brought in by htmx start; the module runs once', () =>
  onForm(async (page, loaded) => {
    await page.waitFor(() => !!document.querySelector('[data-rx-code-editor] [contenteditable]'));
    // Listeners on the page itself (window and document), as DevTools counts
    // them: an editor's own go on its elements and leave with them.
    const pageListeners = async () => {
      let count = 0;
      for (const expression of ['window', 'document']) {
        const { result } = await page.send('Runtime.evaluate', { expression });
        const { listeners } = await page.send('DOMDebugger.getEventListeners', { objectId: result.objectId });
        count += listeners.length;
      }
      return count;
    };
    const listenersBefore = await pageListeners();
    const modules = () => loaded.filter((p) => /\/editors-[0-9a-f]+\.js$/.test(p) || p.includes('/editors.js')).length;
    const before = modules();
    // The same form again, swapped into the page (with its <script> tag).
    await page.eval(() => {
      const box = document.createElement('div');
      box.id = 'swapped';
      document.body.append(box);
      return htmx.ajax('GET', '/about/fields', { target: '#swapped', select: '#sample-form', swap: 'innerHTML' });
    });
    await page.waitFor(() => document.querySelectorAll('.rx-editor__code').length === 2, { message: 'the new code editor started' });
    await page.waitFor(() => [...document.querySelectorAll('trix-editor')].every((t) => t.editor), { message: 'both Trix editors ready' });
    // A second form's editors add nothing page-wide: their listeners are on
    // their own elements (Trix shares one page listener among its editors).
    assert.equal(await pageListeners(), listenersBefore, 'a second form added no page-wide listeners');
    assert.equal(modules(), before, 'editors.js not loaded again');
    // The original form removed; the new one's editors work, and a toolbar
    // button acts once (a second copy of the module would act twice).
    await page.eval(() => document.querySelector('#sample-form').remove());
    const area = '#swapped [data-rx-markdown] textarea';
    await page.type(area, 'beans');
    await page.eval((a) => document.querySelector(a).setSelectionRange(0, 5), area);
    await page.click('#swapped [data-md="bold"]');
    assert.equal(await page.eval((a) => document.querySelector(a).value, area), '**beans**');
    await page.click('#swapped [data-rx-code-editor] [contenteditable]');
    await page.type('#swapped [data-rx-code-editor] [contenteditable]', '[1]');
    await page.waitFor(() => document.querySelector('#swapped textarea[name="settings"]').value === '[1]');
    // One form left (the first was removed above): as many as at the start.
    assert.equal(await pageListeners(), listenersBefore, 'the removed form took its listeners along');
    // None left: nothing stays behind (Trix even takes its shared one away).
    await page.eval(() => document.querySelector('#swapped').remove());
    assert.ok((await pageListeners()) <= listenersBefore, 'removed editors left no page-wide listeners');
  }));
