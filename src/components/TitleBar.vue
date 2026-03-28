<script setup>
import { ref, watch, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n } from "vue-i18n";
import { useVideoLibrary } from "../composables/useVideoLibrary.js";

const appWindow = getCurrentWindow();

const { t } = useI18n();

const props = defineProps({ isDark: Boolean, locale: String });
const emit = defineEmits(["toggle-theme", "toggle-locale"]);

const isMac = navigator.userAgent.includes("Mac");

const {
    folderName,
    currentFolder,
    folderHistory,
    isLoading,
    videos,
    openFolderDialog,
    loadFolder,
    closeFolder,
    deleteFromHistory,
} = useVideoLibrary();

const showHistory = ref(false);

// Folder thumbnails for the history dropdown
const folderThumbs = ref({});

async function loadThumbs(history) {
    const results = await Promise.all(
        history.map(async (entry) => {
            if (folderThumbs.value[entry.path] !== undefined) return;
            const url = await invoke("store_get_folder_thumb", {
                dirPath: entry.path,
            });
            return [entry.path, url ?? null];
        }),
    );
    for (const item of results) {
        if (item) folderThumbs.value[item[0]] = item[1];
    }
}

watch(folderHistory, (val) => loadThumbs(val), { immediate: true });

const toggleHistory = () => {
    if (folderHistory.value.length === 0) {
        openFolderDialog(t("titlebar.dialogTitle"));
        return;
    }
    showHistory.value = !showHistory.value;
};

const selectFolder = async (path) => {
    showHistory.value = false;
    await loadFolder(path);
};

const removeFolder = (e, path) => {
    e.stopPropagation();
    deleteFromHistory(path);
};

const handleOutsideClick = (e) => {
    if (!e.target.closest(".folder-control")) showHistory.value = false;
};

onMounted(() => document.addEventListener("mousedown", handleOutsideClick));
onUnmounted(() =>
    document.removeEventListener("mousedown", handleOutsideClick),
);

function relativeTime(ts) {
    const diff = Date.now() - ts;
    const m = Math.floor(diff / 60000);
    const h = Math.floor(diff / 3600000);
    const d = Math.floor(diff / 86400000);
    if (m < 1) return t("titlebar.timeNow");
    if (m < 60) return t("titlebar.timeMinutes", { m });
    if (h < 24) return t("titlebar.timeHours", { h });
    return t("titlebar.timeDays", { d });
}
</script>

