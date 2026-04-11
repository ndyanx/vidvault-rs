import { ref, computed, watch, onMounted, onUnmounted, nextTick } from "vue";

const DEFAULT_BREAKPOINTS = { 480: 1, 720: 2, 1024: 3, 1440: 4, Infinity: 5 };
const DEFAULT_GAP = 10;
const DEFAULT_VIEWPORT_MARGIN = 400;
const DEFAULT_LOOKAHEAD = 800;
const DEFAULT_IDLE_DELAY = 5_000;

/**
 * useVirtualMasonry
 *
 * A Vue composable that computes a masonry layout with built-in virtual
 * scrolling. Only items near the viewport are rendered; everything else
 * exists only as a pre-calculated position.
 *
 * @param {import('vue').Ref<Array>} items  - Reactive array of any objects
 * @param {object} options
 * @param {(item: any, colWidth: number) => number} options.getItemHeight
 *   Required. Given an item and the current column width, return the card
 *   height in pixels.
 * @param {(item: any) => string|number} [options.getItemKey]
 *   How to derive a unique key from an item. Defaults to `item.id`.
 * @param {Record<number, number>} [options.breakpoints]
 *   Map of max-width → column count.
 * @param {number} [options.gap=10]
 *   Gap between cards in px.
 * @param {number} [options.viewportMargin=400]
 *   Extra px above and below the viewport to keep rendered.
 * @param {number} [options.lookahead=800]
 *   Extra px below the viewport to include in the lookahead zone.
 * @param {number} [options.idleDelay=5000]
 *   ms of scroll inactivity before the idle pass runs.
 * @param {(items: any[], zone: 'visible'|'lookahead'|'idle') => void} [options.onItemsEntered]
 *   Called whenever items enter a zone. Use this to trigger lazy work.
 * @param {number} [options.paddingX=32]
 *   Horizontal padding subtracted from container width when computing column width.
 */
