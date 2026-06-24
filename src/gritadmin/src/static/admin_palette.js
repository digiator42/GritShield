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