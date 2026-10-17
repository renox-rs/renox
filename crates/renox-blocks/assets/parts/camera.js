// renox-blocks: camera_capture. Photos from the device's camera
// (getUserMedia), each downsized on a canvas and put on the block's file
// input as `photo-N.jpg` through DataTransfer, so the form posts real files.
// Without the script the input is simply a visible file field (`capture`
// opens the camera app on a phone). When the camera is missing, denied or
// fails, the input stays (or becomes) visible with a message saying why, and
// what it picks goes through the same downsizing. The stream is stopped when
// the block is removed or the page is left. Nothing is animated.

/** Puts `files` on a file input; false when the browser can't. */
function setFiles(input, files) {
  if (typeof DataTransfer !== "function") return false;
  const transfer = new DataTransfer();
  files.forEach((file) => transfer.items.add(file));
  input.files = transfer.files;
  return true;
}

/** A canvas as a JPEG File. */
function jpeg(canvas, name, quality) {
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => (blob ? resolve(new File([blob], name, { type: "image/jpeg" })) : reject(new Error("no image"))),
      "image/jpeg",
      quality,
    );
  });
}

/** Loads an image file as something drawable: [source, width, height, release]. */
async function decode(file) {
  if (window.createImageBitmap) {
    const bitmap = await createImageBitmap(file);
    return [bitmap, bitmap.width, bitmap.height, () => bitmap.close && bitmap.close()];
  }
  const url = URL.createObjectURL(file);
  const image = new Image();
  image.src = url;
  await image.decode();
  return [image, image.naturalWidth, image.naturalHeight, () => URL.revokeObjectURL(url)];
}

/** `source` drawn at most `limit` pixels wide (aspect kept) as a JPEG File. */
function shrink(source, width, height, limit, quality, name) {
  const ratio = width > limit ? limit / width : 1;
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width * ratio));
  canvas.height = Math.max(1, Math.round(height * ratio));
  const context = canvas.getContext("2d");
  // JPEG has no transparency: a white page under it.
  context.fillStyle = "#fff";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(source, 0, 0, canvas.width, canvas.height);
  return jpeg(canvas, name, quality);
}

export function setup(el, kit) {
  const input = el.querySelector("[data-rx-camera-file]");
  const fallback = el.querySelector("[data-rx-camera-fallback]");
  const live = el.querySelector("[data-rx-camera-live]");
  const video = el.querySelector("[data-rx-camera-video]");
  const shutter = el.querySelector("[data-rx-camera-shutter]");
  const list = el.querySelector("[data-rx-camera-shots]");
  const status = el.querySelector("[data-rx-camera-status]");
  // Without DataTransfer the plain file input is all there is.
  if (!input || !fallback || !live || !video || !shutter || !list || !setFiles(input, []) || !kit.claim(el)) return;
  const texts = kit.texts(el);
  const max = Math.max(1, parseInt(el.getAttribute("data-rx-camera-max"), 10) || 1);
  const limit = Math.max(16, parseInt(el.getAttribute("data-rx-camera-width"), 10) || 2048);
  const quality = Math.min(1, Math.max(0.1, parseFloat(el.getAttribute("data-rx-camera-quality")) || 0.85));
  const facing = el.getAttribute("data-rx-camera-facing") || "environment";
  const needed = input.hasAttribute("data-rx-camera-required");

  /** The photos: { file, url }. */
  let shots = [];
  let stream = null;
  let counter = 0;

  function say(text) {
    if (status) status.textContent = text;
  }

  /** Brings the list, the input and the shutter in line with `shots`. */
  function render() {
    list.textContent = "";
    shots.forEach((shot, index) => {
      const item = document.createElement("li");
      item.className = "rx-camera__shot";
      const image = document.createElement("img");
      image.className = "rx-camera__thumb";
      image.src = shot.url;
      image.alt = kit.fill(texts.photo, { n: index + 1 });
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "rx-button rx-button--secondary rx-button--small";
      remove.textContent = kit.fill(texts.remove, { n: index + 1 });
      remove.addEventListener("click", () => {
        URL.revokeObjectURL(shot.url);
        shots = shots.filter((other) => other !== shot);
        render();
        say(texts.removed);
        const next = list.querySelector("button") || (live.hidden ? input : shutter);
        next.focus();
      });
      item.append(image, remove);
      list.append(item);
    });
    setFiles(input, shots.map((shot) => shot.file));
    shutter.disabled = !stream || shots.length >= max;
  }

  /** Adds downsized photos, up to `max` (a single-photo block keeps the last). */
  function add(files) {
    for (const file of files) {
      if (max === 1) {
        shots.forEach((shot) => URL.revokeObjectURL(shot.url));
        shots = [];
      } else if (shots.length >= max) {
        say(texts.full);
        break;
      }
      counter += 1;
      shots.push({ file, url: URL.createObjectURL(file) });
      say(kit.fill(texts.taken, { n: shots.length }));
    }
    render();
  }

  function stop() {
    if (stream) stream.getTracks().forEach((track) => track.stop());
    stream = null;
    video.srcObject = null;
    shutter.disabled = true;
  }

  /** The visible file input, with the reason the camera isn't used. */
  function useInput(reason) {
    stop();
    live.hidden = true;
    fallback.hidden = false;
    if (needed) input.required = true;
    if (reason) say(reason);
  }

  shutter.addEventListener("click", async () => {
    if (!stream || shots.length >= max || !video.videoWidth) return;
    shutter.disabled = true;
    try {
      add([await shrink(video, video.videoWidth, video.videoHeight, limit, quality, `photo-${counter + 1}.jpg`)]);
    } catch (_) {
      render();
    }
  });

  // What the file input picks goes through the same downsizing.
  input.addEventListener("change", async () => {
    const chosen = Array.from(input.files || []).filter((file) => file.type.startsWith("image/"));
    if (!chosen.length) return;
    const done = [];
    for (const file of chosen) {
      try {
        const [source, width, height, release] = await decode(file);
        done.push(await shrink(source, width, height, limit, quality, `photo-${counter + done.length + 1}.jpg`));
        release();
      } catch (_) {
        // Not an image the browser can read: the server's rules will say so.
      }
    }
    if (done.length) add(done);
  });

  const form = el.closest("form");
  if (form) {
    form.addEventListener("reset", () =>
      setTimeout(() => {
        shots.forEach((shot) => URL.revokeObjectURL(shot.url));
        shots = [];
        render();
      }),
    );
  }

  // Release the camera when the block goes away (htmx swap) or the page is left.
  function release(event) {
    if (event.target === el || event.type === "pagehide") stop();
  }
  document.addEventListener("htmx:beforeCleanupElement", release);
  window.addEventListener("pagehide", release);

  async function start() {
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      useInput(texts.missing);
      return;
    }
    try {
      const opened = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: { ideal: facing } },
        audio: false,
      });
      if (!el.isConnected) {
        opened.getTracks().forEach((track) => track.stop());
        return;
      }
      stream = opened;
      video.srcObject = stream;
      fallback.hidden = true;
      live.hidden = false;
      // A hidden required input would block the submit with nothing to focus.
      if (needed) input.required = false;
      const playing = video.play && video.play();
      if (playing && playing.catch) playing.catch(() => {});
      render();
    } catch (error) {
      const name = error && error.name;
      if (name === "NotAllowedError" || name === "SecurityError") useInput(texts.denied);
      else if (name === "NotFoundError" || name === "OverconstrainedError") useInput(texts.missing);
      else useInput(texts.failed);
    }
  }

  render();
  start();
}