<template>
    <header class="titlebar" :class="{ 'is-mac': isMac, 'is-win': !isMac }">
        <!-- macOS: reserve space for traffic lights on the left -->
        <div v-if="isMac" class="traffic-lights-spacer" />

        <div class="titlebar-brand">
            <span class="brand-icon">▣</span>
            <span class="brand-name">VidVault</span>
        </div>

        <!-- Center: folder pill -->
        <div class="titlebar-center">
            <div class="folder-control" v-if="currentFolder">
                <button
                    class="folder-pill"
                    @click="toggleHistory"
                    :title="currentFolder"
                    :aria-label="
                        t('titlebar.folderPillLabel', { name: folderName })
                    "
                    :aria-expanded="showHistory"
                >
                    <svg
                        width="11"
                        height="11"
                        viewBox="0 0 16 16"
                        fill="currentColor"
                    >
                        <path
                            d="M1 3.5A1.5 1.5 0 0 1 2.5 2h3.764c.414 0 .811.162 1.104.451l.897.898A1.5 1.5 0 0 0 9.37 3.8H13.5A1.5 1.5 0 0 1 15 5.3v7.2A1.5 1.5 0 0 1 13.5 14h-11A1.5 1.5 0 0 1 1 12.5z"
                        />
                    </svg>
                    <span class="folder-name">{{ folderName }}</span>
                    <span v-if="videos.length > 0" class="video-count">{{
                        videos.length
                    }}</span>
                    <svg
                        class="chevron"
                        :class="{ open: showHistory }"
                        width="10"
                        height="10"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2.5"
                    >
                        <polyline points="6 9 12 15 18 9" />
                    </svg>
                </button>

                <button
                    class="close-folder-btn"
                    @click="closeFolder"
                    :title="t('titlebar.closeFolder')"
                    :aria-label="t('titlebar.closeFolder')"
                >
                    <svg
                        width="11"
                        height="11"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2.5"
                    >
                        <line x1="18" y1="6" x2="6" y2="18" />
                        <line x1="6" y1="6" x2="18" y2="18" />
                    </svg>
                </button>

                <Transition name="dropdown">
                    <div v-if="showHistory" class="history-dropdown">
                        <div class="dropdown-header">
                            {{ t("titlebar.recents") }}
                        </div>
                        <button
                            v-for="entry in folderHistory"
                            :key="entry.path"
                            class="history-item"
                            :class="{ active: entry.path === currentFolder }"
                            @click="selectFolder(entry.path)"
                        >
                            <div class="history-item-left">
                                <div class="history-thumb">
                                    <img
                                        v-if="folderThumbs[entry.path]"
                                        :src="folderThumbs[entry.path]"
                                        class="history-thumb-img"
                                        draggable="false"
                                    />
                                    <svg
                                        v-else
                                        width="12"
                                        height="12"
                                        viewBox="0 0 16 16"
                                        fill="currentColor"
                                        class="history-icon"
                                    >
                                        <path
                                            d="M1 3.5A1.5 1.5 0 0 1 2.5 2h3.764c.414 0 .811.162 1.104.451l.897.898A1.5 1.5 0 0 0 9.37 3.8H13.5A1.5 1.5 0 0 1 15 5.3v7.2A1.5 1.5 0 0 1 13.5 14h-11A1.5 1.5 0 0 1 1 12.5z"
                                        />
                                    </svg>
                                </div>
                                <div class="history-item-info">
                                    <span class="history-name">{{
                                        entry.name
                                    }}</span>
                                    <span class="history-path">{{
                                        entry.path
                                    }}</span>
                                </div>
                            </div>
                            <div class="history-item-right">
                                <span class="history-time">{{
                                    relativeTime(entry.lastOpened)
                                }}</span>
                                <button
                                    class="history-remove"
                                    @click="removeFolder($event, entry.path)"
                                    :title="t('titlebar.remove')"
                                    :aria-label="
                                        t('titlebar.removeLabel', {
                                            name: entry.name,
                                        })
                                    "
                                >
                                    <svg
                                        width="10"
                                        height="10"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="2.5"
                                    >
                                        <line x1="18" y1="6" x2="6" y2="18" />
                                        <line x1="6" y1="6" x2="18" y2="18" />
                                    </svg>
                                </button>
                            </div>
                        </button>
                        <div class="dropdown-divider" />
                        <button
                            class="history-open-new"
                            @click="
                                () => {
                                    showHistory = false;
                                    openFolderDialog(t('titlebar.dialogTitle'));
                                }
                            "
                        >
                            <svg
                                width="12"
                                height="12"
                                viewBox="0 0 16 16"
                                fill="currentColor"
                            >
                                <path
                                    d="M1 3.5A1.5 1.5 0 0 1 2.5 2h3.764c.414 0 .811.162 1.104.451l.897.898A1.5 1.5 0 0 0 9.37 3.8H13.5A1.5 1.5 0 0 1 15 5.3v7.2A1.5 1.5 0 0 1 13.5 14h-11A1.5 1.5 0 0 1 1 12.5z"
                                />
                            </svg>
                            {{ t("titlebar.openOther") }}
                        </button>
                    </div>
                </Transition>
            </div>
        </div>

        <!-- Right controls -->
        <div class="titlebar-controls">
            <button
                class="ctrl-btn open-btn"
                @click="openFolderDialog(t('titlebar.dialogTitle'))"
                :disabled="isLoading"
                :title="t('titlebar.openFolder')"
                :aria-label="t('titlebar.openFolder')"
            >
                <svg
                    v-if="!isLoading"
                    width="14"
                    height="14"
                    viewBox="0 0 16 16"
                    fill="currentColor"
                >
                    <path
                        d="M1 3.5A1.5 1.5 0 0 1 2.5 2h3.764c.414 0 .811.162 1.104.451l.897.898A1.5 1.5 0 0 0 9.37 3.8H13.5A1.5 1.5 0 0 1 15 5.3v7.2A1.5 1.5 0 0 1 13.5 14h-11A1.5 1.5 0 0 1 1 12.5z"
                    />
                </svg>
                <svg
                    v-else
                    class="spin"
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2.5"
                >
                    <path d="M21 12a9 9 0 1 1-6.219-8.56" />
                </svg>
                <span>{{
                    isLoading ? t("titlebar.loading") : t("titlebar.openFolder")
                }}</span>
            </button>

            <!-- Window controls — solo en Windows -->
            <div v-if="!isMac" class="win-controls">
                <button
                    class="win-btn"
                    @click="appWindow.minimize()"
                    title="Minimizar"
                    aria-label="Minimizar"
                >
                    <svg
                        width="11"
                        height="11"
                        viewBox="0 0 24 24"
                        fill="currentColor"
                    >
                        <path d="M19 13H5v-2h14v2z" />
                    </svg>
                </button>

                <button
                    class="win-btn"
                    @click="appWindow.toggleMaximize()"
                    title="Maximizar"
                    aria-label="Maximizar"
                >
                    <svg
                        width="11"
                        height="11"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                    >
                        <rect x="3" y="3" width="18" height="18" rx="1" />
                    </svg>
                </button>

                <button
                    class="win-btn win-btn-close"
                    @click="appWindow.close()"
                    title="Cerrar"
                    aria-label="Cerrar"
                >
                    <svg
                        width="11"
                        height="11"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2.5"
                    >
                        <line x1="18" y1="6" x2="6" y2="18" />
                        <line x1="6" y1="6" x2="18" y2="18" />
                    </svg>
                </button>
            </div>

            <!-- Language toggle -->
            <button
                class="ctrl-btn icon-btn lang-btn"
                @click="$emit('toggle-locale')"
                :title="
                    locale === 'es' ? 'Switch to English' : 'Cambiar a Español'
                "
                :aria-label="
                    locale === 'es' ? 'Switch to English' : 'Cambiar a Español'
                "
            >
                {{ locale === "es" ? "EN" : "ES" }}
            </button>

            <!-- Theme toggle -->
            <button
                class="ctrl-btn icon-btn"
                @click="$emit('toggle-theme')"
                :title="
                    isDark ? t('titlebar.themeLight') : t('titlebar.themeDark')
                "
                :aria-label="
                    isDark ? t('titlebar.themeLight') : t('titlebar.themeDark')
                "
            >
                <svg
                    v-if="isDark"
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                >
                    <circle cx="12" cy="12" r="5" />
                    <line x1="12" y1="1" x2="12" y2="3" />
                    <line x1="12" y1="21" x2="12" y2="23" />
                    <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
                    <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
                    <line x1="1" y1="12" x2="3" y2="12" />
                    <line x1="21" y1="12" x2="23" y2="12" />
                    <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
                    <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
                </svg>
                <svg
                    v-else
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                >
                    <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
                </svg>
            </button>
        </div>
    </header>
