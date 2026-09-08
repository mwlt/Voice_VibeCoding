import { createRouter, createWebHashHistory } from "vue-router";
import XiaomiSettings from "../views/XiaomiSettings.vue";
import T1BleSettings from "../views/T1BleSettings.vue";
import T1UsbSettings from "../views/T1UsbSettings.vue";
import V60Settings from "../views/V60Settings.vue";
import GlobalSettings from "../views/GlobalSettings.vue";
import KeyboardTest from "../views/KeyboardTest.vue";
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
      redirect: "/t1-ble",
    },
    {
      path: "/t1-ble",
      name: "t1_ble",
      component: T1BleSettings,
    },
    {
      path: "/t1-usb",
      name: "t1_usb",
      component: T1UsbSettings,
    },
    {
      path: "/v60",
      name: "v60",
      component: V60Settings,
    },
    {
      path: "/keyboard-test",
      name: "keyboard-test",
      component: KeyboardTest,
    },
    {
      path: "/settings",
      name: "settings",
      component: GlobalSettings,
    },
  ],
});

router.beforeEach(async (to) => {
  if (to.path !== "/v60") return true;
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
