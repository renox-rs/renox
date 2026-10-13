// renox-blocks: datetime_range. Writes the two hidden fields that are sent
// ("YYYY-MM-DDTHH:MM") from each end's day and time, starts the end's
// calendar at the start's day, and says how long the range is (or that the
// end comes before the start).

const ISO = /^\d{4}-\d{2}-\d{2}$/;

function duration(t, minutes, kit) {
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  const rest = minutes % 60;
  const parts = [];
  if (days) parts.push(kit.fill(days === 1 ? t.day : t.days, { n: days }));
  if (hours) parts.push(kit.fill(hours === 1 ? t.hour : t.hours, { n: hours }));
  if (rest) parts.push(kit.fill(t.minutes, { n: rest }));
  return parts.join(" ");
}

function update(field, kit) {
  const t = kit.texts(field);
  const at = {};
  for (const which of ["start", "end"]) {
    const date = field.querySelector(`[data-rx-datetime-date="${which}"]`);
    const time = field.querySelector(`[data-rx-datetime-time="${which}"]`);
    const hidden = field.querySelector(`[data-rx-datetime-value="${which}"]`);
    const day = date ? date.value.trim() : "";
    const value = ISO.test(day) && time && time.value ? `${day}T${time.value}` : "";
    if (hidden && hidden.value !== value) {
      hidden.value = value;
      kit.fire(hidden, "change");
    }
    if (value) {
      at[which] = new Date(+day.slice(0, 4), +day.slice(5, 7) - 1, +day.slice(8, 10), +time.value.slice(0, 2), +time.value.slice(3, 5));
    }
  }
  // The end's calendar starts at the start's day.
  const startDay = field.querySelector('[data-rx-datetime-date="start"]');
  const endCalendar = field.querySelector('[data-rx-datetime-end="end"] calendar-date');
  if (startDay && endCalendar) {
    if (!endCalendar.hasAttribute("data-rx-datetime-min")) endCalendar.setAttribute("data-rx-datetime-min", endCalendar.getAttribute("min") || "");
    const floor = endCalendar.getAttribute("data-rx-datetime-min");
    const chosen = startDay.value.trim();
    endCalendar.setAttribute("min", ISO.test(chosen) && chosen > floor ? chosen : floor);
  }
  const summary = field.querySelector("[data-rx-datetime-summary]");
  const endTime = field.querySelector('[data-rx-datetime-time="end"]');
  if (!summary) return;
  if (at.start && at.end) {
    const minutes = Math.round((at.end - at.start) / 60000);
    if (minutes <= 0) {
      summary.textContent = t.order || "";
      summary.setAttribute("data-rx-datetime-invalid", "");
      if (endTime) endTime.setAttribute("aria-invalid", "true");
      return;
    }
    summary.textContent = duration(t, minutes, kit);
  } else {
    summary.textContent = "";
  }
  summary.removeAttribute("data-rx-datetime-invalid");
  if (endTime) endTime.removeAttribute("aria-invalid");
}

export function setup(field, kit) {
  if (!kit.claim(field)) return;
  field.addEventListener("change", () => update(field, kit));
  field.addEventListener("input", (event) => {
    if (!event.target.hasAttribute("data-rx-datetime-value")) update(field, kit);
  });
  update(field, kit);
}
