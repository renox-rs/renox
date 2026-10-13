// renox-blocks: keypad. A number pad that types into its target field: one
// Tab stop, the arrow keys move between keys by where they are drawn, and
// digits, the decimal point, Backspace and Delete typed while the pad has
// the focus work as on its keys.

const separators = new WeakMap();

/** The page language's decimal separator ("." or ","). */
function decimalSeparator(locale) {
  try {
    const part = new Intl.NumberFormat(locale).formatToParts(1.5).find((p) => p.type === "decimal");
    return part ? part.value : ".";
  } catch (_) {
    return ".";
  }
}

function press(pad, key, kit) {
  const target = document.getElementById(pad.getAttribute("data-rx-keypad"));
  if (!target) return;
  const button = pad.querySelector(`[data-rx-keypad-key="${key}"]`);
  if (button) {
    button.setAttribute("data-rx-keypad-pressed", "");
    setTimeout(() => button.removeAttribute("data-rx-keypad-pressed"), 120);
  }
  if (key === "enter") {
    if (target.form) {
      if (target.form.requestSubmit) target.form.requestSubmit();
      else target.form.submit();
    }
    return;
  }
  let value = target.value;
  const separator = separators.get(pad) || ".";
  if (key === "backspace") value = value.slice(0, -1);
  else if (key === "clear") value = "";
  else if (key === "decimal") {
    if (value.indexOf(separator) === -1) value = (value || "0") + separator;
  } else if (/^\d+$/.test(key)) {
    value = value === "0" ? key.replace(/^0+(?=\d)/, "") || "0" : value + key;
  }
  const limit = parseInt(target.getAttribute("maxlength"), 10);
  if (limit > 0) value = value.slice(0, limit);
  if (value === target.value) return;
  target.value = value;
  kit.fire(target, "input");
  kit.fire(target, "change");
}

/** The key next to `from` in a direction, by where the keys are drawn. */
function nearest(pad, from, dx, dy) {
  const a = from.getBoundingClientRect();
  const ax = a.left + a.width / 2;
  const ay = a.top + a.height / 2;
  let best = null;
  let bestScore = Infinity;
  pad.querySelectorAll("[data-rx-keypad-key]").forEach((key) => {
    if (key === from) return;
    const b = key.getBoundingClientRect();
    const x = b.left + b.width / 2 - ax;
    const y = b.top + b.height / 2 - ay;
    const along = x * dx + y * dy;
    if (along <= 4) return;
    const score = along + (Math.abs(x * dy) + Math.abs(y * dx)) * 3;
    if (score < bestScore) {
      bestScore = score;
      best = key;
    }
  });
  return best;
}

function focusKey(pad, key) {
  pad.querySelectorAll("[data-rx-keypad-key]").forEach((k) => {
    k.tabIndex = k === key ? 0 : -1;
  });
  key.focus();
}

export function start(kit) {
  document.addEventListener("click", (event) => {
    const key = event.target.closest && event.target.closest("[data-rx-keypad] [data-rx-keypad-key]");
    if (!key) return;
    const pad = key.closest("[data-rx-keypad]");
    focusKey(pad, key);
    press(pad, key.getAttribute("data-rx-keypad-key"), kit);
  });

  document.addEventListener("keydown", (event) => {
    const pad = event.target.closest && event.target.closest("[data-rx-keypad]");
    if (!pad || event.ctrlKey || event.metaKey || event.altKey) return;
    const keys = pad.querySelectorAll("[data-rx-keypad-key]");
    const current = event.target.closest("[data-rx-keypad-key]") || keys[0];
    const moves = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    let next = null;
    if (moves[event.key]) next = nearest(pad, current, moves[event.key][0], moves[event.key][1]);
    else if (event.key === "Home") next = keys[0];
    else if (event.key === "End") next = keys[keys.length - 1];
    else if (/^[0-9]$/.test(event.key)) press(pad, event.key, kit);
    else if (event.key === "Backspace") press(pad, "backspace", kit);
    else if (event.key === "Delete") press(pad, "clear", kit);
    else if ((event.key === "." || event.key === ",") && pad.querySelector('[data-rx-keypad-key="decimal"]')) press(pad, "decimal", kit);
    else return; // Enter and Space press the focused key, as on any button.
    event.preventDefault();
    if (next) focusKey(pad, next);
  });
}

export function setup(pad, kit) {
  if (!kit.claim(pad)) return;
  const separator = decimalSeparator(pad.getAttribute("data-rx-keypad-locale") || undefined);
  pad.querySelectorAll("[data-rx-keypad-decimal]").forEach((el) => {
    el.textContent = separator;
  });
  separators.set(pad, separator);
}
