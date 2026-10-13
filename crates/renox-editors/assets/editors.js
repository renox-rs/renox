// renox-editors: the rich text (Trix), Markdown and code (CodeJar + Prism)
// editors, and highlighting for code entries. A module, so it runs once per
// page however often its <script> tag arrives (htmx swaps bring it again);
// it loads each library only when the page has a field that needs it.

const script = document.querySelector("script[data-renox-editors]");
const files = {
  trix: script ? script.dataset.trix : null,
  prism: script ? script.dataset.prism : null,
  codejar: script ? script.dataset.codejar : null,
};

let trixLoading = null;
let codeLoading = null;

function loadTrix() {
  if (!trixLoading) {
    trixLoading = import(files.trix);
  }
  return trixLoading;
}

function loadCode() {
  if (!codeLoading) {
    codeLoading = import(files.prism).then(() => import(files.codejar));
  }
  return codeLoading;
}

function csrf() {
  const meta = document.querySelector('meta[name="csrf-token"]');
  return meta ? meta.content : "";
}

// Live validation (the kit's `data-live-validate`) listens to `input` and
// `focusout` on named fields: an editor passes them on to its field.
function tell(field, type) {
  field.dispatchEvent(new Event(type, { bubbles: true }));
}

// ---------- Rich text (Trix) ----------

document.addEventListener("trix-file-accept", (event) => {
  if (event.target.closest("[data-rx-rich]")) event.preventDefault();
});

document.addEventListener("trix-before-initialize", (event) => {
  const editor = event.target;
  if (!editor.closest("[data-rx-rich]")) return;
  // No files in this field, so no paste of images either.
  editor.addEventListener("trix-attachment-add", (e) => {
    e.attachment.remove();
  });
});

document.addEventListener("trix-change", (event) => {
  const box = event.target.closest("[data-rx-rich]");
  const field = box && box.querySelector('input[type="hidden"]');
  if (field) tell(field, "input");
});

document.addEventListener("trix-blur", (event) => {
  const box = event.target.closest("[data-rx-rich]");
  const field = box && box.querySelector('input[type="hidden"]');
  if (field) tell(field, "focusout");
});

// The active formats, for screen readers.
function syncPressed(editor) {
  const box = editor.closest("[data-rx-rich]");
  if (!box) return;
  requestAnimationFrame(() => {
    box.querySelectorAll("[data-trix-attribute]").forEach((button) => {
      button.setAttribute("aria-pressed", button.hasAttribute("data-trix-active") ? "true" : "false");
    });
  });
}
document.addEventListener("trix-selection-change", (event) => syncPressed(event.target));
document.addEventListener("trix-focus", (event) => syncPressed(event.target));

// Trix's toolbar answers the mouse; a button pressed with the keyboard
// (Enter or Space) does the same.
document.addEventListener("click", (event) => {
  const button = event.target.closest && event.target.closest("[data-rx-rich] .rx-editor__tool");
  if (!button || event.detail !== 0) return;
  button.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
});

// The rich editor's label has no `for` (a <trix-editor> isn't a form
// control): a click on it focuses the editor.
document.addEventListener("click", (event) => {
  const label = event.target.closest && event.target.closest("[data-rx-rich] > .rx-label");
  if (!label) return;
  const editor = label.parentElement.querySelector("trix-editor");
  if (editor) editor.focus();
});

// ---------- Markdown ----------

const wraps = { bold: ["**", "**"], italic: ["_", "_"], strike: ["~~", "~~"] };
const prefixes = { heading: "## ", quote: "> ", bullets: "- ", numbers: "1. " };

// Replaces the selection with `text`, keeping undo (execCommand), and
// selects `select` (offsets in `text`) afterwards.
function replaceSelection(area, text, select) {
  const start = area.selectionStart;
  area.focus();
  if (!document.execCommand || !document.execCommand("insertText", false, text)) {
    area.setRangeText(text, area.selectionStart, area.selectionEnd, "end");
    tell(area, "input");
  }
  if (select) area.setSelectionRange(start + select[0], start + select[1]);
}

