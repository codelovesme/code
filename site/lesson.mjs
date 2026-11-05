// Copy is optional: source stays selectable when clipboard access is unavailable.
for (const button of document.querySelectorAll('[data-copy]')) {
  button.hidden = false;
  button.addEventListener('click', async () => {
    const code = document.getElementById(button.dataset.copy);
    const status = button.closest('figure')?.querySelector('[role="status"]') ?? document.getElementById('lesson-copy-status');
    try {
      await navigator.clipboard.writeText(code.textContent);
      status.textContent = button.dataset.copyType === 'shell'
        ? 'Commands copied. Paste them into your terminal.'
        : 'Code copied. Paste it into the playground.';
    } catch {
      const range = document.createRange();
      range.selectNodeContents(code);
      const selection = window.getSelection();
      selection.removeAllRanges(); selection.addRange(range);
      code.focus();
      status.textContent = 'Code selected. Use your device’s copy command.';
    }
  });
}
