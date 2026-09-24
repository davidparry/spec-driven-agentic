document.querySelectorAll("[data-copy]").forEach((button) => {
  button.addEventListener("click", async () => {
    const target = document.querySelector(button.getAttribute("data-copy"));
    if (!target) return;
    const text = target.textContent.trim();
    try {
      await navigator.clipboard.writeText(text);
      const previous = button.textContent;
      button.textContent = "Copied";
      setTimeout(() => {
        button.textContent = previous;
      }, 1400);
    } catch {
      button.textContent = "Copy failed";
    }
  });
});
