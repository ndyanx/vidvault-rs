/**
 * useVolume — shared global volume state across the modal and previews.
 *
 * - volume and muted are persisted in localStorage to survive restarts.
 * - any component that imports this composable reads/writes the same state.
 */

import { ref, watch } from "vue";

const STORAGE_KEY = "vidvault:volume";

function loadSaved() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const { volume, muted } = JSON.parse(raw);
      return {
        volume:
          typeof volume === "number" ? Math.min(1, Math.max(0, volume)) : 1,
        muted: typeof muted === "boolean" ? muted : false,
      };
    }
  } catch {
    // ignore
  }
  return { volume: 1, muted: false };
}

const saved = loadSaved();
const globalVolume = ref(saved.volume);
const globalMuted = ref(saved.muted);

// Persist any change automatically
watch([globalVolume, globalMuted], ([volume, muted]) => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ volume, muted }));
  } catch {
    // ignore
  }
});

/**
 * Applies the global volume state to a given <video> element.
 * @param {HTMLVideoElement} el
 */
export function applyVolumeToEl(el) {
  if (!el) return;
  el.volume = globalVolume.value;
  el.muted = globalMuted.value;
}

/**
 * Syncs the global state from a <video> element
 * (call on the video's "volumechange" events).
 * @param {HTMLVideoElement} el
 */
export function syncVolumeFromEl(el) {
  if (!el) return;
  globalVolume.value = el.volume;
  globalMuted.value = el.muted;
}

export function useVolume() {
  return {
    globalVolume,
    globalMuted,
    applyVolumeToEl,
    syncVolumeFromEl,
  };
}
