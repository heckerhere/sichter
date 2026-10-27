<script setup lang="ts">
defineProps<{ name: string }>();

// Einfache 24px-Strichsymbole, damit keine Icon-Bibliothek nötig ist.
const paths: Record<string, string[]> = {
  library: ["M3 5h18v14H3z", "M3 15l5-5 4 4 3-3 6 6", "M15.5 9.5h.01"],
  storage: ["M4 20V10", "M10 20V4", "M16 20v-7", "M22 20H2"],
  events: ["M4 6h16v14H4z", "M4 10h16", "M8 3v4", "M16 3v4"],
  cull: ["M12 3l2.6 5.6 6 .7-4.5 4.1 1.2 6L12 16.5 6.7 19.4l1.2-6L3.4 9.3l6-.7z"],
  dupes: ["M8 8h12v12H8z", "M4 16V4h12"],
  reorg: ["M3 7h13", "M13 3l4 4-4 4", "M21 17H8", "M11 13l-4 4 4 4"],
  history: ["M3 12a9 9 0 1 0 3-6.7", "M3 4v4h4", "M12 7v5l3 2"],
  settings: ["M4 6h10", "M18 6h2", "M16 4v4", "M4 12h4", "M12 12h8", "M10 10v4", "M4 18h12", "M20 18h0", "M18 16v4"],
};
</script>

<template>
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path v-for="d in paths[name]" :key="d" :d="d" />
  </svg>
</template>
