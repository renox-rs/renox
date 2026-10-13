// renox-blocks: gallery. The arrows, the thumbnails, a swipe or the keyboard
// change the photo; the track slides with a Web Animation on `transform`
// and jumps under prefers-reduced-motion. "Enlarge" opens the kit's sheet
// (data-rx-open) with the current photo.

export function setup(gallery, kit) {
  const track = gallery.querySelector("[data-rx-gallery-track]");
  const viewport = gallery.querySelector("[data-rx-gallery-viewport]");
  const slides = track ? Array.from(track.children) : [];
  if (!slides.length || !kit.claim(gallery)) return;
  const prev = gallery.querySelector("[data-rx-gallery-prev]");
  const next = gallery.querySelector("[data-rx-gallery-next]");
  const status = gallery.querySelector("[data-rx-gallery-status]");
  const thumbs = gallery.querySelectorAll("[data-rx-gallery-thumb]");
  if (prev) prev.hidden = false;
  if (next) next.hidden = false;
  viewport.scrollLeft = 0;
  let index = 0;
  let sliding = null;

  const at = (n) => `translateX(${-n * viewport.clientWidth}px)`;

  // At rest the track sits at the current photo, in pixels, and follows
  // the gallery's width when the window changes.
  window.addEventListener("resize", () => {
    track.style.transform = at(index);
  });

  function show(target, from) {
    index = Math.max(0, Math.min(slides.length - 1, target));
    // Where the track is now (mid-slide included), in pixels.
    let start = from;
    if (start === undefined) start = new DOMMatrixReadOnly(getComputedStyle(track).transform).m41;
    if (sliding) sliding.cancel();
    track.style.transform = at(index);
    sliding = kit.animate(track, { transform: [`translateX(${start}px)`, at(index)] }, 420);
    slides.forEach((slide, n) => {
      const current = n === index;
      slide.toggleAttribute("inert", !current);
      if (current) slide.removeAttribute("aria-hidden");
      else slide.setAttribute("aria-hidden", "true");
    });
    thumbs.forEach((thumb, n) => {
      if (n === index) thumb.setAttribute("aria-current", "true");
      else thumb.removeAttribute("aria-current");
    });
    if (prev) prev.disabled = index === 0;
    if (next) next.disabled = index === slides.length - 1;
    if (status) {
      const slide = slides[index];
      status.textContent = `${slide.getAttribute("aria-label")}: ${slide.getAttribute("data-alt")}`;
    }
    gallery.setAttribute("data-rx-gallery-index", String(index));
  }

  gallery.addEventListener("click", (event) => {
    const thumb = event.target.closest("[data-rx-gallery-thumb]");
    if (thumb) {
      event.preventDefault();
      show(parseInt(thumb.getAttribute("data-rx-gallery-thumb"), 10));
      return;
    }
    if (event.target.closest("[data-rx-gallery-prev]")) show(index - 1);
    else if (event.target.closest("[data-rx-gallery-next]")) show(index + 1);
    else if (event.target.closest("[data-rx-gallery-enlarge]")) {
      // The kit opens the sheet; it shows the current photo.
      const sheet = document.getElementById(`${gallery.id}-zoom`);
      const slide = slides[index];
      if (!sheet) return;
      const image = sheet.querySelector("[data-rx-gallery-zoom]");
      const caption = sheet.querySelector("[data-rx-gallery-zoom-caption]");
      if (image) {
        image.src = slide.getAttribute("data-large");
        image.alt = slide.getAttribute("data-alt");
        kit.animate(image, { opacity: [0, 1], transform: ["scale(0.96)", "scale(1)"] }, 300);
      }
      if (caption) caption.textContent = slide.getAttribute("data-caption") || "";
    }
  });

  gallery.addEventListener("keydown", (event) => {
    const keys = { ArrowLeft: index - 1, ArrowRight: index + 1, Home: 0, End: slides.length - 1 };
    if (!(event.key in keys) || event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    show(keys[event.key]);
    // A thumbnail with the focus follows the photo.
    const focused = document.activeElement;
    if (focused && focused.hasAttribute("data-rx-gallery-thumb") && thumbs[index]) thumbs[index].focus();
  });

  // Swipes and drags: the track follows the finger, and a pull of a fifth
  // of the width (or a flick) goes to the next photo.
  let drag = null;
  viewport.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    drag = { id: event.pointerId, x: event.clientX, at: -index * viewport.clientWidth, dx: 0, time: Date.now(), moved: false };
  });
  viewport.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    drag.dx = event.clientX - drag.x;
    if (!drag.moved && Math.abs(drag.dx) < 6) return;
    if (!drag.moved) {
      drag.moved = true;
      if (sliding) sliding.cancel();
      try {
        viewport.setPointerCapture(event.pointerId);
      } catch (_) {
        // The pointer is gone already.
      }
    }
    const edge = (index === 0 && drag.dx > 0) || (index === slides.length - 1 && drag.dx < 0);
    track.style.transform = `translateX(${drag.at + (edge ? drag.dx / 3 : drag.dx)}px)`;
  });
  function release(event) {
    if (!drag || event.pointerId !== drag.id) return;
    const done = drag;
    drag = null;
    if (!done.moved) return;
    const width = viewport.clientWidth;
    const fast = Math.abs(done.dx) / Math.max(1, Date.now() - done.time) > 0.5;
    const step = Math.abs(done.dx) > width / 5 || fast ? (done.dx < 0 ? 1 : -1) : 0;
    show(index + step, done.at + done.dx);
  }
  viewport.addEventListener("pointerup", release);
  viewport.addEventListener("pointercancel", release);

  show(0, 0);
}