export function useVirtualMasonry(items, options = {}) {
  const {
    getItemHeight,
    getItemKey = (item) => item.id,
    breakpoints = DEFAULT_BREAKPOINTS,
    gap = DEFAULT_GAP,
    viewportMargin = DEFAULT_VIEWPORT_MARGIN,
    lookahead = DEFAULT_LOOKAHEAD,
    idleDelay = DEFAULT_IDLE_DELAY,
    onItemsEntered = null,
    paddingX = 32,
  } = options;

  if (typeof getItemHeight !== "function") {
    throw new Error(
      "[useVirtualMasonry] options.getItemHeight is required and must be a function",
    );
  }

  // ── State ────────────────────────────────────────────────────────────────

  const containerRef = ref(null);
  const colCount = ref(4);
  const colWidth = ref(200);
  const scrollTop = ref(0);
  const viewportHeight = ref(800);
  const containerHeight = ref(0);

  /**
   * Full pre-calculated layout — never trimmed.
   * Each entry: { key, item, x, y, width, height }
   *
   * NOTE: Items are NOT sorted by y in a masonry layout because they are
   * distributed across columns. Column 0 item 2 may have a lower y than
   * column 1 item 1. Binary search on y is therefore NOT safe here —
   * we use a plain filter which is O(n) but purely arithmetic and very
   * fast in practice for typical gallery sizes (< 5k items).
   * For datasets beyond that, a spatial index (e.g. grid buckets) would help.
   */
  const layoutItems = ref([]);

  /**
   * Per-column heights kept alive between builds so that incremental
   * appends (infinite scroll) don't require a full rebuild.
   * Reset to [] on every full buildLayout call.
   */
  let _colHeights = [];

  /**
   * Running max of colHeights — kept incrementally to avoid
   * Math.max(..._colHeights) on every item placement.
   */
  let _maxHeight = 0;

  // ── Layout engine ────────────────────────────────────────────────────────

  function getColsForWidth(w) {
    const sorted = Object.keys(breakpoints)
      .map(Number)
      .sort((a, b) => a - b);
    for (const bp of sorted) {
      if (w < bp) return breakpoints[bp];
    }
    return breakpoints[Infinity] ?? 1;
  }

  /**
   * Full rebuild — used when cols/colWidth change (resize, filter, sort).
   * Resets all state and reprocesses every item from scratch.
   */
  function buildLayout(vids, cols, cw) {
    if (!cw || !cols || !vids.length) {
      layoutItems.value = [];
      containerHeight.value = 0;
      _colHeights = [];
      _maxHeight = 0;
      return;
    }

    _colHeights = new Array(cols).fill(0);
    _maxHeight = 0;
    const result = [];
    _appendItems(vids, result, cols, cw);
    layoutItems.value = result;
    containerHeight.value = _maxHeight;
  }

  /**
   * Incremental append — for infinite scroll or live-appended data.
   *
   * IMPORTANT: Only call this when new items are being added to the END of
   * the existing list. If the list was filtered, sorted, or the column count
   * changed, a full buildLayout will be triggered automatically instead.
   *
   * Safety checks performed:
   * - If _colHeights is empty → full rebuild
   * - If column count changed since last build → full rebuild
   * - If new total doesn't match (existing + new) → full rebuild (list was mutated)
   */
  function appendToLayout(newItems) {
    if (!newItems.length || !colCount.value || !colWidth.value) return;

    const needsFullBuild =
      !_colHeights.length ||
      _colHeights.length !== colCount.value ||
      layoutItems.value.length + newItems.length !== items.value.length;

    if (needsFullBuild) {
      buildLayout(items.value, colCount.value, colWidth.value);
      return;
    }

    const added = [];
    _appendItems(newItems, added, colCount.value, colWidth.value);

    // Mutate in place — Vue tracks array mutations, no new array allocation
    layoutItems.value.push(...added);
    containerHeight.value = _maxHeight;
  }

  /**
   * Core placement loop. Mutates _colHeights and _maxHeight in place.
   * Pushes placed items into `out`.
   */
  function _appendItems(vids, out, cols, cw) {
    for (let i = 0; i < vids.length; i++) {
      const item = vids[i];

      // Find shortest column
      let minCol = 0;
      for (let c = 1; c < cols; c++) {
        if (_colHeights[c] < _colHeights[minCol]) minCol = c;
      }

      const h = getItemHeight(item, cw);
      const cardHeight = Math.max(h | 0, 1);
      const x = minCol * (cw + gap);
      const y = _colHeights[minCol];

      out.push({
        key: getItemKey(item),
        item,
        x,
        y,
        width: cw,
        height: cardHeight,
      });

      _colHeights[minCol] += cardHeight + gap;

      // Incremental max — avoids Math.max(..._colHeights) after the loop
      if (_colHeights[minCol] > _maxHeight) _maxHeight = _colHeights[minCol];
    }
  }

  // ── Virtual scroll ───────────────────────────────────────────────────────
  //
  // O(n) filter over pre-computed positions. Intentionally simple — see note
  // above about why binary search doesn't apply to masonry layouts.

  const visibleItems = computed(() => {
    const top = scrollTop.value - viewportMargin;
    const bottom = scrollTop.value + viewportHeight.value + viewportMargin;
    return layoutItems.value.filter(
      (it) => it.y + it.height > top && it.y < bottom,
    );
  });

  // ── Scroll handler with RAF throttle ─────────────────────────────────────
  //
  // requestAnimationFrame caps processing to one call per paint frame (~16ms),
  // preventing runProcess spam during fast scrolling.

  let rafId = null;

  function onScroll(e) {
    scrollTop.value = e.target.scrollTop;
    if (rafId) return;
    rafId = requestAnimationFrame(() => {
      scheduleProcess(true);
      rafId = null;
    });
  }

  // ── Lazy-work scheduling ─────────────────────────────────────────────────
  //
  // runProcess is called on scroll. We cache the last processed range so
  // micro-movements (< 50px) don't trigger a full O(n) loop unnecessarily.

  let lastProcessTop = -1;
  let lastProcessBottom = -1;
  let processTimer = null;
  let idleTimer = null;

  function runProcess() {
    if (!onItemsEntered) return;

    const top = scrollTop.value;
    const bottom = top + viewportHeight.value;

    // Skip if viewport hasn't moved significantly since last run
    if (
      Math.abs(top - lastProcessTop) < 50 &&
      Math.abs(bottom - lastProcessBottom) < 50
    )
      return;

    lastProcessTop = top;
    lastProcessBottom = bottom;

    const visible = [];
    const lookAheadItems = [];

    for (const it of layoutItems.value) {
      const inViewport = it.y + it.height > top && it.y < bottom;
      const inLookahead =
        it.y + it.height > top - 200 && it.y < bottom + lookahead;

      if (inViewport) visible.push(it.item);
      else if (inLookahead) lookAheadItems.push(it.item);
    }

    if (visible.length) onItemsEntered(visible, "visible");
    if (lookAheadItems.length) onItemsEntered(lookAheadItems, "lookahead");
  }

  function runIdle() {
    if (!onItemsEntered) return;

    const top = scrollTop.value;
    const bottom = top + viewportHeight.value;

    const idleItems = [];
    for (const it of layoutItems.value) {
      const inActive =
        it.y + it.height > top - 200 && it.y < bottom + lookahead;
      if (!inActive) idleItems.push(it.item);
    }

    if (idleItems.length) onItemsEntered(idleItems, "idle");
  }

  function scheduleProcess(immediate = false) {
    clearTimeout(processTimer);
    clearTimeout(idleTimer);

    // Use requestIdleCallback for the idle pass when available —
    // lets the browser schedule it during genuine free time.
    idleTimer = setTimeout(() => {
      if (typeof requestIdleCallback !== "undefined") {
        requestIdleCallback(runIdle, { timeout: 2000 });
      } else {
        runIdle();
      }
    }, idleDelay);

    if (immediate) {
      runProcess();
    } else {
      processTimer = setTimeout(runProcess, 150);
    }
  }

  // ── Scroll position preservation ─────────────────────────────────────────
  // Saves scrollTop before a layout rebuild and restores it on the next tick.
  // Note: this is scrollTop-based, not anchor-based. If the list changes
  // drastically (e.g. heavy filter), the restored position may show different
  // content. Anchor-based restore (tracking the first visible item's y) would
  // be more accurate but adds complexity — a worthwhile future improvement.

  let savedScrollTop = null;

  function saveScroll() {
    if (containerRef.value) savedScrollTop = containerRef.value.scrollTop;
  }

  function restoreScroll() {
    if (containerRef.value && savedScrollTop !== null) {
      containerRef.value.scrollTop = savedScrollTop;
      savedScrollTop = null;
    }
  }

  // ── Layout update (full, triggered by resize or item list change) ─────────

  function updateLayout() {
    if (!containerRef.value) return;
    saveScroll();
    const w = containerRef.value.clientWidth - paddingX;
    viewportHeight.value = containerRef.value.clientHeight;
    const cols = getColsForWidth(w);
    const cw = Math.floor((w - (cols - 1) * gap) / cols);
    colCount.value = cols;
    colWidth.value = cw;
    // Reset process cache so the new layout triggers a fresh pass
    lastProcessTop = -1;
    lastProcessBottom = -1;
    buildLayout(items.value, cols, cw);
  }

  // ── Watchers ─────────────────────────────────────────────────────────────

  watch(
    items,
    () =>
      nextTick(() => {
        updateLayout();
        scheduleProcess(false);
      }),
    { flush: "post" },
  );

  watch(
    visibleItems,
    () => {
      nextTick(() => {
        restoreScroll();
        scheduleProcess(false);
      });
    },
    { flush: "post" },
  );

  // ── Lifecycle ────────────────────────────────────────────────────────────

  let resizeObserver = null;

  onMounted(() => {
    resizeObserver = new ResizeObserver(() => updateLayout());
    if (containerRef.value) resizeObserver.observe(containerRef.value);
    updateLayout();
  });

  onUnmounted(() => {
    resizeObserver?.disconnect();
    clearTimeout(processTimer);
    clearTimeout(idleTimer);
    if (rafId) cancelAnimationFrame(rafId);
  });

  // ── Public API ───────────────────────────────────────────────────────────

  return {
    /** Attach to your scroll container via ref="containerRef" */
    containerRef,
    /** Total canvas height — set as the inner div's height */
    containerHeight,
    /** Items currently near the viewport — render only these */
    visibleItems,
    /** Current column count — useful for skeleton loaders */
    colCount,
    /** Current column width in px */
    colWidth,
    /** Attach to your scroll container's @scroll event */
    onScroll,
    /**
     * Append new items without a full layout rebuild.
     * Use for infinite scroll or live-appended data (e.g. folder watcher).
     * Automatically falls back to a full rebuild if the column count changed
     * or the item list was mutated in a way that isn't a clean append.
     * @param {any[]} newItems
     */
    appendToLayout: (newItems) => {
      appendToLayout(newItems);
      scheduleProcess(false);
    },
  };
}
