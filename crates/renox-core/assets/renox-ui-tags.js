// Renox UI: the tags field (`tags_input`). renox-ui.js loads this module
// when a page, or what htmx swaps in, has one; its listeners are on the
// document, so it runs once per page whatever comes later.

var kit = window.Renox._kit;
var CLOSE = kit.CLOSE;

// ---------- Tags ----------

function tagValues(box) {
  return Array.prototype.map.call(box.querySelectorAll('.rx-tag input[type="hidden"]'), function (i) { return i.value; });
}

function addTags(box, text) {
  var entry = box.querySelector("[data-rx-tags-entry]");
  var list = box.querySelector(".rx-tags__list");
  var name = box.getAttribute("data-rx-tags");
  var have = tagValues(box).map(function (v) { return v.toLowerCase(); });
  var added = false;
  text.split(",").map(function (t) { return t.trim(); }).filter(Boolean).forEach(function (tag) {
    if (have.indexOf(tag.toLowerCase()) >= 0) return;
    have.push(tag.toLowerCase());
    var li = document.createElement("li");
    li.className = "rx-tag";
    var label = document.createElement("span");
    label.className = "rx-tag__text";
    label.textContent = tag;
    var remove = document.createElement("button");
    remove.type = "button";
    remove.className = "rx-tag__remove";
    remove.setAttribute("data-rx-tag-remove", "");
    remove.setAttribute("aria-label", (box.getAttribute("data-remove") || "Remove") + " " + tag);
    remove.innerHTML = CLOSE;
    var hidden = document.createElement("input");
    hidden.type = "hidden";
    hidden.name = name;
    hidden.value = tag;
    li.append(label, remove, hidden);
    list.appendChild(li);
    added = true;
  });
  if (entry) entry.value = "";
  if (added && entry) entry.dispatchEvent(new Event("change", { bubbles: true }));
}

// Early (capture): Enter here makes a tag, whatever else on the page
// listens for Enter (a wizard's goes on to the next step), and those
// handlers may have been added first.
document.addEventListener("keydown", function (event) {
  var entry = event.target;
  if (!entry.hasAttribute || !entry.hasAttribute("data-rx-tags-entry")) return;
  var box = entry.closest("[data-rx-tags]");
  if (event.key === "Enter" || event.key === ",") {
    if (event.isComposing) return;
    event.preventDefault();
    addTags(box, entry.value);
  } else if (event.key === "Backspace" && !entry.value) {
    var last = box.querySelector(".rx-tag:last-of-type");
    if (last) { last.remove(); entry.dispatchEvent(new Event("change", { bubbles: true })); }
  }
}, true);
document.addEventListener("input", function (event) {
  var entry = event.target;
  if (!entry.hasAttribute || !entry.hasAttribute("data-rx-tags-entry") || entry.value.indexOf(",") < 0) return;
  // A pasted "a, b, c": all but what follows the last comma become tags.
  var cut = entry.value.lastIndexOf(",");
  var rest = entry.value.slice(cut + 1);
  addTags(entry.closest("[data-rx-tags]"), entry.value.slice(0, cut));
  entry.value = rest;
});
document.addEventListener("focusout", function (event) {
  var entry = event.target;
  if (entry.hasAttribute && entry.hasAttribute("data-rx-tags-entry") && entry.value.trim()) {
    addTags(entry.closest("[data-rx-tags]"), entry.value);
  }
});
// Text still in the box when the form is sent is a tag too.
document.addEventListener("submit", function (event) {
  event.target.querySelectorAll && event.target.querySelectorAll("[data-rx-tags-entry]").forEach(function (entry) {
    if (entry.value.trim()) addTags(entry.closest("[data-rx-tags]"), entry.value);
  });
}, true);
document.addEventListener("click", function (event) {
  var target = event.target.closest ? event.target : event.target.parentElement;
  if (!target) return;
  var remove = target.closest("[data-rx-tag-remove]");
  if (remove) {
    var box = remove.closest("[data-rx-tags]");
    remove.closest(".rx-tag").remove();
    var entry = box.querySelector("[data-rx-tags-entry]");
    if (entry) { entry.focus(); entry.dispatchEvent(new Event("change", { bubbles: true })); }
    return;
  }
  var tags = target.closest(".rx-tags");
  if (tags && target === tags) {
    var input = tags.querySelector("input:not([type=hidden])");
    if (input) input.focus();
  }
});

kit.ready("tags", function () {});