</template>

<style scoped>
.titlebar {
    height: 48px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    background: transparent;
    border-bottom: 1px solid var(--border-subtle);
    -webkit-app-region: drag;
    flex-shrink: 0;
    position: relative;
    z-index: 200;
}
/* Windows: right padding reserves space for native WCO controls (3 × 46px) */
.titlebar.is-win {
    padding: 0 148px 0 16px;
}
/* macOS: traffic-lights-spacer handles the left offset */
.titlebar.is-mac {
    padding: 0 16px;
}
button,
a,
input {
    -webkit-app-region: no-drag;
}

.traffic-lights-spacer {
    width: 72px;
    flex-shrink: 0;
}

.titlebar-brand {
    display: flex;
    align-items: center;
    gap: 7px;
    flex-shrink: 0;
}
.brand-icon {
    font-size: 15px;
    color: var(--accent);
    line-height: 1;
}
.brand-name {
    font-family: var(--font-display);
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.04em;
    color: var(--text-primary);
}

.titlebar-center {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
}

.folder-control {
    display: flex;
    align-items: center;
    gap: 4px;
    position: relative;
    -webkit-app-region: no-drag;
}

.folder-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-subtle);
    border-radius: 20px;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 11px;
    cursor: pointer;
    max-width: 280px;
    transition:
        background 0.15s,
        border-color 0.15s;
}
.folder-pill:hover {
    background: var(--bg-app);
    border-color: var(--border-medium);
}

.folder-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary);
    font-weight: 500;
}

.video-count {
    background: var(--accent-subtle);
    color: var(--accent);
    font-size: 10px;
    font-weight: 500;
    padding: 1px 6px;
    border-radius: 10px;
    flex-shrink: 0;
}

