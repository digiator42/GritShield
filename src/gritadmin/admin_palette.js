// src/routing/js/admin_palette.js
console.log("🛡️ GritAdmin Event Listener Registered Pool Ready.");

document.addEventListener('keydown', function(e) {
    const key = e.key.toLowerCase();
    
    if (e.altKey && key === 'k') {
        e.preventDefault();
        const palette = document.getElementById('command-palette');
        palette.classList.toggle('hidden');
        if (!palette.classList.contains('hidden')) {
            palette.querySelector('input').focus();
        }
    }
    
    if (e.key === 'Escape') {
        document.getElementById('command-palette').classList.add('hidden');
    }
});

  document.addEventListener('DOMContentLoaded', function() {
    // Initialise counter on page load
    updateSelectionCount();

    // Use event delegation for dynamic content (HTMX swaps)
    document.addEventListener('change', function(e) {
      if (e.target.name === 'selected_ids') {
        updateSelectionCount();
      }
    });

    function updateSelectionCount() {
      const checkboxes = document.querySelectorAll('[name="selected_ids"]:checked');
      const countSpan = document.getElementById('selected-count');
      const btn = document.getElementById('bulk-delete-btn');
      if (countSpan) {
        countSpan.textContent = checkboxes.length;
      }
      if (btn) {
        if (checkboxes.length > 0) {
          btn.disabled = false;
          btn.removeAttribute('disabled');
          // Build the comma‑separated list of IDs
          const ids = Array.from(checkboxes).map(cb => cb.value).join(',');
          btn.setAttribute('hx-vals', JSON.stringify({ ids: ids }));
        } else {
          btn.disabled = true;
          btn.setAttribute('disabled', 'disabled');
        }
      }
    }
  });

  document.addEventListener('showToast', function(e) {
    console.log("Show Toast listenenr !!!!!");
    const { message, type } = e.detail;
    const toast = document.createElement('div');
    toast.className = `px-4 py-2 rounded-lg shadow-lg text-sm font-medium ${type === 'error' ? 'bg-red-950 border border-red-800 text-red-400' : 'bg-emerald-950 border border-emerald-800 text-emerald-400'}`;
    toast.textContent = message;
    const container = document.getElementById('toast-container');
    if (!container) return;
    container.appendChild(toast);
    setTimeout(() => {
        toast.remove();
    }, 5000);
});