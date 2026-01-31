// Search functionality is now handled entirely by Datastar attributes
// See website/templates/_partials/header_docs.html for implementation
//
// Signal Management:
// - $searchOpen: Tracks dialog open/close state
// - $searchQuery: Tracks search input value (bound to input field)
//
// Keyboard Shortcuts:
// - CMD+K/Ctrl+K to open: data-on:keydown__window on header
// - ESC to close: data-on:keydown__window on header
//
// Dialog Interactions:
// - Show/hide dialog: data-show="$searchOpen"
// - Close on backdrop click: data-on:click on .search-dialog
// - Prevent close on drawer click: data-on:click__stop on .search-container
// - Open on button click: data-on:click on .search-trigger
//
// Search States (controlled by $searchQuery):
// - Empty query: Show "Suggestions" section
// - 1-2 characters: Show "Type at least 3 characters to search"
// - 3+ characters: Show "Results" section (currently placeholder)
//
// Focus Management:
// - Focus input when opened: data-effect with data-ref:searchInput
// - Clear query when closed: data-effect watching $searchOpen
//
// TODO: Wire up actual search functionality
// - Could use client-side search with lunr.js or similar
// - Or make backend API calls for server-side search
// - Update the Results section to show real search results
