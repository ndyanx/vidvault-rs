export default {
  // TitleBar
  titlebar: {
    openFolder: 'Open folder',
    dialogTitle: 'Select video folder',
    loading: 'Loading…',
    closeFolder: 'Close folder',
    folderPillLabel: 'Current folder: {name}. Click to open recent folders.',
    removeLabel: 'Remove {name} from history',
    recents: 'Recent',
    openOther: 'Open another folder…',
    themeDark: 'Dark mode',
    themeLight: 'Light mode',
    remove: 'Remove',
    timeNow: 'just now',
    timeMinutes: '{m}m ago',
    timeHours: '{h}h ago',
    timeDays: '{d}d ago'
  },

  // EmptyState
  empty: {
    title: 'No videos',
    desc: 'Open a folder with your local videos and they will appear organized in a masonry gallery.',
    openBtn: 'Open folder',
    hint: 'mp4 · mov · mkv · avi · webm and more',
    dropHere: 'Drop folder here',
    recents: 'Recent',
    removeFromHistory: 'Remove from history'
  },

  // GalleryPanel
  gallery: {
    searchPlaceholder: 'Search by name…',
    favorites: 'Favorites',
    onlyFavorites: 'Favorites only',
    favorite: 'Favorite',
    sortDate: 'Date',
    sortName: 'Name',
    sortSize: 'Size',
    sortDuration: 'Duration',
    resultCount: '{n} result | {n} results',
    videoCount: '{n} video | {n} videos',
    of: 'of',
    noResults: 'No results for',
    noVideos: 'No videos found in this folder.',
    dropHere: 'Drop to open this folder',
    play: 'Play',
    addFavorite: 'Add to favorites',
    removeFavorite: 'Remove from favorites',
    showInFolder: 'Show in folder',
    copyPath: 'Copy path',
    clearSearch: 'Clear search'
  },

  // VideoModal
  modal: {
    copyPath: 'Copy path',
    copied: 'Copied!',
    showInFolder: 'Show in folder',
    close: 'Close (Esc)',
    prev: 'Previous (←)',
    next: 'Next (→)'
  },

  // App.vue — error banner
  error: {
    notFound: 'Folder {folder} no longer exists or was moved. It has been removed from history.',
    readError: 'Could not read the folder. Check folder permissions.',
    openOther: 'Open another',
    close: 'Close'
  }
}
