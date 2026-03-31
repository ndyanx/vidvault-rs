import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { formatSize } from "../utils/format.js";

const MAX_HISTORY = 8;

const videos = ref([]);
const currentFolder = ref(null);
const isLoading = ref(false);
const isInitializing = ref(true);
const error = ref(null);
const folderHistory = ref([]);

function folderNameFrom(folderPath) {
  return (
    folderPath.replace(/\\/g, "/").split("/").filter(Boolean).pop() ||
    folderPath
  );
}

function pushToHistory(history, folderPath) {
  const filtered = history.filter((h) => h.path !== folderPath);
  filtered.unshift({
    path: folderPath,
    name: folderNameFrom(folderPath),
    lastOpened: Date.now(),
  });
  return filtered.slice(0, MAX_HISTORY);
}

function removeFromHistory(history, folderPath) {
  return history.filter((h) => h.path !== folderPath);
}

// ── Event listeners ───────────────────────────────────────────────────────────

let unlistenThumbnail = null;
let unlistenDims = null;
let unlistenNoStream = null;
let unlistenFolderChanged = null;
let initPromise = null;

// Serializes concurrent applyDiff calls so that existingIds is always
// computed against an already-updated videos array. Without this, two
// folder:changed events arriving before the first invoke resolves both see
// the same videos list and can insert duplicate cards.
let applyDiffPromise = Promise.resolve();

async function ensureListeners() {
  if (unlistenThumbnail) return;

  unlistenThumbnail = await listen("thumbnail:ready", ({ payload }) => {
    const video = videos.value.find((v) => v.id === payload.id);
    if (video) video.thumbnailUrl = payload.thumbnailUrl;
  });

  unlistenDims = await listen("dims:ready", ({ payload }) => {
    const video = videos.value.find((v) => v.id === payload.id);
    if (video) {
      video.width = payload.width;
      video.height = payload.height;
      video.duration = payload.duration;
    }
  });

  unlistenNoStream = await listen("video:no-stream", ({ payload }) => {
    videos.value = videos.value.filter((v) => v.id !== payload.id);
  });
}

async function ensureFolderWatcher(loadFolder, applyDiff) {
  if (unlistenFolderChanged) return;
  unlistenFolderChanged = await listen("folder:changed", ({ payload }) => {
    if (!currentFolder.value) return;
    if (payload.removed.length) {
      loadFolder(currentFolder.value);
      return;
    }
    if (payload.added.length) applyDiff(payload.added);
  });
}

// ── Store helpers ─────────────────────────────────────────────────────────────

const store = {
  get: (key) => invoke("store_get", { key }),
  set: (key, value) => invoke("store_set", { key, value }),
  getAll: () => invoke("store_get_all"),
  getFolderThumb: (dirPath) => invoke("store_get_folder_thumb", { dirPath }),
};

// ── Composable ────────────────────────────────────────────────────────────────