.chevron {
    color: var(--text-tertiary);
    flex-shrink: 0;
    transition: transform 0.2s ease;
}
.chevron.open {
    transform: rotate(180deg);
}

.close-folder-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 50%;
    cursor: pointer;
    color: var(--text-tertiary);
    flex-shrink: 0;
    transition:
        background 0.15s,
        color 0.15s,
        border-color 0.15s;
}
.close-folder-btn:hover {
    background: rgba(220, 50, 40, 0.12);
    border-color: rgba(220, 50, 40, 0.3);
    color: #dc3228;
}

/* History dropdown */
.history-dropdown {
    position: absolute;
    top: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    width: 360px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-medium);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-modal);
    overflow: hidden;
    z-index: 999;
}
.dropdown-header {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.08em;
    color: var(--text-tertiary);
    padding: 10px 14px 6px;
    text-transform: uppercase;
}

.history-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 8px 14px;
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background 0.12s;
    gap: 8px;
    text-align: left;
}
.history-item:hover {
    background: var(--bg-app);
}
.history-item.active {
    background: var(--accent-subtle);
}

.history-item-left {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
}
.history-icon {
    color: var(--text-tertiary);
    flex-shrink: 0;
}
.history-item.active .history-icon {
    color: var(--accent);
}

.history-thumb {
    width: 36px;
    height: 24px;
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
    background: var(--bg-app);
    border: 1px solid var(--border-subtle);
    display: flex;
    align-items: center;
    justify-content: center;
}

.history-thumb-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
}

.history-item-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 1px;
}
.history-name {
    font-family: var(--font-display);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}
.history-path {
    font-family: var(--font-mono);
    font-size: 9.5px;
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.history-item-right {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
}
.history-time {
    font-family: var(--font-mono);
    font-size: 9.5px;
    color: var(--text-tertiary);
    white-space: nowrap;
}

.history-remove {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    color: var(--text-tertiary);
    opacity: 0;
    transition:
        opacity 0.15s,
        background 0.15s,
        color 0.15s;
}
.history-item:hover .history-remove {
    opacity: 1;
}
.history-remove:hover {
    background: rgba(220, 50, 40, 0.12);
    color: #dc3228;
}

.dropdown-divider {
    height: 1px;
    background: var(--border-subtle);
    margin: 4px 0;
}

.history-open-new {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 10px 14px;
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-display);
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
    transition: background 0.12s;
}
.history-open-new:hover {
    background: var(--accent-subtle);
}

/* Right controls */
.titlebar-controls {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    -webkit-app-region: no-drag;
}

.ctrl-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-medium);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-display);
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
    transition:
        background 0.15s,
        border-color 0.15s,
        transform 0.1s;
}
.ctrl-btn:hover:not(:disabled) {
    background: var(--bg-app);
    border-color: var(--border-strong);
}
.ctrl-btn:active:not(:disabled) {
    transform: scale(0.97);
}
.ctrl-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
}

.open-btn {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--accent-subtle);
}
.open-btn:hover:not(:disabled) {
    background: var(--accent);
    color: var(--text-on-accent);
    border-color: var(--accent);
}

.icon-btn {
    padding: 6px 8px;
    color: var(--text-secondary);
}
.icon-btn:hover {
    color: var(--text-primary);
}

.lang-btn {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    padding: 6px 10px;
}

.spin {
    animation: spin 0.9s linear infinite;
}

.win-controls {
    display: flex;
    align-items: stretch;
    height: 100%;
    /* Posición fija a la derecha, superpuesto al padding reservado */
    position: absolute;
    right: 0;
    top: 0;
}

.win-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 46px;
    height: 100%;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    -webkit-app-region: no-drag;
    transition:
        background 0.15s,
        color 0.15s;
}
.win-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
}
.win-btn-close:hover {
    background: #c42b1c;
    color: #fff;
}

@keyframes spin {
    to {
        transform: rotate(360deg);
    }
}

.dropdown-enter-active {
    transition:
        opacity 0.15s ease,
        transform 0.15s ease;
}
.dropdown-leave-active {
    transition:
        opacity 0.12s ease,
        transform 0.12s ease;
}
.dropdown-enter-from {
    opacity: 0;
    transform: translateX(-50%) translateY(-6px);
}
.dropdown-leave-to {
    opacity: 0;
    transform: translateX(-50%) translateY(-4px);
}
</style>
