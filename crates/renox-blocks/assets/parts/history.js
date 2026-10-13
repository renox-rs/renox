// renox-blocks: history. The events fade and rise in one after the other
// the first time the list comes into view (opacity and transform only).
// A list already on screen when the page first painted isn't animated
// again, and nothing moves under prefers-reduced-motion.

function painted() {
  return performance.getEntriesByType && performance.getEntriesByType("paint").length > 0;
}

function onScreen(el) {
  const box = el.getBoundingClientRect();
  return box.bottom > 0 && box.top < window.innerHeight;
}

function reveal(list, kit) {
  Array.from(list.children).forEach((item, n) => {
    kit.animate(item, { opacity: [0, 1], transform: ["translateY(12px)", "translateY(0)"] }, 350, { delay: n * 50, fill: "backwards" });
  });
}

export function setup(list, kit) {
  if (!kit.claim(list) || kit.still() || !list.children.length) return;
  if (onScreen(list)) {
    if (!painted()) reveal(list, kit);
    return;
  }
  if (!("IntersectionObserver" in window)) return;
  const watch = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return;
    watch.disconnect();
    reveal(list, kit);
  });
  watch.observe(list);
}
