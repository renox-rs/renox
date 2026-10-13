// The documentation site's only script: copy buttons on code panels. The
// buttons stay hidden where the browser can't copy.
document.querySelectorAll("[data-copy]").forEach((button) => {
  if (!navigator.clipboard) return;
  button.hidden = false;
  const label = button.querySelector(".site-code__copy-label");
  button.addEventListener("click", async () => {
    const code = button.closest(".site-code").querySelector("code").innerText;
    try {
      await navigator.clipboard.writeText(code);
      label.textContent = "Copied";
    } catch {
      label.textContent = "Couldn't copy";
    }
    button.classList.add("is-done");
    setTimeout(() => {
      button.classList.remove("is-done");
      label.textContent = "Copy";
    }, 1600);
  });
});
