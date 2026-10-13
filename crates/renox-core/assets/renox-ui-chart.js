// Renox UI: charts (`chart(…)`) and the period filter (`period_filter`).
// renox-ui.js loads this module when a page, or what htmx swaps in, has
// one; it runs once per page and sets up each root it's handed.

var kit = window.Renox._kit;

// ---------- Charts (chart(…)) ----------

// A crosshair and one tooltip for every series at the nearest label (line,
// area), or per band (bar) and slice (pie); arrow keys move it too.
function setupChart(figure) {
  if (figure._rxChart) return;
  figure._rxChart = true;
  var data;
  try { data = JSON.parse(figure.getAttribute("data-rx-chart")); } catch (e) { return; }
  var plot = figure.querySelector(".rx-chart__plot");
  var tip = figure.querySelector(".rx-chart__tip");
  var cross = figure.querySelector(".rx-chart__cross");
  if (data.points) { setupPoints(figure, data.points, plot, tip); return; }
  if (!plot || !tip || !data.labels || !data.labels.length) return;
  var n = data.labels.length;
  var pie = data.kind === "pie" || data.kind === "doughnut";
  var bar = data.kind === "bar";
  var current = -1;

  function position(i) {
    if (bar) return (i + 0.5) / n * 100;
    return n > 1 ? i / (n - 1) * 100 : 50;
  }

  function row(key, value, name) {
    var line = document.createElement("p");
    line.className = "rx-chart__tip-row";
    if (key) {
      var mark = document.createElement("span");
      mark.className = "rx-chart__key rx-chart__key--line " + key;
      line.appendChild(mark);
    }
    var strong = document.createElement("span");
    strong.className = "rx-chart__tip-value";
    strong.textContent = value;
    line.appendChild(strong);
    if (name) {
      var label = document.createElement("span");
      label.className = "rx-chart__tip-name";
      label.textContent = name;
      line.appendChild(label);
    }
    return line;
  }

  function show(i, pointX) {
    current = i;
    tip.textContent = "";
    var title = document.createElement("p");
    title.className = "rx-chart__tip-label";
    title.textContent = data.labels[i];
    tip.appendChild(title);
    data.series.forEach(function (series) {
      var value = series.values[i];
      if (value === null || value === undefined) return;
      tip.appendChild(row(pie ? "" : series.slot, value, pie || data.series.length < 2 ? "" : series.name));
    });
    tip.hidden = false;
    var width = plot.clientWidth;
    if (pie) {
      figure.querySelectorAll(".rx-chart__slice").forEach(function (s) {
        if (s.getAttribute("data-index") === String(i)) s.setAttribute("data-on", ""); else s.removeAttribute("data-on");
      });
      var x = pointX === undefined ? width / 2 : pointX;
      tip.style.left = Math.min(Math.max(x - tip.offsetWidth / 2, 0), Math.max(width - tip.offsetWidth, 0)) + "px";
      return;
    }
    var left = position(i) / 100 * width;
    if (cross && !bar) { cross.hidden = false; cross.style.left = left + "px"; }
    if (bar) {
      figure.querySelectorAll(".rx-chart__band").forEach(function (b) {
        if (b.getAttribute("data-index") === String(i)) b.setAttribute("data-on", ""); else b.removeAttribute("data-on");
      });
    }
    var tipLeft = left + 12;
    if (tipLeft + tip.offsetWidth > width) tipLeft = left - 12 - tip.offsetWidth;
    tip.style.left = Math.max(tipLeft, 0) + "px";
  }

  function hide() {
    current = -1;
    tip.hidden = true;
    if (cross) cross.hidden = true;
    figure.querySelectorAll("[data-on]").forEach(function (el) { el.removeAttribute("data-on"); });
  }

  function indexAt(clientX) {
    var rect = plot.getBoundingClientRect();
    var x = Math.min(Math.max((clientX - rect.left) / rect.width, 0), 1);
    return bar ? Math.min(Math.floor(x * n), n - 1) : Math.round(x * (n - 1));
  }

  plot.addEventListener("pointermove", function (event) {
    if (pie) {
      var slice = event.target.closest && event.target.closest(".rx-chart__slice");
      if (!slice) { hide(); return; }
      var rect = plot.getBoundingClientRect();
      show(parseInt(slice.getAttribute("data-index"), 10), event.clientX - rect.left);
      return;
    }
    show(indexAt(event.clientX));
  });
  plot.addEventListener("pointerleave", hide);
  plot.addEventListener("blur", hide);
  plot.addEventListener("focus", function () { show(current < 0 ? n - 1 : current); });
  plot.addEventListener("keydown", function (event) {
    var next = current < 0 ? n - 1 : current;
    if (event.key === "ArrowLeft") next = Math.max(next - 1, 0);
    else if (event.key === "ArrowRight") next = Math.min(next + 1, n - 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = n - 1;
    else if (event.key === "Escape") { hide(); return; }
    else return;
    event.preventDefault();
    show(next);
  });
}

// Scatter and bubble charts: the tooltip of the point nearest the pointer
// (within reach of its edge), or of the one the arrow keys reach, left to
// right.
function setupPoints(figure, points, plot, tip) {
  var n = points.length;
  if (!plot || !tip || !n) return;
  var marks = [];
  figure.querySelectorAll(".rx-chart__point").forEach(function (el) {
    marks[parseInt(el.getAttribute("data-index"), 10)] = el;
  });
  var current = -1;

  function text(tag, className, value) {
    var el = document.createElement(tag);
    el.className = className;
    el.textContent = value;
    return el;
  }

  function show(i) {
    current = i;
    var point = points[i];
    tip.textContent = "";
    if (point.title) tip.appendChild(text("p", "rx-chart__tip-label", point.title));
    if (point.name) {
      var series = text("p", "rx-chart__tip-row", "");
      var mark = text("span", "rx-chart__key rx-chart__key--dot " + point.slot, "");
      series.appendChild(mark);
      series.appendChild(text("span", "rx-chart__tip-name", point.name));
      tip.appendChild(series);
    }
    point.rows.forEach(function (pair) {
      var line = text("p", "rx-chart__tip-row", "");
      line.appendChild(text("span", "rx-chart__tip-name", pair[0]));
      line.appendChild(text("span", "rx-chart__tip-value", pair[1]));
      tip.appendChild(line);
    });
    tip.hidden = false;
    marks.forEach(function (el, k) {
      if (!el) return;
      if (k === i) el.setAttribute("data-on", ""); else el.removeAttribute("data-on");
    });
    var width = plot.clientWidth, height = plot.clientHeight;
    var x = point.left / 100 * width, y = (1 - point.bottom / 100) * height;
    var reach = (marks[i] ? marks[i].offsetWidth / 2 : 4) + 8;
    var left = x + reach;
    if (left + tip.offsetWidth > width) left = x - reach - tip.offsetWidth;
    tip.style.left = Math.max(left, 0) + "px";
    var top = y - tip.offsetHeight / 2;
    tip.style.top = Math.min(Math.max(top, 0), Math.max(height - tip.offsetHeight, 0)) + "px";
  }

  function hide() {
    current = -1;
    tip.hidden = true;
    marks.forEach(function (el) { if (el) el.removeAttribute("data-on"); });
  }

  plot.addEventListener("pointermove", function (event) {
    var rect = plot.getBoundingClientRect();
    var px = event.clientX - rect.left, py = event.clientY - rect.top;
    var best = -1, bestDistance = Infinity;
    points.forEach(function (point, i) {
      var dx = point.left / 100 * rect.width - px;
      var dy = (1 - point.bottom / 100) * rect.height - py;
      var distance = Math.sqrt(dx * dx + dy * dy);
      var reach = (marks[i] ? marks[i].offsetWidth / 2 : 4) + 16;
      if (distance <= reach && distance < bestDistance) { best = i; bestDistance = distance; }
    });
    if (best < 0) hide(); else if (best !== current) show(best);
  });
  plot.addEventListener("pointerleave", hide);
  plot.addEventListener("blur", hide);
  plot.addEventListener("focus", function () { show(current < 0 ? 0 : current); });
  plot.addEventListener("keydown", function (event) {
    var next = current < 0 ? 0 : current;
    if (event.key === "ArrowLeft" || event.key === "ArrowDown") next = Math.max(next - 1, 0);
    else if (event.key === "ArrowRight" || event.key === "ArrowUp") next = Math.min(next + 1, n - 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = n - 1;
    else if (event.key === "Escape") { hide(); return; }
    else return;
    event.preventDefault();
    show(next);
  });
}

// ---------- The period filter's custom range (period_filter) ----------

// Its panel stays on screen and, opened from its button, takes focus;
// Escape (outside the calendar) closes it back to the button, and so does
// a click elsewhere.
document.addEventListener("click", function (event) {
  var summary = event.target.closest && event.target.closest("details[data-rx-period] > summary");
  if (summary) summary.parentElement._rxOpenedHere = true;
  document.querySelectorAll("details[data-rx-period][open]").forEach(function (details) {
    if (!details.contains(event.target)) details.open = false;
  });
});
document.addEventListener("toggle", function (event) {
  var details = event.target;
  if (!details.matches || !details.matches("details[data-rx-period]") || !details.open) return;
  var form = details.querySelector(".rx-period__form");
  if (!form) return;
  form.style.left = "";
  var rect = form.getBoundingClientRect();
  var over = rect.right - (document.documentElement.clientWidth - 16);
  if (over > 0) form.style.left = -Math.max(Math.min(over, rect.left - 16), 0) + "px";
  if (details._rxOpenedHere) {
    details._rxOpenedHere = false;
    var first = form.querySelector("input:not([type=hidden])");
    if (first) first.focus();
  }
}, true);
document.addEventListener("keydown", function (event) {
  if (event.key !== "Escape") return;
  var details = event.target.closest && event.target.closest("details[data-rx-period][open]");
  if (!details || details.querySelector(":popover-open")) return;
  details.open = false;
  var summary = details.querySelector("summary");
  if (summary) summary.focus();
});

kit.ready("chart", function (root) { kit.each(root, "[data-rx-chart]", setupChart); });