function applyMarkdown(area, kind) {
  const { selectionStart: start, selectionEnd: end, value } = area;
  const selected = value.slice(start, end);
  if (wraps[kind]) {
    const [open, close] = wraps[kind];
    const inner = selected || kind;
    replaceSelection(area, open + inner + close, [open.length, open.length + inner.length]);
  } else if (kind === "code") {
    if (selected.includes("\n")) {
      const block = "```\n" + selected.replace(/\n$/, "") + "\n```\n";
      replaceSelection(area, block, [4, 4 + selected.replace(/\n$/, "").length]);
    } else {
      const inner = selected || "code";
      replaceSelection(area, "`" + inner + "`", [1, 1 + inner.length]);
    }
  } else if (kind === "link") {
    const text = selected || "text";
    const md = "[" + text + "](https://)";
    // The address is selected, ready to type over.
    replaceSelection(area, md, [text.length + 3, md.length - 1]);
  } else if (prefixes[kind]) {
    // Each line of the selection (or the line of the cursor).
    const from = value.lastIndexOf("\n", start - 1) + 1;
    let to = value.indexOf("\n", end);
    if (to < 0) to = value.length;
    const lines = value.slice(from, to).split("\n");
    const text = lines
      .map((line, i) => (kind === "numbers" ? i + 1 + ". " : prefixes[kind]) + line)
      .join("\n");
    area.setSelectionRange(from, to);
    replaceSelection(area, text, [text.length, text.length]);
  }
}

function previewOf(box) {
  return {
    area: box.querySelector("textarea"),
    preview: box.querySelector(".rx-editor__preview"),
    toggle: box.querySelector("[data-md-preview]"),
  };
}

function showWrite(box) {
  const { area, preview, toggle } = previewOf(box);
  preview.hidden = true;
  area.hidden = false;
  toggle.setAttribute("aria-pressed", "false");
  box.querySelectorAll("[data-rx-markdown-tools] button").forEach((b) => (b.disabled = false));
}

function showPreview(box) {
  const { area, preview, toggle } = previewOf(box);
  toggle.setAttribute("aria-pressed", "true");
  box.querySelectorAll("[data-rx-markdown-tools] button").forEach((b) => (b.disabled = true));
  preview.style.minHeight = area.offsetHeight + "px";
  area.hidden = true;
  preview.hidden = false;
  if (!area.value.trim()) {
    preview.innerHTML = "";
    const p = document.createElement("p");
    p.className = "rx-editor__placeholder";
    p.textContent = box.dataset.previewEmpty;
    preview.append(p);
    return;
  }
  preview.setAttribute("aria-busy", "true");
  const body = new URLSearchParams({ text: area.value });
  fetch(box.dataset.previewUrl, {
    method: "POST",
    body,
    credentials: "same-origin",
    headers: { "X-CSRF-Token": csrf(), Accept: "text/html" },
  })
    .then((res) => (res.ok ? res.text() : Promise.reject(res.status)))
    .then((html) => {
      // Rendered on the server by the `markdown` filter, which shows raw
      // HTML as text: the same HTML the page shows later.
      preview.innerHTML = html;
    })
    .catch(() => {
      preview.textContent = box.dataset.previewFailed;
    })
    .finally(() => preview.removeAttribute("aria-busy"));
}

document.addEventListener("click", (event) => {
  const target = event.target.closest && event.target.closest("[data-rx-markdown] button");
  if (!target) return;
  const box = target.closest("[data-rx-markdown]");
  if (target.hasAttribute("data-md-preview")) {
    if (target.getAttribute("aria-pressed") === "true") {
      showWrite(box);
      box.querySelector("textarea").focus();
    } else {
      showPreview(box);
    }
    return;
  }
  const area = box.querySelector("textarea");
  if (target.dataset.md && !area.disabled && !area.readOnly) applyMarkdown(area, target.dataset.md);
});

document.addEventListener("keydown", (event) => {
  const area = event.target;
  if (!(event.ctrlKey || event.metaKey) || event.altKey || area.tagName !== "TEXTAREA") return;
  if (!area.closest("[data-rx-markdown]") || area.readOnly) return;
  const kind = { b: "bold", i: "italic", k: "link" }[event.key.toLowerCase()];
  if (!kind || event.shiftKey) return;
  event.preventDefault();
  applyMarkdown(area, kind);
});

// A required field left empty while the preview shows: back to writing,
// so the browser can point at it.
document.addEventListener(
  "invalid",
  (event) => {
    const box = event.target.closest && event.target.closest("[data-rx-markdown]");
    if (box && event.target.hidden) showWrite(box);
  },
  true,
);

// ---------- Code ----------

