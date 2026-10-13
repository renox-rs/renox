// renox-blocks: kanban. Cards move between columns with a pointer (a mouse
// anywhere on the card, a finger or a pen by its handle, so a swipe still
// scrolls the page) or the keyboard (Space/Enter picks up and drops, the
// arrow keys move, Escape puts back). Each move is announced, then sent to
// the server through the board's hidden htmx form; a refused move puts the
// card back. The card glides into place unless less motion is asked for.

const pending = new WeakMap();
let kit;

function cardsOf(list) {
  return Array.from(list.children).filter((el) => el.hasAttribute("data-rx-kanban-card"));
}

/** Where a card is: its column, its title, its place (from 1) of how many. */
function placeOf(card) {
  const column = card.closest("[data-rx-kanban-column]");
  const cards = cardsOf(card.parentElement);
  const title = card.querySelector("[data-rx-kanban-title]");
  return {
    card: title ? title.textContent.trim() : card.getAttribute("data-rx-kanban-card"),
    column: column ? column.getAttribute("data-rx-kanban-column-title") : "",
    key: column ? column.getAttribute("data-rx-kanban-column") : "",
    position: cards.indexOf(card) + 1,
    total: cards.length,
  };
}

function announce(board, text) {
  const live = board.querySelector("[data-rx-kanban-live]");
  if (live) live.textContent = text;
}

function counts(board) {
  board.querySelectorAll("[data-rx-kanban-column]").forEach((column) => {
    const badge = column.querySelector("[data-rx-kanban-count]");
    const list = column.querySelector("[data-rx-kanban-list]");
    if (badge && list) badge.textContent = String(cardsOf(list).length);
  });
}

function prepareCards(board) {
  const help = board.getAttribute("aria-describedby");
  board.querySelectorAll("[data-rx-kanban-card]").forEach((card) => {
    if (!kit.claim(card)) return;
    card.tabIndex = 0;
    if (help) card.setAttribute("aria-describedby", help);
  });
}

function restore(move) {
  const next = move.origin.next;
  if (next && next.parentElement === move.origin.list) move.origin.list.insertBefore(move.card, next);
  else move.origin.list.appendChild(move.card);
}

/** The card was dropped: tell the server where, unless it didn't move. */
function commit(board, card, origin) {
  counts(board);
  const place = placeOf(card);
  announce(board, kit.fill(kit.texts(board).dropped, place));
  if (card.parentElement === origin.list && card.nextElementSibling === origin.next) return;
  const form = board.querySelector("[data-rx-kanban-form]");
  if (!form || !window.htmx) return;
  const move = {
    card,
    origin,
    id: card.getAttribute("data-rx-kanban-card"),
    column: place.key,
    position: place.position - 1,
  };
  form.elements.card.value = move.id;
  form.elements.column.value = move.column;
  form.elements.position.value = String(move.position);
  pending.get(board).push(move);
  window.htmx.trigger(form, "rx:kanban-move");
}

// The keyboard: the card picked up, if any.
let grabbed = null;

function release(card) {
  card.removeAttribute("aria-grabbed");
  grabbed = null;
}

function onKey(event) {
  const card = event.target.closest && event.target.closest("[data-rx-kanban-card]");
  const board = card && card.closest("[data-rx-kanban][data-rx-blocks-ready]");
  if (!board || event.target !== card || event.ctrlKey || event.metaKey || event.altKey) return;
  const t = kit.texts(board);
  const list = card.parentElement;
  const lists = Array.from(board.querySelectorAll("[data-rx-kanban-list]"));
  const column = lists.indexOf(list);
  const siblings = cardsOf(list);
  const at = siblings.indexOf(card);
  const holding = grabbed && grabbed.card === card;

  if (event.key === " " || event.key === "Enter") {
    event.preventDefault();
    if (holding) {
      const origin = grabbed.origin;
      release(card);
      commit(board, card, origin);
    } else {
      if (grabbed) {
        restore(grabbed);
        release(grabbed.card);
      }
      grabbed = { card, origin: { list, next: card.nextElementSibling } };
      card.setAttribute("aria-grabbed", "true");
      announce(board, kit.fill(t.picked, placeOf(card)));
    }
    return;
  }

  if (holding && event.key === "Escape") {
    event.preventDefault();
    const back = card.getBoundingClientRect();
    restore(grabbed);
    release(card);
    card.focus();
    kit.glide(card, back);
    counts(board);
    announce(board, kit.fill(t.cancelled, placeOf(card)));
    return;
  }
  if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
  event.preventDefault();

  if (!holding) {
    // Not holding it: the arrows move the focus between cards.
    let target = null;
    if (event.key === "ArrowUp") target = siblings[at - 1];
    else if (event.key === "ArrowDown") target = siblings[at + 1];
    else {
      const other = lists[column + (event.key === "ArrowLeft" ? -1 : 1)];
      if (other) {
        const cards = cardsOf(other);
        target = cards[Math.min(at, cards.length - 1)];
      }
    }
    if (target) target.focus();
    return;
  }

  // Holding it: the arrows move the card.
  const before = card.getBoundingClientRect();
  let moved = false;
  if (event.key === "ArrowUp" && at > 0) {
    list.insertBefore(card, siblings[at - 1]);
    moved = true;
  } else if (event.key === "ArrowDown" && at < siblings.length - 1) {
    list.insertBefore(card, siblings[at + 1].nextSibling);
    moved = true;
  } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
    const to = lists[column + (event.key === "ArrowLeft" ? -1 : 1)];
    if (to) {
      const spot = cardsOf(to)[Math.min(at, cardsOf(to).length)];
      if (spot) to.insertBefore(card, spot);
      else to.appendChild(card);
      moved = true;
    }
  }
  if (!moved) return;
  card.focus();
  if (card.scrollIntoView) card.scrollIntoView({ block: "nearest", inline: "nearest" });
  kit.glide(card, before);
  counts(board);
  announce(board, kit.fill(t.moved, placeOf(card)));
}

