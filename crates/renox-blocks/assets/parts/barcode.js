// renox-blocks: barcode_scan. One text field that a code can fill three ways:
// typing, a scanner that types like a keyboard (a "keyboard wedge": keys less
// than 50 ms apart ending in Enter are one scan), or the device's camera read
// by the browser's BarcodeDetector. Every way ends the same: the value is on
// the field and `input` and `change` fire, so the form never knows which one
// was used. Enter in the field does not submit the form unless the block was
// given `submit`, so a scan never posts a half-filled form. The camera button
// is shown only where a camera can be opened; the stream stops when the
// camera is closed, on Escape, when the page is hidden or left, and when the
// block is removed. Nothing is animated.
//
// Where the browser has no BarcodeDetector, the vendored @zxing/library
// (data-zxing on the loader's tag, served from 'self') decodes the frames
// instead. It is fetched only when the camera is opened, once per page.

const WEDGE_MS = 50;
const POLL_MS = 100;

let zxingLoading = null;

/** @zxing/library, added to the page once (a classic script that sets window.ZXing). */
function zxing() {
  if (window.ZXing && window.ZXing.MultiFormatReader) return Promise.resolve(window.ZXing);
  if (!zxingLoading) {
    const tag = document.querySelector("script[data-renox-blocks]");
    const src = tag && tag.getAttribute("data-zxing");
    zxingLoading = new Promise((resolve, reject) => {
      if (!src) return reject(new Error("renox-blocks: no file for zxing"));
      const script = document.createElement("script");
      script.src = src;
      script.onload = () => resolve(window.ZXing);
      script.onerror = () => {
        zxingLoading = null;
        reject(new Error("renox-blocks: zxing did not load"));
      };
      document.head.append(script);
    });
  }
  return zxingLoading;
}

/** A decoder for `formats` (BarcodeDetector names): { detect(video) -> Promise<string|null> }, or null when none can be had. */
async function detectorFor(formats) {
  if (typeof window.BarcodeDetector === "function") {
    const detector = new window.BarcodeDetector(formats.length ? { formats } : undefined);
    return {
      async detect(video) {
        const found = await detector.detect(video);
        return found && found.length && found[0].rawValue ? found[0].rawValue : null;
      },
    };
  }
  try {
    return zxingDetector(await zxing(), formats);
  } catch (error) {
    console.warn(error);
    return null;
  }
}

/** The same interface over zxing: a frame is drawn on a canvas and decoded. */
function zxingDetector(Z, formats) {
  const reader = new Z.MultiFormatReader();
  const known = formats.filter((name) => {
    const ok = Z.BarcodeFormat[name.toUpperCase()] !== undefined;
    if (!ok) console.warn(`renox-blocks: unknown barcode format "${name}"`);
    return ok;
  });
  const hints = new Map();
  if (known.length) hints.set(Z.DecodeHintType.POSSIBLE_FORMATS, known.map((name) => Z.BarcodeFormat[name.toUpperCase()]));
  hints.set(Z.DecodeHintType.TRY_HARDER, true);
  reader.setHints(hints);
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d", { willReadFrequently: true });
  return {
    async detect(video) {
      const width = video.videoWidth;
      const height = video.videoHeight;
      if (!width || !height) return null;
      canvas.width = width;
      canvas.height = height;
      context.drawImage(video, 0, 0, width, height);
      try {
        const source = new Z.HTMLCanvasElementLuminanceSource(canvas);
        return reader.decodeWithState(new Z.BinaryBitmap(new Z.HybridBinarizer(source))).getText() || null;
      } catch (_) {
        return null; // nothing readable in this frame
      }
    },
  };
}

/** The wanted formats, with the ones this browser can't read dropped (and warned about). */
async function supportedFormats(wanted) {
  if (typeof window.BarcodeDetector !== "function" || !window.BarcodeDetector.getSupportedFormats) return wanted;
  try {
    const known = await window.BarcodeDetector.getSupportedFormats();
    const kept = wanted.filter((format) => known.includes(format));
    wanted.filter((format) => !known.includes(format)).forEach((format) => console.warn(`renox-blocks: unknown barcode format "${format}"`));
    return kept;
  } catch (_) {
    return wanted;
  }
}

