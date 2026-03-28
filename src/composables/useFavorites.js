// useFavorites.js — Tauri version
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const favSet = ref(new Set())
let initPromise = null

async function ensureInit() {
  try {
    const ids = await invoke('store_get', { key: 'favorites' })
    favSet.value = new Set(Array.isArray(ids) ? ids : [])
  } catch {
    favSet.value = new Set()
  }
}

export async function initFavorites() {
  if (!initPromise) initPromise = ensureInit()
  return initPromise
}

export function useFavorites() {
  const isFavorite = (id) => favSet.value.has(id)

  const toggle = async (id) => {
    const next = new Set(favSet.value)
    if (next.has(id)) {
      next.delete(id)
    } else {
      next.add(id)
    }
    favSet.value = next
    await invoke('store_set', { key: 'favorites', value: [...next] })
  }

  return { favSet, isFavorite, toggle }
}
