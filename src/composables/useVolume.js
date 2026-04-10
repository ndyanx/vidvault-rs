/**
 * useVolume — estado global de volumen compartido entre modal y previews.
 *
 * - volumen y muted se persisten en localStorage para sobrevivir reinicios.
 * - cualquier componente que importe este composable lee/escribe el mismo estado.
 */

import { ref, watch } from "vue";

const STORAGE_KEY = "vidvault:volume";

function loadSaved() {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (raw) {
            const { volume, muted } = JSON.parse(raw);
            return {
                volume: typeof volume === "number" ? Math.min(1, Math.max(0, volume)) : 1,
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

// Persiste cualquier cambio automáticamente
watch([globalVolume, globalMuted], ([volume, muted]) => {
    try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify({ volume, muted }));
    } catch {
        // ignore
    }
});

/**
 * Aplica el estado global a un elemento <video> dado.
 * @param {HTMLVideoElement} el
 */
export function applyVolumeToEl(el) {
    if (!el) return;
    el.volume = globalVolume.value;
    el.muted = globalMuted.value;
}

/**
 * Sincroniza el estado global desde un elemento <video>
 * (llamar en los eventos "volumechange" del video).
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
