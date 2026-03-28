<script setup>
import { onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useTheme } from './composables/useTheme.js'
import { useLocale } from './composables/useLocale.js'
import { useVideoLibrary } from './composables/useVideoLibrary.js'
import { initFavorites } from './composables/useFavorites.js'
import TitleBar from './components/TitleBar.vue'
import GalleryPanel from './components/GalleryPanel.vue'
import EmptyState from './components/EmptyState.vue'

const { t } = useI18n()
const { isDark, toggle } = useTheme()
const { locale, toggle: toggleLocale } = useLocale()
const { init, isEmpty, isLoading, isInitializing, error, dismissError, openFolderDialog } =
  useVideoLibrary()

onMounted(async () => {
  await initFavorites()
  await init()
})
</script>

<template>
  <div class="app-root">
    <TitleBar
      :isDark="isDark"
      :locale="locale"
      @toggle-theme="toggle"
      @toggle-locale="toggleLocale"
    />

    <!-- Folder not found banner -->
    <Transition name="banner">
      <div v-if="error" class="error-banner">
        <svg
          width="15"
          height="15"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <span v-if="error.type === 'not_found'">
          {{ t('error.notFound', { folder: error.folder }) }}
        </span>
        <span v-else>{{ t('error.readError') }}</span>
        <div class="banner-actions">
          <button class="banner-btn accent" @click="openFolderDialog">
            {{ t('error.openOther') }}
          </button>
          <button class="banner-btn" @click="dismissError">{{ t('error.close') }}</button>
        </div>
      </div>
    </Transition>

    <main class="app-body">
      <EmptyState v-if="isEmpty && !isLoading && !isInitializing" />
      <GalleryPanel v-else />
    </main>
  </div>
</template>

<style scoped>
.app-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: transparent; /* don't block native acrylic/vibrancy effect */
  overflow: hidden;
}

.app-body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  position: relative;
}

.error-banner {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 16px;
  background: rgba(220, 50, 40, 0.08);
  border-bottom: 1px solid rgba(220, 50, 40, 0.2);
  font-family: var(--font-display);
  font-size: 12.5px;
  color: var(--text-primary);
  flex-shrink: 0;
}

.error-banner svg {
  color: #dc3228;
  flex-shrink: 0;
}
.error-banner span {
  flex: 1;
  line-height: 1.4;
}
.error-banner strong {
  color: #dc3228;
  font-weight: 600;
}

.banner-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.banner-btn {
  padding: 4px 12px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-medium);
  background: var(--bg-elevated);
  font-family: var(--font-display);
  font-size: 11.5px;
  font-weight: 500;
  color: var(--text-primary);
  cursor: pointer;
  transition: background 0.15s;
}
.banner-btn:hover {
  background: var(--bg-app);
}
.banner-btn.accent {
  background: #dc3228;
  border-color: #dc3228;
  color: white;
}
.banner-btn.accent:hover {
  background: #c02820;
}

.banner-enter-active {
  transition:
    opacity 0.2s,
    transform 0.2s;
}
.banner-leave-active {
  transition:
    opacity 0.15s,
    transform 0.15s;
}
.banner-enter-from {
  opacity: 0;
  transform: translateY(-8px);
}
.banner-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
