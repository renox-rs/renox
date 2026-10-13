// Renox data grid (renox/grid.html): columns per screen size, frozen
// columns, the column menu, popovers, the date range calendar, and the
// form that asks the server for filtered, sorted pages. Plain DOM, no
// inline handlers (works under a strict CSP), keyboard-usable.
(function () {
  "use strict";
  if (window.RenoxGrid) return;

  var compact = window.matchMedia("(max-width: 767px)");
  var reopen = null; // the column menu to open again after a reload

  function csrf() {
    var meta = document.querySelector('meta[name="csrf-token"]');
    return meta ? meta.content : "";
  }

  function emit(el, name, detail) { var c = el && el.getAttribute && el.getAttribute("data-rx-component"); if (!c) return; el.dispatchEvent(new CustomEvent("rx:" + c + ":" + name, { bubbles: true, detail: detail || {} })); }

  function config(grid) {
    if (!grid._rx) {
      try { grid._rx = JSON.parse(grid.getAttribute("data-rx-grid")); } catch (_) { grid._rx = {}; }
    }
    return grid._rx;
  }

  function screen() { return compact.matches ? "compact" : "wide"; }

  function shownSet(grid) {
    var cfg = config(grid);
    return new Set(cfg[screen()] || cfg.order || []);
  }

  function cellsOf(grid, key) {
    return grid.querySelectorAll('.rx-grid__table [data-col="' + CSS.escape(key) + '"]');
  }

  // Shows the columns picked for this screen size, fixes the grouped
  // headings' spans, then places the frozen columns.
  function applyVisibility(grid) {
    var shown = shownSet(grid);
    grid.querySelectorAll(".rx-grid__table [data-col]").forEach(function (cell) {
      cell.classList.toggle("rx-grid__off", !shown.has(cell.getAttribute("data-col")));
    });
    grid.querySelectorAll(".rx-grid__table [data-cols]").forEach(function (th) {
      var n = th.getAttribute("data-cols").split(" ").filter(function (k) { return shown.has(k); }).length;
      th.colSpan = Math.max(n, 1);
      th.classList.toggle("rx-grid__off", n === 0);
    });
    var extra = grid.querySelector("thead th[data-tools]") ? 1 : 0;
    grid.querySelectorAll("[data-grid-span]").forEach(function (span) {
      span.colSpan = Math.max(shown.size, 1) + extra;
    });
    // Open details are rebuilt for the new columns.
    rowsOf(grid).forEach(function (row) {
      if (row._rxDetail) { closeDetails(grid, row); openDetails(grid, row); }
    });
    layoutPins(grid);
  }

  // Frozen columns stick at an offset: the widths of the frozen columns
  // before them.
  function layoutPins(grid) {
    var cfg = config(grid);
    var shown = shownSet(grid);
    grid.querySelectorAll(".rx-grid__edge-left, .rx-grid__edge-right").forEach(function (el) {
      el.classList.remove("rx-grid__edge-left", "rx-grid__edge-right");
    });
    var offsets = {};
    function width(key) {
      var th = grid.querySelector('thead th[data-col="' + CSS.escape(key) + '"]');
      return th ? th.getBoundingClientRect().width : 0;
    }
    function place(keys, side, start) {
      var at = start || 0, last = null;
      keys.forEach(function (key) {
        if (!shown.has(key)) return;
        offsets[key] = at;
        cellsOf(grid, key).forEach(function (cell) {
          cell.style[side] = at + "px";
          cell.style[side === "left" ? "right" : "left"] = "";
        });
        at += width(key);
        last = key;
      });
      if (last) cellsOf(grid, last).forEach(function (cell) { cell.classList.add("rx-grid__edge-" + side); });
    }
    var tools = grid.querySelectorAll(".rx-grid__table [data-tools]");
    var toolsWidth = 0;
    if (tools.length) {
      toolsWidth = tools[0].getBoundingClientRect().width;
      tools.forEach(function (cell) { cell.style.left = "0px"; });
    }
    place(cfg.left || [], "left", toolsWidth);
    if (tools.length && !(cfg.left || []).some(function (k) { return shown.has(k); })) {
      tools.forEach(function (cell) { cell.classList.add("rx-grid__edge-left"); });
    }
    place((cfg.right || []).slice().reverse(), "right");
    grid.querySelectorAll(".rx-grid__table th[data-cols][data-pin]").forEach(function (th) {
      var side = th.getAttribute("data-pin");
      var keys = th.getAttribute("data-cols").split(" ").filter(function (k) { return k in offsets; });
      if (!keys.length) return;
      th.style[side] = Math.min.apply(null, keys.map(function (k) { return offsets[k]; })) + "px";
    });
    edges(grid);
  }

  // Shadows on the frozen edges once rows scroll under them.
  function edges(grid) {
    var scroll = grid.querySelector(".rx-grid__scroll");
    if (!scroll) return;
    var max = scroll.scrollWidth - scroll.clientWidth;
    var x = Math.abs(scroll.scrollLeft);
    grid.classList.toggle("rx-grid--scrolled-left", x > 1);
    grid.classList.toggle("rx-grid--scrolled-right", x < max - 1);
  }

  // ---------- The form: sort, pages, filters ----------

  function setState(grid, name, value) {
    var input = grid.querySelector('[data-grid-state="' + name + '"]');
    if (input) input.value = value;
  }

  function submit(grid, keepPage) {
    if (!keepPage) setState(grid, "page", "1");
    if (grid.requestSubmit) grid.requestSubmit(); else grid.submit();
  }

  // Empty fields and defaults stay out of the URL.
  function trim(grid) {
    var disabled = [];
    var prefix = config(grid).prefix || "";
    grid.querySelectorAll("[name]").forEach(function (el) {
      if (el.disabled || el.closest(".rx-grid__editor")) return;
      var full = el.name, drop = false;
      var name = full.indexOf(prefix) === 0 ? full.slice(prefix.length) : full;
      if ((el.type === "checkbox" || el.type === "radio") && !el.checked) return;
      if (el.value === "") drop = true;
      if (name === "page" && el.value === "1") drop = true;
      if (name === "match" && el.value === "all") drop = true;
      if (name === "per_page" && el.value === el.getAttribute("data-default")) drop = true;
      if (name.indexOf("m.") === 0) {
        var text = grid.querySelector('[name="' + CSS.escape(prefix + "q." + name.slice(2)) + '"]');
        if (!text || !text.value.trim() || el.value === "contains") drop = true;
      }
      if (drop) { el.disabled = true; disabled.push(el); }
    });
    grid._rxDisabled = disabled;
  }

  // A grid with a prefix shares the page with others: their values in its
  // links and form come from the address as it is now, not as it was when
  // this grid was drawn (the others may have paged or filtered since).
  function syncKept(grid) {
    var prefix = config(grid).prefix || "";
    if (!prefix) return;
    grid.querySelectorAll("input[data-grid-keep]").forEach(function (el) { el.remove(); });
    var at = grid.querySelector("[data-grid-state]");
    new URLSearchParams(window.location.search).forEach(function (value, name) {
      if (name.indexOf(prefix) === 0) return;
      var input = document.createElement("input");
      input.type = "hidden";
      input.name = name;
      input.value = value;
      input.setAttribute("data-grid-keep", "");
      grid.insertBefore(input, at);
    });
  }

  function untrim(grid) {
    (grid._rxDisabled || []).forEach(function (el) { el.disabled = false; });
    grid._rxDisabled = [];
  }

  document.addEventListener("submit", function (event) {
    var grid = event.target.closest && event.target.closest("form.rx-grid");
    if (!grid || event.target !== grid) return;
    var cause = grid._rxCause || (grid._rxKeepPage ? "page" : "filter");
    grid._rxCause = null;
    if (!grid._rxKeepPage) setState(grid, "page", "1");
    if (grid._rxFocusSearch) { window._rxSearchFocus = grid.id; grid._rxFocusSearch = false; }
    grid._rxKeepPage = false;
    syncKept(grid);
    trim(grid);
    if (cause === "sort") emit(grid, "sorted", { sort: grid.querySelector('[data-grid-state="sort"]').value });
    else if (cause === "filter") emit(grid, "filtered", { query: new URLSearchParams(new FormData(grid)).toString() });
    // Without htmx the browser submits the form itself; give the fields back after.
    if (!window.htmx) setTimeout(function () { untrim(grid); }, 0);
  }, true);

  document.addEventListener("htmx:afterRequest", function (event) {
    var grid = event.detail.elt;
    if (grid && grid.classList && grid.classList.contains("rx-grid")) untrim(grid);
  });

  document.addEventListener("click", function (event) {
    var search = event.target.closest && event.target.closest("[data-grid-clear-search]");
    if (search) {
      var g = search.closest("form.rx-grid");
      g.querySelector("[data-grid-search]").value = "";
      submit(g);
      return;
    }
    var target = event.target.closest && event.target.closest("[data-grid-sort], [data-grid-page], [data-grid-clear], [data-grid-clear-all], [data-grid-move], [data-grid-reset]");
    if (!target) return;
    var grid = target.closest("form.rx-grid");
    if (!grid) return;
    if (target.hasAttribute("data-grid-sort")) {
      setState(grid, "sort", target.getAttribute("data-grid-sort"));
      grid._rxCause = "sort";
      submit(grid);
    } else if (target.hasAttribute("data-grid-page")) {
      setState(grid, "page", target.getAttribute("data-grid-page"));
      grid._rxKeepPage = true;
      submit(grid, true);
    } else if (target.hasAttribute("data-grid-clear")) {
      var pop = target.closest("[data-grid-filter]");
      clearFields(pop);
      submit(grid);
    } else if (target.hasAttribute("data-grid-clear-all")) {
      grid.querySelectorAll("[data-grid-filter]").forEach(clearFields);
      grid.querySelectorAll("[data-grid-rules] [data-grid-rule]").forEach(function (li) { li.remove(); });
      var box = grid.querySelector("[data-grid-search]");
      if (box) box.value = "";
      submit(grid);
    } else if (target.hasAttribute("data-grid-move")) {
      move(grid, target.closest("[data-grid-pick]").getAttribute("data-grid-pick"), parseInt(target.getAttribute("data-grid-move"), 10));
    } else if (target.hasAttribute("data-grid-reset")) {
      reset(grid);
    }
  });

  function clearFields(pop) {
    if (!pop) return;
    pop.querySelectorAll("input, select").forEach(function (el) {
      if (el.type === "checkbox") el.checked = false;
      else if (el.tagName === "SELECT") el.selectedIndex = 0;
      else el.value = "";
    });
    var range = pop.querySelector("calendar-range");
    if (range) range.removeAttribute("value");
  }

  document.addEventListener("change", function (event) {
    var el = event.target;
    var grid = el.closest && el.closest("form.rx-grid");
    if (!grid) return;
    if (el.hasAttribute("data-grid-per-page") || el.hasAttribute("data-grid-autosubmit")) {
      submit(grid);
    } else if (el.hasAttribute("data-grid-toggle")) {
      toggle(grid, el.getAttribute("data-grid-toggle"), el.checked);
    } else if (el.hasAttribute("data-grid-pin")) {
      pin(grid, el.getAttribute("data-grid-pin"), el.value);
    } else if (el.hasAttribute("data-grid-date")) {
      var box = el.closest("[data-grid-filter]");
      var from = box.querySelector('[data-grid-date="from"]').value;
      var to = box.querySelector('[data-grid-date="to"]').value;
      var cal = box.querySelector("calendar-range");
      if (cal && from && to) { cal.value = from + "/" + to; cal.focusedDate = from; }
    }
  });

  // A range picked on the calendar fills the From and To fields. Cally's
  // `change` doesn't bubble, so it's heard on its way down (capture).
  document.addEventListener("change", function (event) {
    var el = event.target;
    if (!el.matches || !el.matches("calendar-range[data-grid-range]")) return;
    var parts = (el.value || "").split("/");
    var pop = el.closest("[data-grid-filter]");
    pop.querySelector('[data-grid-date="from"]').value = parts[0] || "";
    pop.querySelector('[data-grid-date="to"]').value = parts[1] || "";
  }, true);

  // ---------- The column menu ----------

  function prefs(cfg) {
    return { order: cfg.order, left: cfg.left, right: cfg.right, compact: cfg.compact, wide: cfg.wide, widths: cfg.widths || {} };
  }

  function save(grid, method) {
    var cfg = config(grid);
    return fetch(cfg.prefs, {
      method: method || "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf() },
      body: method === "DELETE" ? undefined : JSON.stringify(prefs(cfg))
    });
  }

  function reload(grid) {
    reopen = grid.id;
    grid._rxKeepPage = true;
    submit(grid, true);
  }

  function toggle(grid, key, on) {
    var cfg = config(grid);
    var list = cfg[screen()] || [];
    var set = new Set(list);
    if (on) set.add(key); else set.delete(key);
    cfg[screen()] = cfg.order.filter(function (k) { return set.has(k); });
    applyVisibility(grid);
    save(grid);
  }

  function pin(grid, key, side) {
    var cfg = config(grid);
    cfg.left = (cfg.left || []).filter(function (k) { return k !== key; });
    cfg.right = (cfg.right || []).filter(function (k) { return k !== key; });
    if (side === "left") cfg.left.push(key);
    if (side === "right") cfg.right.unshift(key);
    // Frozen columns are shown at their edge, in the menu's order.
    cfg.left = cfg.order.filter(function (k) { return cfg.left.indexOf(k) >= 0; });
    cfg.right = cfg.order.filter(function (k) { return cfg.right.indexOf(k) >= 0; });
    save(grid).then(function () { reload(grid); });
  }

  function move(grid, key, by) {
    var cfg = config(grid);
    var order = cfg.order.slice();
    var i = order.indexOf(key), j = i + by;
    if (i < 0 || j < 0 || j >= order.length) return;
    order.splice(i, 1);
    order.splice(j, 0, key);
    cfg.order = order;
    ["compact", "wide", "left", "right"].forEach(function (name) {
      var set = new Set(cfg[name] || []);
      cfg[name] = order.filter(function (k) { return set.has(k); });
    });
    save(grid).then(function () { reload(grid); });
  }

  function reset(grid) {
    save(grid, "DELETE").then(function () { reload(grid); });
  }

  function syncPicker(grid) {
    var shown = shownSet(grid);
    grid.querySelectorAll("[data-grid-toggle]").forEach(function (box) {
      box.checked = shown.has(box.getAttribute("data-grid-toggle"));
    });
  }

  // ---------- Popovers ----------

  function place(pop) {
    if (compact.matches) { pop.style.top = pop.style.left = ""; return; }
    var button = document.querySelector('[popovertarget="' + CSS.escape(pop.id) + '"]');
    if (!button) return;
    var r = button.getBoundingClientRect();
    var w = pop.offsetWidth, h = pop.offsetHeight;
    var left = r.left;
    if (left + w > window.innerWidth - 8) left = r.right - w;
    left = Math.max(8, Math.min(left, window.innerWidth - w - 8));
    var top = r.bottom + 6;
    if (top + h > window.innerHeight - 8) top = Math.max(8, r.top - h - 6);
    pop.style.left = left + "px";
    pop.style.top = top + "px";
  }

  document.addEventListener("toggle", function (event) {
    var pop = event.target;
    if (!pop.classList || !pop.classList.contains("rx-grid__pop") || event.newState !== "open") return;
    var grid = pop.closest("form.rx-grid");
    if (grid && pop.classList.contains("rx-grid__columns")) syncPicker(grid);
    var range = pop.querySelector("calendar-range");
    if (range) range.setAttribute("months", compact.matches ? "1" : "2");
    place(pop);
    // Again once the calendar has drawn its months.
    requestAnimationFrame(function () { place(pop); });
    var first = pop.querySelector("input:not([type=hidden]), select");
    if (first && !compact.matches) first.focus();
  }, true);

  // ---------- Row details ----------

  function visibleKeys(grid) {
    var shown = shownSet(grid);
    return (config(grid).order || []).filter(function (k) { return shown.has(k); });
  }

  // Cells of merged columns that span past `row` (they cover the next row).
  function spanning(row) {
    var next = row.nextElementSibling;
    while (next && next.hasAttribute("data-grid-detail")) next = next.nextElementSibling;
    var covered = next && next.getAttribute("data-covered");
    return covered ? covered.split(" ") : [];
  }

  function groupCell(grid, row, key) {
    // The cell starting the merged group that `row` belongs to.
    for (var r = row; r; r = r.previousElementSibling) {
      var cell = r.querySelector('td[data-col="' + CSS.escape(key) + '"]');
      if (cell) return cell;
    }
    return null;
  }

  function openDetails(grid, row) {
    var template = row.querySelector("template[data-grid-details]");
    if (!template || row._rxDetail) return;
    var detail = document.createElement("tr");
    detail.setAttribute("data-grid-detail", "");
    detail.className = "rx-grid__detail-row";
    var span = spanning(row);
    var shown = shownSet(grid);
    // Merged cells spanning past this row take one more row; the details fill
    // the columns between them, as one or more cells.
    var hasTools = !!grid.querySelector("thead th[data-tools]");
    var segments = [], current = hasTools ? 1 : 0;
    visibleKeys(grid).forEach(function (key) {
      if (span.indexOf(key) >= 0) {
        if (current) segments.push(current);
        current = 0;
      } else {
        current += 1;
      }
    });
    if (current) segments.push(current);
    span.forEach(function (key) {
      if (!shown.has(key)) return;
      var cell = groupCell(grid, row, key);
      if (cell) cell.rowSpan += 1;
    });
    // The details go in the widest run of columns, not in the row tools
    // alone (left of merged cells), and the later one on a tie.
    var widest = 0, best = -1;
    segments.forEach(function (n, i) {
      var toolsOnly = hasTools && i === 0 && n === 1 && segments.length > 1 && span.length;
      var size = toolsOnly ? 0 : n;
      if (size >= best) { best = size; widest = i; }
    });
    segments.forEach(function (n, i) {
      var td = document.createElement("td");
      td.colSpan = n;
      td.className = "rx-grid__detail";
      if (i === widest) td.appendChild(template.content.cloneNode(true));
      detail.appendChild(td);
    });
    row.after(detail);
    row._rxDetail = { el: detail, span: span.filter(function (k) { return shown.has(k); }) };
    row.classList.add("rx-grid__row--open");
    var button = row.querySelector("[data-grid-expand]");
    if (button) button.setAttribute("aria-expanded", "true");
  }

  function closeDetails(grid, row) {
    var open = row._rxDetail;
    if (!open) return;
    open.span.forEach(function (key) {
      var cell = groupCell(grid, row, key);
      if (cell && cell.rowSpan > 1) cell.rowSpan -= 1;
    });
    open.el.remove();
    row._rxDetail = null;
    row.classList.remove("rx-grid__row--open");
    var button = row.querySelector("[data-grid-expand]");
    if (button) button.setAttribute("aria-expanded", "false");
  }

  function toggleDetails(grid, row) {
    if (row._rxDetail) closeDetails(grid, row); else openDetails(grid, row);
  }

  // ---------- Editing ----------

  function columnOf(grid, key) { return (config(grid).columns || {})[key] || {}; }

  function rawValue(cell) {
    try { return JSON.parse(cell.getAttribute("data-raw")); } catch (_) { return null; }
  }

  function editor(grid, cell) {
    var key = cell.getAttribute("data-col");
    var col = columnOf(grid, key);
    var value = rawValue(cell);
    var wrap = document.createElement("span");
    wrap.className = "rx-grid__editor";
    var input;
    if (col.kind === "bool") {
      input = document.createElement("input");
      input.type = "checkbox";
      input.checked = !!value;
    } else if (col.kind === "select") {
      input = document.createElement("select");
      (col.options || []).forEach(function (o) {
        var opt = new Option(o[1], o[0], false, o[0] === value);
        input.appendChild(opt);
      });
    } else if (col.kind === "tags") {
      input = document.createElement("span");
      input.className = "rx-grid__editor-tags";
      input.setAttribute("role", "group");
      input.setAttribute("aria-label", col.label || key);
      (col.options || []).forEach(function (o) {
        var label = document.createElement("label");
        var box = document.createElement("input");
        box.type = "checkbox";
        box.value = o[0];
        box.checked = (value || []).indexOf(o[0]) >= 0;
        label.appendChild(box);
        label.appendChild(document.createTextNode(" " + o[1]));
        input.appendChild(label);
      });
    } else {
      input = document.createElement("input");
      input.type = { number: "number", money: "number", date: "date", date_time: "datetime-local" }[col.kind] || "text";
      if (input.type === "number") input.step = "any";
      var v = value == null ? "" : String(value);
      if (col.kind === "date_time") v = v.slice(0, 16);
      input.value = v;
    }
    input.classList.add("rx-grid__editor-input");
    input.setAttribute("data-grid-input", key);
    if (input.tagName !== "SPAN") {
      input.name = key;
      input.setAttribute("aria-label", col.label || key);
    }
    wrap.appendChild(input);
    return wrap;
  }

  function valueOf(grid, cell) {
    var key = cell.getAttribute("data-col");
    var col = columnOf(grid, key);
    var input = cell.querySelector("[data-grid-input]");
    if (!input) return undefined;
    if (col.kind === "bool") return input.checked ? "true" : "false";
    if (col.kind === "tags") {
      return Array.prototype.map.call(input.querySelectorAll("input:checked"), function (b) { return b.value; });
    }
    return input.value;
  }

  function startEdit(grid, cell) {
    if (cell._rxOriginal != null) return;
    cell._rxOriginal = cell.innerHTML;
    cell.innerHTML = "";
    cell.appendChild(editor(grid, cell));
    cell.classList.add("rx-grid__editing");
  }

  function stopEdit(cell) {
    if (cell._rxOriginal == null) return;
    cell.innerHTML = cell._rxOriginal;
    cell._rxOriginal = null;
    cell.classList.remove("rx-grid__editing");
  }

  // Sends the row's edited cells; the grid reloads its page when saved, and
  // a 422 shows the errors next to the fields (renox.js). (Not `save`: that
  // one stores the column preferences, and a second function declaration of
  // the same name would replace it.)
  function saveCells(grid, row, cells) {
    var url = row.getAttribute("data-edit");
    if (!url || !window.htmx) return;
    var values = {};
    cells.forEach(function (cell) {
      var v = valueOf(grid, cell);
      if (v !== undefined) values[cell.getAttribute("data-col")] = v;
    });
    row.setAttribute("aria-busy", "true");
    window.htmx.ajax("PATCH", url, { source: row, values: values, swap: "none" }).then(function () {
      row.removeAttribute("aria-busy");
    });
  }

  document.addEventListener("htmx:afterRequest", function (event) {
    var row = event.detail.elt;
    if (!row || !row.matches || !row.matches("tr[data-edit]")) return;
    var grid = row.closest("form.rx-grid");
    row.removeAttribute("aria-busy");
    if (event.detail.successful) {
      grid._rxKeepPage = true;
      submit(grid, true);
    }
  });

  function editRow(grid, row, on) {
    var cells = row.querySelectorAll("td[data-editable]");
    cells.forEach(function (cell) { if (on) startEdit(grid, cell); else stopEdit(cell); });
    row.classList.toggle("rx-grid__row--editing", on);
    row.querySelector("[data-grid-edit-row]").hidden = on;
    row.querySelector("[data-grid-save-row]").hidden = !on;
    row.querySelector("[data-grid-cancel-row]").hidden = !on;
    if (on && cells[0]) {
      var first = cells[0].querySelector("input, select");
      if (first) first.focus();
    }
  }

  function editCell(grid, cell) {
    var row = cell.closest("tr");
    if (row.classList.contains("rx-grid__row--editing")) return;
    startEdit(grid, cell);
    var input = cell.querySelector("input, select");
    if (input) { input.focus(); if (input.select && input.type === "text") input.select(); }
  }

  function commitCell(grid, cell) {
    if (cell._rxOriginal == null) return;
    var before = JSON.stringify(rawValue(cell));
    var now = valueOf(grid, cell);
    var col = columnOf(grid, cell.getAttribute("data-col"));
    var same = col.kind === "bool" ? (String(rawValue(cell)) === now)
      : col.kind === "tags" ? JSON.stringify(now) === before
      : String(rawValue(cell) == null ? "" : rawValue(cell)).slice(0, col.kind === "date_time" ? 16 : undefined) === now;
    if (same) { stopEdit(cell); return; }
    saveCells(grid, cell.closest("tr"), [cell]);
  }

  document.addEventListener("dblclick", function (event) {
    var cell = event.target.closest && event.target.closest("td[data-editable]");
    if (!cell) return;
    var grid = cell.closest("form.rx-grid");
    if (grid) editCell(grid, cell);
  });

  document.addEventListener("keydown", function (event) {
    var grid = event.target.closest && event.target.closest("form.rx-grid");
    if (!grid) return;
    var cell = event.target.closest("td[data-editable]");
    if (cell && event.target === cell && (event.key === "Enter" || event.key === "F2")) {
      event.preventDefault();
      editCell(grid, cell);
      return;
    }
    var input = event.target.closest("[data-grid-input], .rx-grid__editor-tags");
    if (cell && input) {
      var row = cell.closest("tr");
      var rowMode = row.classList.contains("rx-grid__row--editing");
      if (event.key === "Escape") {
        event.preventDefault();
        if (rowMode) editRow(grid, row, false); else { stopEdit(cell); cell.focus(); }
      } else if (event.key === "Enter" && event.target.type !== "checkbox") {
        // Enter saves instead of submitting the grid's filters.
        event.preventDefault();
        if (rowMode) saveCells(grid, row, Array.prototype.slice.call(row.querySelectorAll("td[data-editable]")));
        else commitCell(grid, cell);
      }
      return;
    }
    var grip = event.target.closest("[data-grid-drag]");
    if (grip && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      moveRow(grid, grip.closest("tr"), event.key === "ArrowUp" ? -1 : 1);
      grip.focus();
    }
  });

  document.addEventListener("focusout", function (event) {
    var cell = event.target.closest && event.target.closest("td.rx-grid__editing");
    if (!cell) return;
    var row = cell.closest("tr");
    if (row.classList.contains("rx-grid__row--editing")) return;
    var grid = cell.closest("form.rx-grid");
    // Leaving the cell (not moving between its own checkboxes) saves it.
    setTimeout(function () {
      if (!cell.contains(document.activeElement)) commitCell(grid, cell);
    }, 0);
  });

  // ---------- Rows in order ----------

  function rowsOf(grid) {
    return Array.prototype.slice.call(grid.querySelectorAll(".rx-grid__table tbody tr[data-id]"));
  }

  function saveOrder(grid) {
    var cfg = config(grid);
    if (!cfg.reorder) return;
    var body = new URLSearchParams();
    body.append("ids", rowsOf(grid).map(function (row) { return row.getAttribute("data-id"); }).join(","));
    body.append("offset", String(cfg.offset || 0));
    fetch(cfg.reorder, {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/x-www-form-urlencoded", "X-CSRF-Token": csrf() },
      body: body
    });
  }

  function moveRow(grid, row, by) {
    closeDetails(grid, row);
    var rows = rowsOf(grid), i = rows.indexOf(row), j = i + by;
    if (j < 0 || j >= rows.length) return;
    closeDetails(grid, rows[j]);
    if (by < 0) rows[j].before(row); else rows[j].after(row);
    saveOrder(grid);
  }

  var drag = null;
  document.addEventListener("pointerdown", function (event) {
    var grip = event.target.closest && event.target.closest("[data-grid-drag]:not(:disabled)");
    if (!grip) return;
    var grid = grip.closest("form.rx-grid");
    var row = grip.closest("tr");
    rowsOf(grid).forEach(function (r) { closeDetails(grid, r); });
    drag = { grid: grid, row: row, moved: false };
    row.classList.add("rx-grid__row--dragging");
    grip.setPointerCapture(event.pointerId);
    event.preventDefault();
  });
  document.addEventListener("pointermove", function (event) {
    if (!drag) return;
    var under = document.elementFromPoint(event.clientX, event.clientY);
    var target = under && under.closest && under.closest("tr[data-id]");
    if (!target || target === drag.row || target.closest("form.rx-grid") !== drag.grid) return;
    var r = target.getBoundingClientRect();
    if (event.clientY < r.top + r.height / 2) target.before(drag.row); else target.after(drag.row);
    drag.moved = true;
  });
  function endDrag() {
    if (!drag) return;
    drag.row.classList.remove("rx-grid__row--dragging");
    if (drag.moved) saveOrder(drag.grid);
    drag = null;
  }
  document.addEventListener("pointerup", endDrag);
  document.addEventListener("pointercancel", endDrag);

  document.addEventListener("click", function (event) {
    var grid = event.target.closest && event.target.closest("form.rx-grid");
    if (!grid) return;
    var t = event.target.closest("[data-grid-expand], [data-grid-edit-row], [data-grid-save-row], [data-grid-cancel-row]");
    if (t) {
      var row = t.closest("tr");
      if (t.hasAttribute("data-grid-expand")) toggleDetails(grid, row);
      else if (t.hasAttribute("data-grid-edit-row")) editRow(grid, row, true);
      else if (t.hasAttribute("data-grid-save-row")) saveCells(grid, row, Array.prototype.slice.call(row.querySelectorAll("td[data-editable]")));
      else editRow(grid, row, false);
      return;
    }
    // A click on a row (not on a control in it) opens its details.
    // A row with a link of its own opens it (Ctrl/Cmd: in a new tab).
    var linked = event.target.closest("tr[data-href]");
    if (linked && !event.target.closest("a, button, input, select, label, textarea, td.rx-grid__editing")) {
      if (window.getSelection && String(window.getSelection())) return;
      if (event.ctrlKey || event.metaKey) window.open(linked.getAttribute("data-href"), "_blank", "noopener");
      else window.location.assign(linked.getAttribute("data-href"));
      return;
    }
    var plain = event.target.closest("tr[data-grid-row]");
    if (plain && !event.target.closest("a, button, input, select, label, textarea, td.rx-grid__editing, td[data-editable]:focus")) {
      if (window.getSelection && String(window.getSelection())) return;
      toggleDetails(grid, plain);
    }
  });

  // The print page's button (no inline handlers under a strict CSP).
  document.addEventListener("click", function (event) {
    if (event.target.closest && event.target.closest("[data-grid-print]")) window.print();
  });

  // ---------- Column widths ----------

  function applyWidths(grid) {
    var widths = config(grid).widths || {};
    grid.querySelectorAll(".rx-grid__table [data-col]").forEach(function (cell) {
      var w = widths[cell.getAttribute("data-col")];
      cell.style.width = cell.style.minWidth = cell.style.maxWidth = w ? w + "px" : "";
      cell.classList.toggle("rx-grid__sized", !!w);
    });
  }

  function setWidth(grid, key, width) {
    var cfg = config(grid);
    cfg.widths = cfg.widths || {};
    if (width == null) delete cfg.widths[key];
    else cfg.widths[key] = Math.round(Math.max(40, Math.min(2000, width)));
    applyWidths(grid);
    layoutPins(grid);
  }

  function savePrefs(grid) {
    var cfg = config(grid);
    var body = prefs(cfg);
    body.widths = cfg.widths || {};
    return fetch(cfg.prefs, {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf() },
      body: JSON.stringify(body)
    });
  }

  var resizing = null;
  document.addEventListener("pointerdown", function (event) {
    var handle = event.target.closest && event.target.closest("[data-grid-resize]");
    if (!handle) return;
    var th = handle.closest("th[data-col]");
    var grid = handle.closest("form.rx-grid");
    resizing = { grid: grid, key: th.getAttribute("data-col"), x: event.clientX, width: th.getBoundingClientRect().width };
    grid.classList.add("rx-grid--resizing");
    handle.setPointerCapture(event.pointerId);
    event.preventDefault();
    event.stopPropagation();
  }, true);
  document.addEventListener("pointermove", function (event) {
    if (!resizing) return;
    var dir = getComputedStyle(resizing.grid).direction === "rtl" ? -1 : 1;
    setWidth(resizing.grid, resizing.key, resizing.width + (event.clientX - resizing.x) * dir);
  });
  function endResize() {
    if (!resizing) return;
    resizing.grid.classList.remove("rx-grid--resizing");
    savePrefs(resizing.grid);
    resizing = null;
  }
  document.addEventListener("pointerup", endResize);
  document.addEventListener("pointercancel", endResize);

  document.addEventListener("dblclick", function (event) {
    var handle = event.target.closest && event.target.closest("[data-grid-resize]");
    if (!handle) return;
    var grid = handle.closest("form.rx-grid");
    setWidth(grid, handle.closest("th").getAttribute("data-col"), null);
    savePrefs(grid);
  });

  document.addEventListener("keydown", function (event) {
    var handle = event.target.closest && event.target.closest("[data-grid-resize]");
    if (!handle || (event.key !== "ArrowLeft" && event.key !== "ArrowRight")) return;
    event.preventDefault();
    var grid = handle.closest("form.rx-grid");
    var th = handle.closest("th");
    var step = event.shiftKey ? 50 : 10;
    setWidth(grid, th.getAttribute("data-col"), th.getBoundingClientRect().width + (event.key === "ArrowRight" ? step : -step));
    clearTimeout(grid._rxSaveWidths);
    grid._rxSaveWidths = setTimeout(function () { savePrefs(grid); }, 400);
  });

  // ---------- Moving columns by their heading ----------

  var moving = null;
  document.addEventListener("pointerdown", function (event) {
    if (event.pointerType === "touch" || event.button !== 0) return;
    var th = event.target.closest && event.target.closest(".rx-grid__table thead th[data-col]");
    if (!th || event.target.closest("[data-grid-resize], .rx-grid__pop, .rx-grid__filter")) return;
    moving = { grid: th.closest("form.rx-grid"), th: th, x: event.clientX, y: event.clientY, active: false, target: null };
  });
  document.addEventListener("pointermove", function (event) {
    if (!moving) return;
    if (!moving.active) {
      if (Math.abs(event.clientX - moving.x) + Math.abs(event.clientY - moving.y) < 8) return;
      moving.active = true;
      moving.grid.classList.add("rx-grid--moving");
      moving.th.classList.add("rx-grid__moving");
    }
    var under = document.elementFromPoint(event.clientX, event.clientY);
    var target = under && under.closest && under.closest(".rx-grid__table thead th[data-col]");
    moving.grid.querySelectorAll(".rx-grid__drop-before, .rx-grid__drop-after").forEach(function (el) {
      el.classList.remove("rx-grid__drop-before", "rx-grid__drop-after");
    });
    moving.target = null;
    if (!target || target === moving.th || target.closest("form.rx-grid") !== moving.grid) return;
    var r = target.getBoundingClientRect();
    var after = event.clientX > r.left + r.width / 2;
    target.classList.add(after ? "rx-grid__drop-after" : "rx-grid__drop-before");
    moving.target = { key: target.getAttribute("data-col"), after: after };
  });
  document.addEventListener("pointerup", function () {
    if (!moving) return;
    var m = moving;
    moving = null;
    if (!m.active) return;
    m.grid.classList.remove("rx-grid--moving");
    m.th.classList.remove("rx-grid__moving");
    m.grid.querySelectorAll(".rx-grid__drop-before, .rx-grid__drop-after").forEach(function (el) {
      el.classList.remove("rx-grid__drop-before", "rx-grid__drop-after");
    });
    // The click that ends a drag doesn't sort.
    m.grid._rxNoClick = true;
    setTimeout(function () { m.grid._rxNoClick = false; }, 0);
    if (!m.target) return;
    var cfg = config(m.grid);
    var key = m.th.getAttribute("data-col");
    var order = cfg.order.filter(function (k) { return k !== key; });
    var at = order.indexOf(m.target.key) + (m.target.after ? 1 : 0);
    order.splice(at, 0, key);
    cfg.order = order;
    ["compact", "wide", "left", "right"].forEach(function (name) {
      var set = new Set(cfg[name] || []);
      cfg[name] = order.filter(function (k) { return set.has(k); });
    });
    savePrefs(m.grid).then(function () { m.grid._rxKeepPage = true; submit(m.grid, true); });
  });
  document.addEventListener("click", function (event) {
    var grid = event.target.closest && event.target.closest("form.rx-grid");
    if (grid && grid._rxNoClick) { event.preventDefault(); event.stopPropagation(); }
  }, true);

  // The search box asks as you type, after a pause.
  document.addEventListener("input", function (event) {
    var box = event.target.closest && event.target.closest("[data-grid-search]");
    if (!box) return;
    var grid = box.closest("form.rx-grid");
    clearTimeout(grid._rxSearch);
    grid._rxSearch = setTimeout(function () { grid._rxFocusSearch = true; submit(grid); }, 350);
  });

  document.addEventListener("keydown", function (event) {
    var row = event.target.matches && event.target.matches("tr[data-href]") ? event.target : null;
    if (row && event.key === "Enter") window.location.assign(row.getAttribute("data-href"));
  });

  // ---------- Selection, bulk and row actions ----------

  function selectedBoxes(grid) {
    return Array.prototype.slice.call(grid.querySelectorAll("[data-grid-select]:checked"));
  }

  function emitSelection(grid) {
    emit(grid, "selected", { ids: selectedBoxes(grid).map(function (b) { return b.value; }), all: !!grid._rxAllMatching });
  }

  function syncSelection(grid) {
    var bar = grid.querySelector("[data-grid-bulk]");
    if (!bar) return;
    var boxes = grid.querySelectorAll("[data-grid-select]");
    var picked = selectedBoxes(grid);
    var all = grid.querySelector("[data-grid-select-all]");
    if (all) {
      all.checked = boxes.length > 0 && picked.length === boxes.length;
      all.indeterminate = picked.length > 0 && picked.length < boxes.length;
    }
    if (!picked.length) grid._rxAllMatching = false;
    bar.hidden = picked.length === 0;
    var count = bar.querySelector("[data-grid-count]");
    var matching = bar.querySelector("[data-grid-select-matching]");
    var total = matching ? parseInt(matching.getAttribute("data-total"), 10) : 0;
    count.textContent = grid._rxAllMatching ? String(total) : String(picked.length);
    if (matching) matching.hidden = grid._rxAllMatching || picked.length !== boxes.length || total <= boxes.length;
    rowsOf(grid).forEach(function (row) {
      var box = row.querySelector("[data-grid-select]");
      row.classList.toggle("rx-grid__row--selected", !!(box && box.checked));
    });
  }

  // A question in the grid's dialog; resolves to true when confirmed.
  function ask(grid, question, danger) {
    var dialog = grid.querySelector("[data-grid-dialog]");
    if (!question || !dialog || !dialog.showModal) return Promise.resolve(true);
    dialog.querySelector("[data-grid-dialog-text]").textContent = question;
    var okButton = dialog.querySelector("[data-grid-dialog-ok]");
    okButton.classList.toggle("rx-button--danger", !!danger);
    okButton.classList.toggle("rx-button--primary", !danger);
    return new Promise(function (resolve) {
      function done(answer) {
        dialog.removeEventListener("close", onClose);
        dialog._rxAnswer = null;
        resolve(answer);
      }
      function onClose() { done(dialog.returnValue === "ok"); }
      dialog.returnValue = "";
      dialog.addEventListener("close", onClose);
      dialog.showModal();
      dialog.querySelector("[data-grid-dialog-ok]").focus();
    });
  }

  // An action that asks for input: its sheet (a kit `action_sheet` somewhere
  // on the page) is pointed at the action's address and given the selection,
  // then opened. The sheet's form is sent with htmx like any other.
  function openActionSheet(id, opener, url, values) {
    var dialog = document.getElementById(id);
    var form = dialog && dialog.querySelector("form");
    if (!dialog || !form || typeof dialog.showModal !== "function" || dialog.open) return;
    form.setAttribute("action", url);
    form.setAttribute("hx-post", url);
    form.querySelectorAll("input[data-grid-sheet-value]").forEach(function (input) { input.remove(); });
    Object.keys(values).forEach(function (name) {
      var input = document.createElement("input");
      input.type = "hidden";
      input.name = name;
      input.setAttribute("value", values[name]);
      input.setAttribute("data-grid-sheet-value", "");
      form.appendChild(input);
    });
    dialog._opener = opener;
    dialog.showModal();
    var first = dialog.querySelector("[autofocus]") ||
      dialog.querySelector(".rx-sheet__body :is(input:not([type=hidden]), select, textarea):not([disabled])");
    if (first) first.focus();
  }

  function send(grid, source, method, url, values) {
    if (!window.htmx) return;
    source.setAttribute("aria-busy", "true");
    window.htmx.ajax(method, url, { source: source, values: values || {}, swap: "none" }).then(function () {
      source.removeAttribute("aria-busy");
    });
  }

  document.addEventListener("htmx:afterRequest", function (event) {
    var el = event.detail.elt;
    if (!el || !el.matches || !el.matches("[data-grid-action], [data-grid-bulk-action]")) return;
    el.removeAttribute("aria-busy");
    var grid = el.closest("form.rx-grid");
    if (event.detail.successful && grid) {
      grid._rxKeepPage = true;
      submit(grid, true);
    }
  });

  document.addEventListener("change", function (event) {
    var el = event.target;
    var grid = el.closest && el.closest("form.rx-grid");
    if (!grid) return;
    if (el.hasAttribute("data-grid-select-all")) {
      grid.querySelectorAll("[data-grid-select]").forEach(function (b) { b.checked = el.checked; });
      grid._rxAllMatching = false;
      syncSelection(grid);
      emitSelection(grid);
    } else if (el.hasAttribute("data-grid-select")) {
      grid._rxAllMatching = false;
      syncSelection(grid);
      emitSelection(grid);
    }
  });

  document.addEventListener("click", function (event) {
    var t = event.target.closest && event.target.closest("[data-grid-select-matching], [data-grid-select-none], [data-grid-bulk-action], [data-grid-action], [data-grid-dialog-ok], [data-grid-dialog-cancel]");
    if (!t) return;
    var grid = t.closest("form.rx-grid");
    if (t.hasAttribute("data-grid-dialog-ok") || t.hasAttribute("data-grid-dialog-cancel")) {
      t.closest("dialog").close(t.hasAttribute("data-grid-dialog-ok") ? "ok" : "");
      return;
    }
    if (t.hasAttribute("data-grid-select-matching")) {
      grid._rxAllMatching = true;
      syncSelection(grid);
      emitSelection(grid);
    } else if (t.hasAttribute("data-grid-select-none")) {
      grid.querySelectorAll("[data-grid-select]").forEach(function (b) { b.checked = false; });
      syncSelection(grid);
      emitSelection(grid);
    } else if (t.hasAttribute("data-grid-bulk-action")) {
      var ids = selectedBoxes(grid).map(function (b) { return b.value; });
      var all = !!grid._rxAllMatching;
      // The grid's query string goes along, for "all matching".
      var url = t.getAttribute("data-url") + window.location.search;
      if (t.hasAttribute("data-sheet")) {
        openActionSheet(t.getAttribute("data-sheet"), t, url, { ids: ids.join(","), all: all ? "true" : "false" });
        return;
      }
      ask(grid, t.getAttribute("data-confirm"), t.hasAttribute("data-danger")).then(function (ok) {
        if (ok) send(grid, t, t.getAttribute("data-method") || "POST", url, { ids: ids.join(","), all: all ? "true" : "false" });
      });
    } else if (t.hasAttribute("data-grid-action")) {
      var pop = t.closest("[popover]");
      if (pop && pop.hidePopover) pop.hidePopover();
      if (t.hasAttribute("data-sheet")) {
        openActionSheet(t.getAttribute("data-sheet"), t, t.getAttribute("data-url"), {});
        return;
      }
      ask(grid, t.getAttribute("data-confirm"), t.hasAttribute("data-danger")).then(function (ok) {
        if (ok) send(grid, t, t.getAttribute("data-method") || "POST", t.getAttribute("data-url"));
      });
    }
  });

  // Folding a group hides its rows (and their open details).
  document.addEventListener("click", function (event) {
    var fold = event.target.closest && event.target.closest("[data-grid-fold]");
    if (!fold) return;
    var grid = fold.closest("form.rx-grid");
    var id = fold.closest("tr").getAttribute("data-grid-group");
    var open = fold.getAttribute("aria-expanded") === "true";
    fold.setAttribute("aria-expanded", open ? "false" : "true");
    grid.querySelectorAll('[data-in-group="' + CSS.escape(id) + '"]').forEach(function (row) {
      if (open && row._rxDetail) closeDetails(grid, row);
      row.hidden = open;
    });
  });

  // Cards: the toolbar's sort choice; any grid: copy buttons.
  document.addEventListener("change", function (event) {
    var pick = event.target.closest && event.target.closest("[data-grid-sort-pick]");
    if (!pick) return;
    var grid = pick.closest("form.rx-grid");
    setState(grid, "sort", pick.value);
    submit(grid);
  });

  document.addEventListener("click", function (event) {
    var copy = event.target.closest && event.target.closest("[data-grid-copy]");
    if (!copy) return;
    var text = copy.getAttribute("data-grid-copy");
    var done = function () {
      copy.classList.add("rx-grid__copy--done");
      setTimeout(function () { copy.classList.remove("rx-grid__copy--done"); }, 1200);
    };
    if (navigator.clipboard) navigator.clipboard.writeText(text).then(done, function () {});
  });

  // ---------- The advanced filter ----------

  var NO_VALUE = ["empty", "not_empty", "is_true", "is_false"];

  function ruleColumns(grid) {
    if (!grid._rxRuleColumns) {
      try { grid._rxRuleColumns = JSON.parse(grid.getAttribute("data-grid-rule-columns") || "[]"); } catch (_) { grid._rxRuleColumns = []; }
    }
    return grid._rxRuleColumns;
  }

  // The operator and value fields that fit the rule's column.
  function fixRule(grid, li, keepOp) {
    var columnPick = li.querySelector("[data-grid-rule-column]");
    var opPick = li.querySelector("[data-grid-rule-op]");
    var col = ruleColumns(grid).filter(function (c) { return c.key === columnPick.value; })[0];
    if (!col) return;
    var catalog = grid.querySelector("[data-grid-op-catalog]");
    var current = opPick.value;
    if (!keepOp || col.ops.indexOf(current) < 0) {
      opPick.innerHTML = "";
      col.ops.forEach(function (op) {
        var label = catalog ? catalog.querySelector('option[value="' + op + '"]').textContent : op;
        opPick.appendChild(new Option(label, op, false, op === current));
      });
    }
    var value = li.querySelector("[data-grid-rule-value]");
    var name = value.name, old = value.value, next;
    if (col.kind === "select") {
      next = document.createElement("select");
      col.options.forEach(function (o) { next.appendChild(new Option(o.label, o.value, false, o.value === old)); });
    } else {
      next = document.createElement("input");
      next.type = { number: "number", money: "number", date: "date", date_time: "date" }[col.kind] || "text";
      if (next.type === "number") next.step = "any";
      next.value = old;
    }
    next.className = value.className;
    next.name = name;
    next.setAttribute("data-grid-rule-value", "");
    next.setAttribute("aria-label", value.getAttribute("aria-label") || "");
    next.hidden = NO_VALUE.indexOf(opPick.value) >= 0;
    if (next.hidden) next.value = "";
    value.replaceWith(next);
  }

  function renumber(grid) {
    var prefix = config(grid).prefix || "";
    grid.querySelectorAll("[data-grid-rules] [data-grid-rule]").forEach(function (li, n) {
      li.querySelectorAll("[name]").forEach(function (el) {
        el.name = el.name.replace(/r\.(\d+|__n__)\./, "r." + n + ".");
        if (el.name.indexOf(prefix) !== 0) el.name = prefix + el.name;
      });
    });
  }

  document.addEventListener("click", function (event) {
    var t = event.target.closest && event.target.closest("[data-grid-add-rule], [data-grid-remove-rule], [data-grid-clear-rules]");
    if (!t) return;
    var grid = t.closest("form.rx-grid");
    if (t.hasAttribute("data-grid-add-rule")) {
      var list = grid.querySelector("[data-grid-rules]");
      var template = grid.querySelector("[data-grid-rule-template]");
      list.appendChild(template.content.cloneNode(true));
      renumber(grid);
      var li = list.lastElementChild;
      fixRule(grid, li, false);
      li.querySelector("select").focus();
    } else if (t.hasAttribute("data-grid-remove-rule")) {
      t.closest("[data-grid-rule]").remove();
      renumber(grid);
    } else {
      grid.querySelectorAll("[data-grid-rules] [data-grid-rule]").forEach(function (li) { li.remove(); });
      submit(grid);
    }
  });

  document.addEventListener("change", function (event) {
    var el = event.target;
    var li = el.closest && el.closest("[data-grid-rule]");
    if (!li) return;
    var grid = li.closest("form.rx-grid");
    if (el.hasAttribute("data-grid-rule-column")) fixRule(grid, li, false);
    else if (el.hasAttribute("data-grid-rule-op")) fixRule(grid, li, true);
  });

  // ---------- Polling ----------

  function busy(grid) {
    return inUse(grid) || grid.classList.contains("htmx-request");
  }

  // The user is doing something in the grid: editing, a menu, a selection.
  function inUse(grid) {
    return grid.querySelector(".rx-grid__editing, [popover]:popover-open, dialog[open], [data-grid-select]:checked, [aria-busy=true]") ||
      grid.contains(document.activeElement) && document.activeElement.matches("input, select, textarea");
  }

  // A poll sent just before the user started editing (or opened a menu)
  // answers after it: its table would replace what they're doing. Drop it;
  // the next poll comes once they're done.
  document.addEventListener("htmx:beforeSwap", function (event) {
    var grid = event.detail && event.detail.elt;
    if (!grid || !grid._rxPolled) return;
    grid._rxPolled = false;
    if (inUse(grid)) event.detail.shouldSwap = false;
  });

  function startPolling(grid) {
    var seconds = config(grid).poll;
    if (!seconds || grid._rxPoll) return;
    grid._rxPoll = setInterval(function () {
      if (!document.body.contains(grid)) { clearInterval(grid._rxPoll); return; }
      if (document.visibilityState !== "visible" || busy(grid)) return;
      grid._rxKeepPage = true;
      grid._rxPolled = true;
      submit(grid, true);
    }, seconds * 1000);
  }

  // ---------- Setup ----------

  function setup(grid) {
    if (grid._rxReady) return;
    grid._rxReady = true;
    applyWidths(grid);
    grid.querySelectorAll("[data-grid-rules] [data-grid-rule]").forEach(function (li) { fixRule(grid, li, true); });
    startPolling(grid);
    syncSelection(grid);
    applyVisibility(grid);
    var scroll = grid.querySelector(".rx-grid__scroll");
    if (scroll) scroll.addEventListener("scroll", function () { edges(grid); }, { passive: true });
    if (window.ResizeObserver) {
      var table = grid.querySelector(".rx-grid__table");
      var pending = false;
      new ResizeObserver(function () {
        if (pending) return;
        pending = true;
        requestAnimationFrame(function () { pending = false; layoutPins(grid); });
      }).observe(table);
    }
    if (window._rxSearchFocus === grid.id) {
      window._rxSearchFocus = null;
      var box = grid.querySelector("[data-grid-search]");
      if (box) { box.focus(); box.setSelectionRange(box.value.length, box.value.length); }
    }
    if (reopen === grid.id) {
      reopen = null;
      var menu = grid.querySelector(".rx-grid__columns");
      if (menu && menu.showPopover) menu.showPopover();
    }
  }

  function setupAll(root) {
    (root || document).querySelectorAll("form.rx-grid").forEach(setup);
    if (root && root.matches && root.matches("form.rx-grid")) setup(root);
  }

  compact.addEventListener("change", function () {
    document.querySelectorAll("form.rx-grid").forEach(applyVisibility);
  });
  document.addEventListener("htmx:load", function (event) { setupAll(event.target); });
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { setupAll(); });
  } else {
    setupAll();
  }

  window.RenoxGrid = { refresh: setupAll };
})();
