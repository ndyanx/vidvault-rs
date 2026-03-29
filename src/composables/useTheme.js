import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";

const systemDark =
  typeof window !== "undefined"
    ? window.matchMedia?.("(prefers-color-scheme: dark)").matches
    : false;

const isDark = ref(systemDark);
let persistedValueLoaded = false;

function applyTheme(dark) {
  document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
}

// Apply before the persisted value loads to avoid a flash on startup
applyTheme(isDark.value);

invoke("store_get", { key: "theme" })
  .then((saved) => {
    if (saved === "dark" || saved === "light") {
      isDark.value = saved === "dark";
    }
    persistedValueLoaded = true;
  })
  .catch(() => {
    persistedValueLoaded = true;
  });

watch(isDark, (dark) => {
  applyTheme(dark);
  if (!persistedValueLoaded) return;
  invoke("store_set", { key: "theme", value: dark ? "dark" : "light" }).catch(
    () => {},
  );
});

export function useTheme() {
  return {
    isDark,
    toggle: () => {
      isDark.value = !isDark.value;
    },
  };
}
