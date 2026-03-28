<script setup>
const props = defineProps({
  count: { type: Number, default: 12 },
  cols: { type: Number, default: 4 }
})

function itemsForCol(colIndex) {
  const base = Math.floor(props.count / props.cols)
  const extra = colIndex < props.count % props.cols ? 1 : 0
  return base + extra
}

// Varied aspect ratios so skeleton feels like real masonry
const ASPECTS = ['9/16', '4/3', '16/9', '1/1', '9/16', '3/4', '16/9', '4/3']
const getAspect = (col, row) => ASPECTS[(col * 2 + row) % ASPECTS.length]
</script>

<template>
  <div class="skeleton-masonry" :style="{ '--cols': cols }">
    <div v-for="col in cols" :key="col" class="skeleton-col">
      <div
        v-for="i in itemsForCol(col - 1)"
        :key="i"
        class="skeleton-card"
        :style="{
          aspectRatio: getAspect(col, i),
          animationDelay: `${(col * 3 + i) * 0.07}s`
        }"
      >
        <div class="shimmer" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.skeleton-masonry {
  display: flex;
  gap: 10px;
  padding: 16px;
  align-items: flex-start;
}

.skeleton-col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.skeleton-card {
  width: 100%;
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  overflow: hidden;
  position: relative;
  animation: skeleton-fade 1.6s ease-in-out infinite;
}

@keyframes skeleton-fade {
  0%,
  100% {
    opacity: 0.5;
  }
  50% {
    opacity: 1;
  }
}

.shimmer {
  position: absolute;
  inset: 0;
  background: linear-gradient(
    105deg,
    transparent 40%,
    rgba(255, 255, 255, 0.06) 50%,
    transparent 60%
  );
  background-size: 200% 100%;
  animation: shimmer-slide 1.8s ease-in-out infinite;
}

[data-theme='light'] .shimmer {
  background: linear-gradient(
    105deg,
    transparent 40%,
    rgba(255, 255, 255, 0.55) 50%,
    transparent 60%
  );
  background-size: 200% 100%;
}

@keyframes shimmer-slide {
  0% {
    background-position: -200% 0;
  }
  100% {
    background-position: 200% 0;
  }
}
</style>
