// renox-blocks: receipt. The "Print" button opens the browser's print
// dialog (no inline handler, so CSP=strict works). The paper size and what
// is left out of the printout are CSS (@media print in blocks.css).

export function setup(receipt, kit) {
  if (!kit.claim(receipt)) return;
  receipt.querySelectorAll("[data-rx-receipt-print]").forEach((button) => {
    button.addEventListener("click", () => window.print());
  });
}
