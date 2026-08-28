import { createRouter, createWebHashHistory } from "vue-router";

import AppShell from "../components/AppShell.vue";
import CategoriesView from "../views/CategoriesView.vue";
import CollectionsView from "../views/CollectionsView.vue";
import DiscoverView from "../views/DiscoverView.vue";
import DisplaysView from "../views/DisplaysView.vue";
import FavoritesView from "../views/FavoritesView.vue";
import LocalView from "../views/LocalView.vue";
import SearchView from "../views/SearchView.vue";
import SettingsView from "../views/SettingsView.vue";

// Hash history avoids requiring a custom deep-link fallback in the desktop asset protocol.
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      component: AppShell,
      children: [
        { path: "", redirect: "/discover" },
        { path: "discover", component: DiscoverView },
        { path: "categories", component: CategoriesView },
        { path: "collections", component: CollectionsView },
        { path: "search", component: SearchView },
        { path: "favorites", component: FavoritesView },
        { path: "local", component: LocalView },
        { path: "displays", component: DisplaysView },
        { path: "settings", component: SettingsView },
      ],
    },
  ],
});

export default router;
