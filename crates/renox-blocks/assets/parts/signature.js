// renox-blocks: signature_pad. A canvas drawn with pointer events (mouse,
// pen and touch alike). Every stroke is kept as points (fractions of the
// pad's size, so a resize redraws it), which gives Undo and Clear. After
// each change the drawing is put on the block's hidden file input as
// `signature.png` through DataTransfer, so the form posts a real file. The
// typed-name field next to it is the way without a pointer: typing there
// drops the drawing, drawing empties the typed name, so exactly one of the
// two is posted. Nothing is animated.

const MIN_WIDTH = 120;

/** A PNG's bytes from a canvas, synchronously (so a submit never races it). */
function pngFile(canvas, name) {
  const data = canvas.toDataURL("image/png");
  const raw = atob(data.slice(data.indexOf(",") + 1));
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return new File([bytes], name, { type: "image/png" });
}

/** Puts `file` (or nothing) on a file input; false when the browser can't. */
function setFile(input, file) {
  if (typeof DataTransfer !== "function") return false;
  const transfer = new DataTransfer();
  if (file) transfer.items.add(file);
  input.files = transfer.files;
  return true;
}

export function setup(el, kit) {
  const canvas = el.querySelector("[data-rx-signature-canvas]");
  const undo = el.querySelector("[data-rx-signature-undo]");
  const clear = el.querySelector("[data-rx-signature-clear]");
  const typed = el.querySelector("[data-rx-signature-name]");
  const input = el.querySelector("[data-rx-signature-file]");
  const status = el.querySelector("[data-rx-signature-status]");
  const context = canvas && canvas.getContext && canvas.getContext("2d");
  // Without a canvas, or without DataTransfer, only the typed name is offered.
  if (!context || !input || !setFile(input, null) || !kit.claim(el)) return;
  const texts = kit.texts(el);
  const width = parseFloat(el.getAttribute("data-rx-signature-width")) || 400;
  const height = parseFloat(el.getAttribute("data-rx-signature-height")) || 160;
  canvas.style.setProperty("--rx-signature-width", `${Math.max(width, MIN_WIDTH)}px`);
  canvas.style.setProperty("--rx-signature-ratio", `${width} / ${height}`);

  /** Strokes: arrays of [x, y] as fractions of the pad. */
  let strokes = [];
  let current = null;
  let pointer = null;
  let scale = 1;

  function say(text) {
    if (status) status.textContent = text;
  }

  function point(event) {
    const box = canvas.getBoundingClientRect();
    const x = (event.clientX - box.left) / (box.width || 1);
    const y = (event.clientY - box.top) / (box.height || 1);
    return [Math.min(1, Math.max(0, x)), Math.min(1, Math.max(0, y))];
  }

  function ink() {
    context.strokeStyle = getComputedStyle(canvas).color;
    context.fillStyle = context.strokeStyle;
    context.lineWidth = 2.5 * scale;
    context.lineCap = "round";
    context.lineJoin = "round";
  }

  /** Draws the part of `stroke` from its point `from` on. */
  function draw(stroke, from) {
    const w = canvas.width;
    const h = canvas.height;
    ink();
    if (stroke.length === 1) {
      context.beginPath();
      context.arc(stroke[0][0] * w, stroke[0][1] * h, 1.4 * scale, 0, Math.PI * 2);
      context.fill();
      return;
    }
    context.beginPath();
    context.moveTo(stroke[Math.max(from - 1, 0)][0] * w, stroke[Math.max(from - 1, 0)][1] * h);
    for (let i = Math.max(from, 1); i < stroke.length; i++) {
      context.lineTo(stroke[i][0] * w, stroke[i][1] * h);
    }
    context.stroke();
  }

  /** Sizes the bitmap to the pad on screen and the screen's density, and redraws. */
  function paint() {
    const box = canvas.getBoundingClientRect();
    if (!box.width) return;
    scale = window.devicePixelRatio || 1;
    canvas.width = Math.round(box.width * scale);
    canvas.height = Math.round(box.height * scale);
    // A white page, so the PNG reads on any background.
    context.fillStyle = "#fff";
    context.fillRect(0, 0, canvas.width, canvas.height);
    strokes.forEach((stroke) => draw(stroke, 0));
  }

  /** Where the form's two ways stand after a change. */
  function sync(message) {
    const inked = strokes.length > 0;
    undo.disabled = clear.disabled = !inked;
    if (!inked && (document.activeElement === undo || document.activeElement === clear)) canvas.focus();
    if (inked) typed.value = "";
    // Either way satisfies `required`: the name is asked for only while the pad is empty.
    if (typed.hasAttribute("data-rx-signature-required")) typed.required = !inked;
    setFile(input, inked ? pngFile(canvas, "signature.png") : null);
    kit.fire(input, "change");
    if (message) say(message);
  }

  function reset() {
    strokes = [];
    current = null;
    paint();
    sync("");
  }

  canvas.addEventListener("pointerdown", (event) => {
    if (pointer !== null || (event.pointerType === "mouse" && event.button !== 0)) return;
    event.preventDefault();
    pointer = event.pointerId;
    if (canvas.setPointerCapture) canvas.setPointerCapture(pointer);
    current = [point(event)];
    strokes.push(current);
    draw(current, 0);
  });
  canvas.addEventListener("pointermove", (event) => {
    if (event.pointerId !== pointer || !current) return;
    const events = event.getCoalescedEvents ? event.getCoalescedEvents() : [];
    const from = current.length;
    for (const e of events.length ? events : [event]) current.push(point(e));
    draw(current, from);
  });
  function finish(event) {
    if (event.pointerId !== pointer) return;
    pointer = null;
    current = null;
    // Drawn again in one go: segments stroked as they came overlap and darken at their joints,
    // so the picture would depend on how fast the pen moved. Undo and a resize redraw the same way.
    paint();
    sync(texts.drawn);
  }
  canvas.addEventListener("pointerup", finish);
  canvas.addEventListener("pointercancel", finish);

  undo.addEventListener("click", () => {
    strokes.pop();
    paint();
    sync(texts.undone);
  });
  clear.addEventListener("click", () => {
    strokes = [];
    paint();
    sync(texts.cleared);
  });

  // Typing a name drops the drawing: the last way used wins.
  typed.addEventListener("input", () => {
    if (typed.value && strokes.length) {
      strokes = [];
      paint();
      sync(texts.cleared);
    }
  });

  const form = el.closest("form");
  if (form) form.addEventListener("reset", () => setTimeout(reset));

  if (window.ResizeObserver) {
    let last = 0;
    new ResizeObserver(() => {
      const now = canvas.getBoundingClientRect().width;
      if (now && now !== last) {
        last = now;
        paint();
        // The bitmap was cleared by the resize; the file still holds the drawing, redone to the new size.
        if (strokes.length) setFile(input, pngFile(canvas, "signature.png"));
      }
    }).observe(canvas);
  } else {
    paint();
  }
}
