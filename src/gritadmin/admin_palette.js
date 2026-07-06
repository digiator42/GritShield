// src/routing/js/admin_palette.js
console.log("🛡️ GritAdmin Event Listener Registered Pool Ready.");

document.addEventListener('keydown', function(e) {
    const key = e.key.toLowerCase();
    
    if (e.altKey && key === 'k') {
      console.log("alt + k pressed");
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

  // Also listen for HTMX swaps to re-initialize
  document.addEventListener('htmx:afterSwap', function() {
    updateSelectionCount();
  });
});

function updateSelectionCount() {
  const checkboxes = document.querySelectorAll('[name="selected_ids"]:checked');
  const count = checkboxes.length;
  const countSpan = document.getElementById('selected-count');
  
  // Update count display
  if (countSpan) {
    countSpan.textContent = count;
  }
  
  // Build comma-separated list of IDs
  const ids = Array.from(checkboxes).map(cb => cb.value).join(',');
  const idsJson = JSON.stringify({ ids: ids });

  // ---- Update Bulk Delete Button ----
  const deleteBtn = document.getElementById('bulk-delete-btn');
  if (deleteBtn) {
    if (count > 0) {
      deleteBtn.disabled = false;
      deleteBtn.removeAttribute('disabled');
      deleteBtn.setAttribute('hx-vals', idsJson);
    } else {
      deleteBtn.disabled = true;
      deleteBtn.setAttribute('disabled', 'disabled');
    }
  }

  // ---- Update Bulk Action Button ----
  const actionBtn = document.getElementById('bulk-action-btn');
  if (actionBtn) {
    if (count > 0) {
      actionBtn.disabled = false;
      actionBtn.removeAttribute('disabled');
      // Update hx-vals for all bulk action buttons in the dropdown
      document.querySelectorAll('[hx-post*="/bulk-action/"]').forEach(function(btn) {
        btn.setAttribute('hx-vals', idsJson);
      });
    } else {
      actionBtn.disabled = true;
      actionBtn.setAttribute('disabled', 'disabled');
    }
  }
}

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

function addSchemaColumnRow() {
  const track = document.getElementById('dynamic-column-track');
  const rowId = 'col-row-' + Date.now();
  
  const rowHtml = `
      <div id="${rowId}" class="flex gap-3 items-center bg-gray-900/60 border border-gray-800 p-2 rounded-lg animate-slide-in group">
          <input type="text" placeholder="column_name" oninput="syncSchemaJson()" class="schema-col-name bg-gray-950 border border-gray-800 rounded px-3 py-1.5 flex-1 text-xs font-mono text-gray-200 focus:outline-none focus:border-blue-500 placeholder-gray-700" required />
          
          <select onchange="syncSchemaJson()" class="schema-col-type bg-gray-950 border border-gray-800 rounded px-2 py-1.5 w-36 text-xs font-mono text-gray-300 focus:outline-none focus:border-blue-500">
              <option value="string">String / Text</option>
              <option value="int">Integer</option>
              <option value="bool">Boolean</option>
              <option value="datetime">DateTime</option>
              <option value="float">Float / Real</option>
          </select>
          
          <button type="button" onclick="document.getElementById('${rowId}').remove(); syncSchemaJson();" class="w-8 h-8 text-rose-500 hover:text-rose-400 hover:bg-rose-950/20 rounded flex items-center justify-center transition font-mono text-xs">✕</button>
      </div>
  `;
  track.insertAdjacentHTML('beforeend', rowHtml);
  syncSchemaJson();
}

function syncSchemaJson() {
  const names = document.querySelectorAll('.schema-col-name');
  const types = document.querySelectorAll('.schema-col-type');
  const columns = [];
  
  for(let i = 0; i < names.length; i++) {
      // FIXED: Changed 0-8 to 0-9 so names containing '9' do not get cleared out
      const nameVal = names[i].value.trim().toLowerCase().replace(/[^a-z0-9_]/g, '');
      if(nameVal) {
          columns.push({ name: nameVal, type: types[i].value });
      }
  }
  document.getElementById('columns_data_input').value = JSON.stringify(columns);
}

// Setup initial row safely
addSchemaColumnRow();

function copyToClipboard(button, blockId) {
    const text = document.getElementById(blockId).innerText;
    navigator.clipboard.writeText(text);
    
    // Trigger your existing event listener
    document.dispatchEvent(new CustomEvent('showToast', {
        detail: { message: 'Code copied to clipboard!', type: 'success' }
    }));
}