// A pointer: a gap opens where the card would land.
let drag = null;

function onDown(event) {
  const card = event.target.closest && event.target.closest("[data-rx-kanban-card]");
  const board = card && card.closest("[data-rx-kanban][data-rx-blocks-ready]");
  if (!board || event.button !== 0 || event.target.closest("a, button, input, select, textarea")) return;
  if (event.pointerType !== "mouse" && !event.target.closest(".rx-kanban__grip")) return;
  drag = {
    card,
    board,
    id: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    started: false,
    origin: { list: card.parentElement, next: card.nextElementSibling },
  };
}

function startDrag() {
  const card = drag.card;
  const place = placeOf(card);
  const box = card.getBoundingClientRect();
  drag.gap = document.createElement("li");
  drag.gap.className = "rx-kanban__gap";
  drag.gap.setAttribute("aria-hidden", "true");
  drag.gap.style.height = `${box.height}px`;
  card.parentElement.insertBefore(drag.gap, card);
  card.setAttribute("data-rx-kanban-dragging", "");
  Object.assign(card.style, { position: "fixed", left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, margin: "0" });
  // Out of the board while it moves, so nothing the board is inside of
  // (a scrolling frame, a transform) clips it or shifts it.
  document.body.appendChild(card);
  drag.started = true;
  if (grabbed) {
    restore(grabbed);
    release(grabbed.card);
  }
  announce(drag.board, kit.fill(kit.texts(drag.board).picked, place));
}

function onMove(event) {
  if (!drag || event.pointerId !== drag.id) return;
  const dx = event.clientX - drag.x;
  const dy = event.clientY - drag.y;
  if (!drag.started) {
    if (Math.abs(dx) + Math.abs(dy) < 6) return;
    startDrag();
  }
  event.preventDefault();
  drag.card.style.transform = `translate(${dx}px, ${dy}px)`;
  // The column under the pointer, and the card the gap goes before.
  let column = null;
  for (const el of document.elementsFromPoint(event.clientX, event.clientY)) {
    if (drag.card.contains(el)) continue;
    column = el.closest("[data-rx-kanban-column]");
    if (column) break;
  }
  drag.board.querySelectorAll("[data-rx-kanban-over]").forEach((c) => {
    if (c !== column) c.removeAttribute("data-rx-kanban-over");
  });
  if (!column || !drag.board.contains(column)) return;
  column.setAttribute("data-rx-kanban-over", "");
  const list = column.querySelector("[data-rx-kanban-list]");
  const spot = cardsOf(list).find((card) => {
    const r = card.getBoundingClientRect();
    return event.clientY < r.top + r.height / 2;
  });
  if (spot !== drag.gap.nextElementSibling || list !== drag.gap.parentElement) {
    if (spot) list.insertBefore(drag.gap, spot);
    else list.appendChild(drag.gap);
  }
  // Near the board's edges, it scrolls (columns off screen on a phone).
  const edge = drag.board.getBoundingClientRect();
  if (event.clientX > edge.right - 40) drag.board.scrollLeft += 12;
  else if (event.clientX < edge.left + 40) drag.board.scrollLeft -= 12;
}

function endDrag(event, cancelled) {
  if (!drag || event.pointerId !== drag.id) return;
  const done = drag;
  drag = null;
  if (!done.started) return;
  const card = done.card;
  const before = card.getBoundingClientRect();
  done.board.querySelectorAll("[data-rx-kanban-over]").forEach((c) => c.removeAttribute("data-rx-kanban-over"));
  if (cancelled) {
    done.gap.remove();
    restore({ card, origin: done.origin });
  } else {
    done.gap.parentElement.replaceChild(card, done.gap);
  }
  card.removeAttribute("data-rx-kanban-dragging");
  for (const p of ["position", "left", "top", "width", "margin", "transform"]) card.style[p] = "";
  if (!card.getAttribute("style")) card.removeAttribute("style");
  kit.glide(card, before);
  card.focus({ preventScroll: true });
  if (cancelled) {
    counts(done.board);
    announce(done.board, kit.fill(kit.texts(done.board).cancelled, placeOf(card)));
  } else {
    commit(done.board, card, done.origin);
  }
}

export function start(shared) {
  kit = shared;
  document.addEventListener("keydown", onKey);
  document.addEventListener("pointerdown", onDown);
  document.addEventListener("pointermove", onMove);
  document.addEventListener("pointerup", (event) => endDrag(event, false));
  document.addEventListener("pointercancel", (event) => endDrag(event, true));
}

export function setup(board) {
  // Cards swapped into a board that is already set up get ready too.
  prepareCards(board);
  if (!kit.claim(board)) return;
  pending.set(board, []);
  const form = board.querySelector("[data-rx-kanban-form]");
  if (!form) return;
  // The server's answer: keep the move, or put the card back.
  form.addEventListener("htmx:afterRequest", (event) => {
    const move = pending.get(board).shift();
    if (!move) return;
    const ok = !!event.detail.successful;
    if (!ok) {
      const before = move.card.getBoundingClientRect();
      restore(move);
      kit.glide(move.card, before);
      counts(board);
      announce(board, kit.fill(kit.texts(board).failed, placeOf(move.card)));
    }
    board.dispatchEvent(
      new CustomEvent("rx:kanban-moved", {
        bubbles: true,
        detail: { card: move.id, column: move.column, position: move.position, ok },
      }),
    );
  });
}
