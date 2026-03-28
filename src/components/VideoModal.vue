<script setup>
import { ref, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { formatSize, formatDuration } from '../utils/format.js'

const { t } = useI18n()

const props = defineProps({
  video: { type: Object, default: null },
  hasPrev: { type: Boolean, default: false },
  hasNext: { type: Boolean, default: false }
})

const emit = defineEmits(['close', 'prev', 'next'])

const videoRef = ref(null)
const copied = ref(false)

watch(
  () => props.video,
  async (newVideo) => {
    if (videoRef.value && !videoRef.value.paused) {
      videoRef.value.pause()
    }
    if (newVideo && videoRef.value) {
      await nextTick()
      videoRef.value.load()
      videoRef.value.play().catch(() => {})
    }
  }
)

function handleKey(e) {
  if (!props.video) return
  if (e.key === 'Escape') {
    emit('close')
    return
  }
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') emit('next')
  if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') emit('prev')
}

onMounted(() => document.addEventListener('keydown', handleKey))
onUnmounted(() => document.removeEventListener('keydown', handleKey))

async function copyPath() {
  await invoke('shell_copy_path', { filePath: props.video.filePath })
  copied.value = true
  setTimeout(() => {
    copied.value = false
  }, 1800)
}

function showInFolder() {
  if (!props.video) return
  invoke('shell_show_in_folder', { filePath: props.video.filePath })
}
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="video" class="modal-backdrop" @click.self="$emit('close')">
        <div class="modal-container">
          <!-- Header -->
          <div class="modal-header">
            <div class="modal-file-info">
              <span class="modal-ext-badge">{{ video.ext }}</span>
              <span class="modal-filename selectable">{{ video.fileName }}</span>
            </div>

            <div class="modal-meta">
              <span class="meta-chip">{{ formatSize(video.size) }}</span>
              <span v-if="video.width && video.height" class="meta-chip"
                >{{ video.width }}×{{ video.height }}</span
              >
              <span v-if="formatDuration(video.duration)" class="meta-chip">{{
                formatDuration(video.duration)
              }}</span>
            </div>

            <!-- Action buttons -->
            <div class="modal-actions">
              <button
                class="action-btn"
                @click="copyPath"
                :title="copied ? t('modal.copied') : t('modal.copyPath')"
                :aria-label="copied ? t('modal.copied') : t('modal.copyPath')"
              >
                <svg
                  v-if="!copied"
                  width="13"
                  height="13"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                >
                  <rect x="9" y="9" width="13" height="13" rx="2" />
                  <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                </svg>
                <svg
                  v-else
                  width="13"
                  height="13"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2.5"
                >
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              </button>
              <button class="action-btn" @click="showInFolder" :title="t('modal.showInFolder')" :aria-label="t('modal.showInFolder')">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor">
                  <path
                    d="M1 3.5A1.5 1.5 0 0 1 2.5 2h3.764c.414 0 .811.162 1.104.451l.897.898A1.5 1.5 0 0 0 9.37 3.8H13.5A1.5 1.5 0 0 1 15 5.3v7.2A1.5 1.5 0 0 1 13.5 14h-11A1.5 1.5 0 0 1 1 12.5z"
                  />
                </svg>
              </button>
            </div>

            <button class="close-btn" @click="$emit('close')" :title="t('modal.close')" :aria-label="t('modal.close')">
              <svg
                width="16"
                height="16"
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

          <!-- Video + nav arrows -->
          <div class="modal-video-area">
            <button
              v-if="hasPrev"
              class="nav-btn nav-prev"
              @click="$emit('prev')"
              :title="t('modal.prev')"
              :aria-label="t('modal.prev')"
            >
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
              >
                <polyline points="15 18 9 12 15 6" />
              </svg>
            </button>

            <div class="modal-video-wrap">
              <video
                ref="videoRef"
                :src="video.videoUrl"
                controls
                autoplay
                loop
                class="modal-video"
              />
            </div>

            <button
              v-if="hasNext"
              class="nav-btn nav-next"
              @click="$emit('next')"
              :title="t('modal.next')"
              :aria-label="t('modal.next')"
            >
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
              >
                <polyline points="9 18 15 12 9 6" />
              </svg>
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 2000;
  background: rgba(0, 0, 0, 0.82);
  backdrop-filter: blur(8px);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}

.modal-container {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-width: min(90vw, 1000px);
  max-height: 92vh;
  gap: 12px;
}

.modal-header {
  display: flex;
  align-items: center;
  gap: 10px;
}

.modal-file-info {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.modal-ext-badge {
  font-family: var(--font-mono);
  font-size: 9px;
  font-weight: 500;
  letter-spacing: 0.06em;
  color: var(--accent);
  background: var(--accent-subtle);
  border: 1px solid var(--accent);
  padding: 2px 6px;
  border-radius: 4px;
  flex-shrink: 0;
}

.modal-filename {
  font-family: var(--font-mono);
  font-size: 12px;
  color: rgba(255, 255, 255, 0.75);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.modal-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.meta-chip {
  font-family: var(--font-mono);
  font-size: 10px;
  color: rgba(255, 255, 255, 0.45);
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid rgba(255, 255, 255, 0.12);
  padding: 2px 8px;
  border-radius: 20px;
}

.modal-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-sm);
  cursor: pointer;
  color: rgba(255, 255, 255, 0.55);
  transition:
    background 0.15s,
    color 0.15s,
    border-color 0.15s;
}
.action-btn:hover {
  background: rgba(255, 255, 255, 0.14);
  border-color: rgba(255, 255, 255, 0.22);
  color: rgba(255, 255, 255, 0.9);
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: var(--radius-sm);
  cursor: pointer;
  color: rgba(255, 255, 255, 0.7);
  transition:
    background 0.15s,
    color 0.15s;
  flex-shrink: 0;
}
.close-btn:hover {
  background: rgba(220, 50, 40, 0.7);
  border-color: transparent;
  color: white;
}

.modal-video-area {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  gap: 10px;
}

.modal-video-wrap {
  flex: 1;
  min-width: 0;
  min-height: 0;
  border-radius: var(--radius-lg);
  overflow: hidden;
  background: #000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.modal-video {
  width: 100%;
  height: 100%;
  max-height: calc(92vh - 80px);
  object-fit: contain;
  display: block;
}

.nav-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  flex-shrink: 0;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 50%;
  cursor: pointer;
  color: rgba(255, 255, 255, 0.65);
  transition:
    background 0.15s,
    color 0.15s,
    transform 0.1s;
}
.nav-btn:hover {
  background: rgba(255, 255, 255, 0.18);
  color: white;
}
.nav-btn:active {
  transform: scale(0.93);
}

/* Transitions */
.modal-enter-active {
  transition: opacity 0.22s ease;
}
.modal-leave-active {
  transition: opacity 0.18s ease;
}
.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-active .modal-container {
  animation: modal-slide-up 0.22s ease;
}
.modal-leave-active .modal-container {
  animation: modal-slide-down 0.18s ease;
}

@keyframes modal-slide-up {
  from {
    transform: translateY(20px) scale(0.98);
    opacity: 0;
  }
  to {
    transform: translateY(0) scale(1);
    opacity: 1;
  }
}
@keyframes modal-slide-down {
  from {
    transform: translateY(0) scale(1);
    opacity: 1;
  }
  to {
    transform: translateY(12px) scale(0.98);
    opacity: 0;
  }
}

@media (max-width: 600px) {
  .modal-backdrop {
    padding: 12px;
  }
  .modal-video {
    max-height: calc(95vh - 70px);
  }
  .nav-btn {
    width: 32px;
    height: 32px;
  }
}
</style>