export function useVideoLibrary() {
  const isEmpty = computed(() => videos.value.length === 0);

  const folderName = computed(() =>
    currentFolder.value ? folderNameFrom(currentFolder.value) : null,
  );

  function applyDiff(addedPaths) {
    applyDiffPromise = applyDiffPromise.then(() => _applyDiffOnce(addedPaths));
  }

  async function _applyDiffOnce(addedPaths) {
    if (!addedPaths.length || !currentFolder.value) return;

    let entries;
    try {
      entries = await invoke("fs_read_video_entries", {
        filePaths: addedPaths,
      });
    } catch (e) {
      console.error("[applyDiff] fs_read_video_entries error:", e);
      return;
    }

    if (!Array.isArray(entries) || !entries.length) return;

    const existingIds = new Set(videos.value.map((v) => v.id));
    const newVideos = entries
      .filter((v) => !existingIds.has(v.id))
      .map((v) => ({ ...v, sizeFormatted: formatSize(v.size) }));

    if (newVideos.length) {
      videos.value = [...newVideos, ...videos.value];
      const newPaths = newVideos.map((v) => v.filePath);
      invoke("pipeline_process", { filePaths: newPaths }).catch(console.error);
    }
  }

  async function loadFolder(folderPath) {
    if (!folderPath) return;

    await invoke("pipeline_cancel");

    isLoading.value = true;
    error.value = null;
    videos.value = [];
    currentFolder.value = folderPath;

    // Reset the applyDiff queue so stale callbacks from the previous folder
    // don't run against the new one.
    applyDiffPromise = Promise.resolve();

    await ensureListeners();
    await ensureFolderWatcher(loadFolder, applyDiff);

    try {
      const result = await invoke("fs_read_videos", { dirPath: folderPath });

      if (!Array.isArray(result)) {
        const errType = result?.error || "read_error";
        if (errType === "not_found") {
          error.value = { type: "not_found", folder: folderPath };
          currentFolder.value = null;
          const next = removeFromHistory(folderHistory.value, folderPath);
          folderHistory.value = next;
          store.set("lastFolder", null).catch(console.error);
          store.set("folderHistory", next).catch(console.error);
        } else {
          error.value = { type: "read_error", folder: folderPath };
          currentFolder.value = null;
        }
        return;
      }

      error.value = null;
      videos.value = result.map((v) => ({
        ...v,
        sizeFormatted: formatSize(v.size),
      }));

      // Seed the pipeline with the first 20 videos (likely visible on load)
      const seedPaths = result.slice(0, 20).map((v) => v.filePath);
      if (seedPaths.length) {
        invoke("pipeline_process", { filePaths: seedPaths }).catch(
          console.error,
        );
      }

      const next = pushToHistory(folderHistory.value, folderPath);
      folderHistory.value = next;
      store.set("lastFolder", String(folderPath)).catch(console.error);
      store.set("folderHistory", next).catch(console.error);
    } catch (err) {
      error.value = { type: "read_error", folder: folderPath };
      currentFolder.value = null;
      console.error("[useVideoLibrary] loadFolder error:", err);
    } finally {
      isLoading.value = false;
    }
  }

  async function openFolderDialog(dialogTitle) {
    const selected = await open({
      directory: true,
      multiple: false,
      title: dialogTitle || "Select video folder",
    });
    if (selected)
      await loadFolder(typeof selected === "string" ? selected : selected[0]);
  }

  async function closeFolder() {
    await invoke("pipeline_cancel");
    videos.value = [];
    currentFolder.value = null;
    error.value = null;
    store.set("lastFolder", null).catch(console.error);
  }

  async function deleteFromHistory(folderPath) {
    const next = removeFromHistory(folderHistory.value, folderPath);
    folderHistory.value = next;
    store.set("folderHistory", next).catch(console.error);
    if (currentFolder.value === folderPath) {
      await invoke("pipeline_cancel");
      videos.value = [];
      currentFolder.value = null;
      store.set("lastFolder", null).catch(console.error);
    }
  }

  function processVisible(filePaths) {
    if (!filePaths || !filePaths.length) return;
    invoke("pipeline_process", { filePaths }).catch(console.error);
  }

  function showInFolder(filePath) {
    invoke("shell_show_in_folder", { filePath }).catch(console.error);
  }

  async function copyPath(filePath) {
    await invoke("shell_copy_path", { filePath });
  }

  function dismissError() {
    error.value = null;
  }

  async function init() {
    if (initPromise) return initPromise;
    initPromise = (async () => {
      try {
        const state = await store.getAll();
        folderHistory.value = state.folderHistory || [];
        if (state.lastFolder) {
          await loadFolder(state.lastFolder);
        }
      } catch (e) {
        console.error("[useVideoLibrary] init error:", e);
      } finally {
        isInitializing.value = false;
      }
    })();
    return initPromise;
  }

  return {
    videos,
    currentFolder,
    folderName,
    folderHistory,
    isLoading,
    isInitializing,
    error,
    isEmpty,
    openFolderDialog,
    loadFolder,
    closeFolder,
    deleteFromHistory,
    dismissError,
    processVisible,
    showInFolder,
    copyPath,
    init,
    store,
  };
}
