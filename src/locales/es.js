export default {
  // TitleBar
  titlebar: {
    openFolder: 'Abrir carpeta',
    dialogTitle: 'Seleccionar carpeta de videos',
    loading: 'Cargando…',
    closeFolder: 'Cerrar carpeta',
    folderPillLabel: 'Carpeta actual: {name}. Haz clic para ver carpetas recientes.',
    removeLabel: 'Eliminar {name} del historial',
    recents: 'Recientes',
    openOther: 'Abrir otra carpeta…',
    themeDark: 'Modo oscuro',
    themeLight: 'Modo claro',
    remove: 'Eliminar',
    timeNow: 'ahora',
    timeMinutes: 'hace {m}m',
    timeHours: 'hace {h}h',
    timeDays: 'hace {d}d'
  },

  // EmptyState
  empty: {
    title: 'Sin videos',
    desc: 'Abre una carpeta con tus videos locales y aparecerán organizados en un masonry gallery.',
    openBtn: 'Abrir carpeta',
    hint: 'mp4 · mov · mkv · avi · webm y más',
    dropHere: 'Suelta la carpeta aquí',
    recents: 'Recientes',
    removeFromHistory: 'Eliminar del historial'
  },

  // GalleryPanel
  gallery: {
    searchPlaceholder: 'Buscar por nombre…',
    favorites: 'Favoritos',
    onlyFavorites: 'Solo favoritos',
    favorite: 'Favorito',
    sortDate: 'Fecha',
    sortName: 'Nombre',
    sortSize: 'Tamaño',
    sortDuration: 'Duración',
    resultCount: '{n} resultado | {n} resultados',
    videoCount: '{n} video | {n} videos',
    of: 'de',
    noResults: 'Sin resultados para',
    noVideos: 'No se encontraron videos en esta carpeta.',
    dropHere: 'Suelta para abrir esta carpeta',
    play: 'Reproducir',
    addFavorite: 'Marcar favorito',
    removeFavorite: 'Quitar favorito',
    showInFolder: 'Mostrar en carpeta',
    copyPath: 'Copiar ruta',
    clearSearch: 'Limpiar búsqueda'
  },

  // VideoModal
  modal: {
    copyPath: 'Copiar ruta',
    copied: '¡Copiado!',
    showInFolder: 'Mostrar en carpeta',
    close: 'Cerrar (Esc)',
    prev: 'Anterior (←)',
    next: 'Siguiente (→)'
  },

  // App.vue — error banner
  error: {
    notFound: 'La carpeta {folder} ya no existe o fue movida. Se eliminó del historial.',
    readError: 'No se pudo leer la carpeta. Verifica los permisos.',
    openOther: 'Abrir otra',
    close: 'Cerrar'
  }
}
