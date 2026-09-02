import { createRouter, createWebHashHistory } from "vue-router";
import XiaomiSettings from "../views/XiaomiSettings.vue";
import T1Settings from "../views/T1Settings.vue";
import V60Settings from "../views/V60Settings.vue";
import GlobalSettings from "../views/GlobalSettings.vue";
import { useGlobalSettingsStore } from "../stores/globalSettings";

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      redirect: "/xiaomi",
    },
    {
      path: "/xiaomi",
      name: "xiaomi",
      component: XiaomiSettings,
    },
    {
      path: "/t1",
      name: "t1",
      component: T1Settings,
    },
    {
      path: "/v60",
      name: "v60",
      component: V60Settings,
    },
    {
      path: "/settings",
      name: "settings",
      component: GlobalSettings,
    },
  ],
});

router.beforeEach(async (to) => {
  if (to.path !== "/t1" && to.path !== "/v60") return true;
  const store = useGlobalSettingsStore();
  if (!store.loaded) {
    await store.load();
  }
  if (store.hideDevMenus) {
    return { path: "/xiaomi", replace: true };
  }
  return true;
});

export default router;
