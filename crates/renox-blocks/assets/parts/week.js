// renox-blocks: week_calendar. Keyboard navigation for the grid, with one
// Tab stop (a roving tabindex): left and right move between a row's slots
// and bookings, up and down to the same time in the row above or below,
// Home and End to the row's first and last, Ctrl+Home and Ctrl+End to the
// grid's. Enter or Space on a free slot clicks it (htmx listens for the
// click and books it), on a booking it follows the booking's link. Nothing
// moves, so prefers-reduced-motion needs nothing here. In a right-to-left
// page left and right swap.

const CELL = "[role=gridcell]";

function rowsOf(grid) {
  return Array.from(grid.querySelectorAll("[data-rx-week-row]"));
}

function cellsOf(row) {
  return Array.from(row.querySelectorAll(CELL));
}

/** Makes `cell` the grid's one Tab stop, and focuses it when asked. */
function go(grid, cell, focus) {
  if (!cell) return;
  grid.querySelectorAll(`${CELL}[tabindex="0"]`).forEach((other) => {
    if (other !== cell) other.setAttribute("tabindex", "-1");
  });
  cell.setAttribute("tabindex", "0");
  if (focus) cell.focus();
}

/** The cell of `row` nearest, across, to `from`'s middle. */
function nearest(row, from) {
  const cells = cellsOf(row);
  if (!cells.length) return null;
  const box = from.getBoundingClientRect();
  const middle = box.left + box.width / 2;
  let best = null;
  let gap = Infinity;
  for (const cell of cells) {
    const rect = cell.getBoundingClientRect();
    const distance =
      middle >= rect.left && middle <= rect.right ? 0 : Math.min(Math.abs(middle - rect.left), Math.abs(middle - rect.right));
    if (distance < gap) {
      gap = distance;
      best = cell;
    }
  }
  return best;
}

function activate(cell) {
  const link = cell.querySelector("a[href]");
  (link || cell).click();
}

export function setup(root, kit) {
  if (!kit.claim(root)) return;
  const grid = root.querySelector("[role=grid]");
  if (!grid) return;
  const rtl = () => getComputedStyle(grid).direction === "rtl";

  grid.addEventListener("focusin", (event) => {
    const cell = event.target.closest && event.target.closest(CELL);
    if (cell && grid.contains(cell)) go(grid, cell, false);
  });

  grid.addEventListener("keydown", (event) => {
    const cell = event.target.closest && event.target.closest(CELL);
    if (!cell || event.target !== cell || event.altKey || event.metaKey) return;
    const row = cell.closest("[data-rx-week-row]");
    const cells = cellsOf(row);
    const at = cells.indexOf(cell);
    let next = null;
    switch (event.key) {
      case "ArrowRight":
        next = cells[at + (rtl() ? -1 : 1)];
        break;
      case "ArrowLeft":
        next = cells[at + (rtl() ? 1 : -1)];
        break;
      case "ArrowDown":
      case "ArrowUp": {
        const rows = rowsOf(grid);
        const target = rows[rows.indexOf(row) + (event.key === "ArrowDown" ? 1 : -1)];
        next = target && nearest(target, cell);
        break;
      }
      case "Home":
      case "End": {
        const rows = rowsOf(grid);
        const first = event.key === "Home";
        const row_ = event.ctrlKey ? rows[first ? 0 : rows.length - 1] : row;
        const found = cellsOf(row_);
        next = found[first ? 0 : found.length - 1];
        break;
      }
      case "Enter":
      case " ":
        event.preventDefault();
        activate(cell);
        return;
      default:
        return;
    }
    event.preventDefault();
    go(grid, next, true);
  });
}
