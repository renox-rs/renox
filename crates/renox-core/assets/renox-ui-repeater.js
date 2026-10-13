// Renox UI: the repeater (`repeater`, and `key_value`, which is one).
// renox-ui.js loads this module when a page, or what htmx swaps in, has
// one; it runs once per page and sets up each root it's handed.

var kit = window.Renox._kit;
var errorKey = kit.errorKey;
var firstField = kit.firstField;
var setup = kit.setup;
var emit = kit.emit;

// ---------- Repeater ----------

function idOf(name) {
  return "rx-" + name.replace(/\./g, "-").replace(/\[/g, "-").replace(/\]/g, "");
}

function rowsOf(rep) {
  var holder = rep.querySelector("[data-rx-rows]");
  return Array.prototype.filter.call(holder.children, function (el) { return el.hasAttribute("data-rx-row"); });
}

// Names, ids and references follow the row's place: `lines[2][name]`,
// `lines.2.name`, `rx-lines-2-name`.
function renumber(rep) {
  var name = rep.getAttribute("data-rx-repeater");
  var dotted = errorKey(name), idp = idOf(name);
  rowsOf(rep).forEach(function (row, n) {
    var o = row.getAttribute("data-rx-index");
    if (o !== String(n)) {
      var swaps = [[name + "[" + o + "]", name + "[" + n + "]"], [dotted + "." + o + ".", dotted + "." + n + "."], [idp + "-" + o + "-", idp + "-" + n + "-"]];
      [row].concat(Array.prototype.slice.call(row.querySelectorAll("*"))).forEach(function (el) {
        Array.prototype.forEach.call(el.attributes, function (attr) {
          var value = attr.value, next = value;
          swaps.forEach(function (s) { next = next.split(s[0]).join(s[1]); });
          if (next !== value) el.setAttribute(attr.name, next);
        });
      });
      row.setAttribute("data-rx-index", String(n));
    }
    var number = row.querySelector("[data-rx-row-number]");
    if (number) number.textContent = String(n + 1);
  });
  limits(rep);
}

function limits(rep) {
  var count = rowsOf(rep).length;
  var min = parseInt(rep.getAttribute("data-rx-min") || "0", 10);
  var max = parseInt(rep.getAttribute("data-rx-max") || "0", 10);
  rep.querySelectorAll("[data-rx-row-remove]").forEach(function (b) { if (b.closest("[data-rx-repeater]") === rep) b.disabled = count <= min; });
  var add = rep.querySelector("[data-rx-row-add]");
  if (add) add.disabled = max > 0 && count >= max;
}

function addRow(rep) {
  var template = Array.prototype.find.call(rep.querySelectorAll("template[data-rx-row-template]"), function (t) { return t.closest("[data-rx-repeater]") === rep; });
  if (!template) return;
  var index = rowsOf(rep).length;
  var holder = document.createElement("template");
  holder.innerHTML = template.innerHTML.split("__INDEX__").join(String(index));
  var row = holder.content.querySelector("[data-rx-row]");
  // Scripts cloned from a template don't run: put them back as new ones,
  // once per source.
  holder.content.querySelectorAll("script").forEach(function (old) {
    if (old.src && document.querySelector('script[src="' + old.getAttribute("src") + '"]')) { old.remove(); return; }
    var fresh = document.createElement("script");
    Array.prototype.forEach.call(old.attributes, function (a) { fresh.setAttribute(a.name, a.value); });
    fresh.textContent = old.textContent;
    old.replaceWith(fresh);
  });
  rep.querySelector("[data-rx-rows]").appendChild(holder.content);
  renumber(rep);
  setup(row);
  if (window.htmx && window.htmx.process) window.htmx.process(row);
  var first = firstField(row);
  if (first) first.focus();
  emit(rep, "added", { index: index });
}

document.addEventListener("click", function (event) {
  var target = event.target.closest ? event.target : event.target.parentElement;
  if (!target) return;
  var button = target.closest("[data-rx-row-add], [data-rx-row-remove], [data-rx-row-up], [data-rx-row-down]");
  if (!button || button.disabled) return;
  var rep = button.closest("[data-rx-repeater]");
  if (button.hasAttribute("data-rx-row-add")) { addRow(rep); return; }
  var row = button.closest("[data-rx-row]");
  if (button.hasAttribute("data-rx-row-remove")) {
    var next = row.nextElementSibling || row.previousElementSibling;
    var gone = parseInt(row.getAttribute("data-rx-index"), 10);
    row.remove();
    renumber(rep);
    emit(rep, "removed", { index: gone });
    var focus = next && firstField(next) || rep.querySelector("[data-rx-row-add]");
    if (focus) focus.focus();
  } else if (button.hasAttribute("data-rx-row-up") && row.previousElementSibling) {
    row.parentNode.insertBefore(row, row.previousElementSibling);
    renumber(rep);
    button.focus();
  } else if (button.hasAttribute("data-rx-row-down") && row.nextElementSibling) {
    row.parentNode.insertBefore(row.nextElementSibling, row);
    renumber(rep);
    button.focus();
  }
  var form = rep.closest("form");
  if (form) form.dispatchEvent(new Event("change", { bubbles: true }));
});

kit.ready("repeater", function (root) { kit.each(root, "[data-rx-repeater]", limits); });
