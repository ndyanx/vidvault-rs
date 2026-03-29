// useLocale.js — Tauri version
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "vue-i18n";

const locale = ref("es");
let loaded = false;

invoke("store_get", { key: "locale" })
  .then((saved) => {
    if (saved === "en" || saved === "es") locale.value = saved;
    loaded = true;
  })
  .catch(() => {
    loaded = true;
  });

export function useLocale() {
  const { locale: i18nLocale } = useI18n();

  watch(
    locale,
    (val) => {
      i18nLocale.value = val;
      if (loaded) {
        invoke("store_set", { key: "locale", value: val }).catch(() => {});
      }
    },
    { immediate: true },
  );

  return {
    locale,
    toggle: () => {
      locale.value = locale.value === "es" ? "en" : "es";
    },
  };
}