export function setup(el, kit) {
  const input = el.querySelector("[data-rx-barcode-input]");
  const button = el.querySelector("[data-rx-barcode-button]");
  const live = el.querySelector("[data-rx-barcode-live]");
  const video = el.querySelector("[data-rx-barcode-video]");
  const close = el.querySelector("[data-rx-barcode-close]");
  const status = el.querySelector("[data-rx-barcode-status]");
  if (!input || !button || !live || !video || !close || !status || !kit.claim(el)) return;
  const texts = kit.texts(el);
  const wanted = (el.getAttribute("data-rx-barcode-formats") || "")
    .split(",")
    .map((format) => format.trim())
    .filter(Boolean);
  const submits = el.hasAttribute("data-rx-barcode-submit");
  const form = el.closest("form");

  let stream = null;
  let timer = 0;
  let opening = false;

  function say(text) {
    status.textContent = text;
  }

  /** Puts a code on the field the way typing would. */
  function fill(code) {
    input.value = code;
    kit.fire(input, "input");
    kit.fire(input, "change");
    say(kit.fill(texts.read, { code }));
  }

  // The keyboard wedge: a run of keys less than WEDGE_MS apart ending in Enter.
  let lastKey = 0;
  let fast = 0;
  input.addEventListener("keydown", (event) => {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    const gap = event.timeStamp - lastKey;
    lastKey = event.timeStamp;
    if (event.key !== "Enter") {
      if (event.key.length === 1) fast = gap < WEDGE_MS ? fast + 1 : 0;
      return;
    }
    const scanned = gap < WEDGE_MS && fast >= 1;
    fast = 0;
    const code = input.value.trim();
    if (scanned && code) {
      event.preventDefault();
      fill(code);
      if (submits && form) form.requestSubmit();
    } else if (!submits) {
      // Typed by hand: kept as typed, and Enter does not post the form.
      event.preventDefault();
      if (code) kit.fire(input, "change");
    }
  });

  function stop() {
    clearTimeout(timer);
    timer = 0;
    if (stream) stream.getTracks().forEach((track) => track.stop());
    stream = null;
    video.srcObject = null;
  }

  /** Closes the camera panel; `back` puts focus on the field, `note` is said. */
  function closeCamera(note, back) {
    const wasOpen = !live.hidden;
    stop();
    live.hidden = true;
    button.setAttribute("aria-expanded", "false");
    if (note) say(note);
    if (wasOpen && back) (back === "button" ? button : input).focus();
  }

  /** Reads frames until a code is found, about POLL_MS apart. */
  function watch(detector) {
    let busy = false;
    const tick = async () => {
      if (!stream) return;
      if (!busy && video.readyState >= 2) {
        busy = true;
        try {
          const code = await detector.detect(video);
          if (code && stream) {
            closeCamera(null, "input");
            fill(code);
            return;
          }
        } catch (_) {
          // A frame that can't be read is skipped.
        }
        busy = false;
      }
      if (stream) timer = setTimeout(() => requestAnimationFrame(tick), POLL_MS);
    };
    requestAnimationFrame(tick);
  }

  async function open() {
    if (opening || stream) return;
    opening = true;
    try {
      const formats = await supportedFormats(wanted);
      const detector = await detectorFor(formats);
      if (!detector) {
        say(texts.unsupported);
        input.focus();
        return;
      }
      const opened = await navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: "environment" } }, audio: false });
      if (!el.isConnected) {
        opened.getTracks().forEach((track) => track.stop());
        return;
      }
      stream = opened;
      video.srcObject = stream;
      live.hidden = false;
      button.setAttribute("aria-expanded", "true");
      say(texts.scanning);
      const playing = video.play && video.play();
      if (playing && playing.catch) playing.catch(() => {});
      close.focus();
      watch(detector);
    } catch (error) {
      const name = error && error.name;
      if (name === "NotAllowedError" || name === "SecurityError") say(texts.denied);
      else if (name === "NotFoundError" || name === "OverconstrainedError") say(texts.missing);
      else say(texts.failed);
      input.focus();
    } finally {
      opening = false;
    }
  }

  if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
    button.hidden = false;
    button.addEventListener("click", open);
  }
  close.addEventListener("click", () => closeCamera(texts.closed, "button"));
  el.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !live.hidden) {
      event.preventDefault();
      closeCamera(texts.closed, "button");
    }
  });

  // Release the camera when the block goes away (htmx swap) or the page goes.
  function release(event) {
    if (event.target === el || event.type === "pagehide") stop();
  }
  document.addEventListener("htmx:beforeCleanupElement", release);
  window.addEventListener("pagehide", release);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden && stream) closeCamera(texts.closed, null);
  });
}
