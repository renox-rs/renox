// renox-blocks: quantity. The − and + buttons step the number field (its
// own arrow keys already do), within its min and max, and grey out at the
// limits; each step sends `input` and `change`.

function limits(box) {
  const input = box.querySelector("input");
  if (!input) return;
  const value = parseFloat(input.value);
  const min = input.min === "" ? -Infinity : parseFloat(input.min);
  const max = input.max === "" ? Infinity : parseFloat(input.max);
  box.querySelectorAll("[data-rx-quantity-step]").forEach((button) => {
    const up = button.getAttribute("data-rx-quantity-step") === "1";
    button.disabled = !isNaN(value) && (up ? value >= max : value <= min);
  });
}

export function start(kit) {
  document.addEventListener("click", (event) => {
    const button = event.target.closest && event.target.closest("[data-rx-quantity] [data-rx-quantity-step]");
    if (!button) return;
    const box = button.closest("[data-rx-quantity]");
    const input = box.querySelector("input");
    const step = parseFloat(input.step) || 1;
    const min = input.min === "" ? -Infinity : parseFloat(input.min);
    const max = input.max === "" ? Infinity : parseFloat(input.max);
    let current = parseFloat(input.value);
    if (isNaN(current)) current = isFinite(min) ? min : 0;
    const value = Math.min(max, Math.max(min, current + step * parseFloat(button.getAttribute("data-rx-quantity-step"))));
    if (value === current && input.value !== "") return;
    input.value = String(value);
    kit.fire(input, "input");
    kit.fire(input, "change");
    kit.animate(input, { transform: ["scale(1.12)", "scale(1)"] }, 180);
  });
  document.addEventListener("input", (event) => {
    const box = event.target.closest && event.target.closest("[data-rx-quantity]");
    if (box) limits(box);
  });
}

export function setup(box, kit) {
  kit.claim(box);
  limits(box);
}