function highlight(Prism, element) {
  const match = /language-([\w-]+)/.exec(element.className);
  const language = match ? match[1] : "none";
  const grammar = Prism.languages[language === "html" ? "markup" : language];
  if (grammar) {
    element.innerHTML = Prism.highlight(element.textContent, grammar, language);
  }
}

function startCodeEditor(box, CodeJar, Prism) {
  const area = box.querySelector("textarea");
  if (!area || box.querySelector(".rx-editor__code")) return;
  const language = box.dataset.language || "none";
  const editor = document.createElement("div");
  editor.className = "rx-editor__area rx-editor__code language-" + language;
  editor.setAttribute("role", "textbox");
  editor.setAttribute("aria-multiline", "true");
  editor.setAttribute("aria-labelledby", area.id + "-label");
  editor.setAttribute("aria-describedby", area.getAttribute("aria-describedby") || "");
  editor.setAttribute("spellcheck", "false");
  editor.setAttribute("translate", "no");
  editor.style.minHeight = area.offsetHeight + "px";
  for (const name of ["aria-invalid", "aria-required"]) {
    if (area.hasAttribute(name)) editor.setAttribute(name, area.getAttribute(name));
  }
  if (area.placeholder) editor.dataset.placeholder = area.placeholder;
  const locked = area.disabled || area.readOnly;
  if (locked) editor.setAttribute("aria-readonly", "true");
  editor.tabIndex = area.disabled ? -1 : 0;
  area.after(editor);
  // The textarea stays the form's field, out of sight; the browser can't
  // point at a hidden field, so the server's `required` speaks instead.
  area.hidden = true;
  area.required = false;
  const jar = CodeJar(editor, (el) => highlight(Prism, el), {
    tab: box.dataset.tab || "  ",
    catchTab: true,
    addClosing: false,
  });
  jar.updateCode(area.value);
  if (locked) editor.setAttribute("contenteditable", "false");
  const sync = (code) => {
    if (area.value === code) return;
    area.value = code;
    tell(area, "input");
  };
  jar.onUpdate(sync);
  // CodeJar reports on key presses; text that arrives otherwise (dictation,
  // an input method, a drop) is caught here.
  editor.addEventListener("input", () => {
    sync(jar.toString());
    requestAnimationFrame(() => {
      // Removed meanwhile (an htmx swap right after typing): nothing to repaint.
      if (!editor.isConnected) return;
      const caret = jar.save();
      highlight(Prism, editor);
      jar.restore(caret);
    });
  });
  // Escape lets the next Tab leave the editor (keyboard users aren't trapped).
  editor.addEventListener("keydown", (event) => {
    if (event.key === "Escape") jar.updateOptions({ catchTab: false });
  });
  editor.addEventListener("focus", () => jar.updateOptions({ catchTab: true }));
  editor.addEventListener("focusout", () => tell(area, "focusout"));
  // Errors shown on the textarea (live validation, a 422) show on the editor.
  new MutationObserver(() => {
    if (area.hasAttribute("aria-invalid")) editor.setAttribute("aria-invalid", area.getAttribute("aria-invalid"));
    else editor.removeAttribute("aria-invalid");
  }).observe(area, { attributes: true, attributeFilter: ["aria-invalid"] });
  // The label points at the textarea: a click on it focuses the editor.
  const label = box.querySelector(".rx-label");
  if (label) label.addEventListener("click", (event) => {
    event.preventDefault();
    editor.focus();
  });
  // A form reset puts the textarea back; show that.
  if (area.form) area.form.addEventListener("reset", () => setTimeout(() => jar.updateCode(area.value)));
}

// ---------- Start ----------

function start(root) {
  if (!root || !root.querySelector) return;
  if (root.querySelector("[data-rx-rich]")) loadTrix();
  const editors = root.querySelectorAll("[data-rx-code-editor]");
  const entries = root.querySelectorAll("code[data-rx-highlight]:not([data-rx-highlighted])");
  if (editors.length || entries.length) {
    loadCode().then((module) => {
      const Prism = window.Prism;
      entries.forEach((code) => {
        code.setAttribute("data-rx-highlighted", "");
        highlight(Prism, code);
      });
      editors.forEach((box) => startCodeEditor(box, module.CodeJar, Prism));
    });
  }
}

start(document);
// Content htmx brings in later (a form in a sheet, a swapped section).
document.addEventListener("htmx:load", (event) => start(event.target));
