import { createRouter, createWebHashHistory } from "vue-router";
import LibraryView from "./views/LibraryView.vue";
import PlaceholderView from "./views/PlaceholderView.vue";

export const sections = [
  { path: "/", name: "Bibliothek", icon: "library" },
  { path: "/speicher", name: "Speicher", icon: "storage", phase: 2 },
  { path: "/events", name: "Events", icon: "events", phase: 4 },
  { path: "/sichten", name: "Sichten", icon: "cull", phase: 5 },
  { path: "/duplikate", name: "Duplikate", icon: "dupes", phase: 3 },
  { path: "/umsortieren", name: "Umsortieren", icon: "reorg", phase: 6 },
  { path: "/verlauf", name: "Verlauf", icon: "history", phase: 6 },
  { path: "/einstellungen", name: "Einstellungen", icon: "settings", phase: 7 },
] as const;

export const router = createRouter({
  history: createWebHashHistory(),
  routes: sections.map((s) =>
    s.path === "/"
      ? { path: s.path, component: LibraryView, meta: { title: s.name } }
      : { path: s.path, component: PlaceholderView, meta: { title: s.name, phase: s.phase } },
  ),
});
