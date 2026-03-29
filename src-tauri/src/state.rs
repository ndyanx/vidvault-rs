use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OnceCell as AsyncOnceCell, RwLock};
use tokio::time::sleep;

// ── Persisted state shape ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderHistoryEntry {
    pub path: String,
    pub name: String,
    pub last_opened: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    #[serde(default)]
    pub last_folder: Option<String>,
    #[serde(default)]
    pub folder_history: Vec<FolderHistoryEntry>,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            last_folder: None,
            folder_history: vec![],
            favorites: vec![],
            theme: None,
            locale: None,
        }
    }
}

// ── Dimensions cache ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    pub mtime: f64,
    // true when ffprobe found no valid video stream — card is hidden
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_stream: bool,
}

pub type DimCache = HashMap<String, DimEntry>;

// ── Path helpers ──────────────────────────────────────────────────────────────

fn user_data_dir() -> PathBuf {
    // ~/.local/share/vidvault  |  ~/Library/Application Support/vidvault  |  %APPDATA%\vidvault
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vidvault")
}

fn state_path() -> PathBuf {
    user_data_dir().join("app-state.json")
}

fn cache_path() -> PathBuf {
    user_data_dir().join("dimensions-cache.json")
}

pub fn thumbnail_dir() -> PathBuf {
    user_data_dir().join("thumbnails")
}

/// Returns the bucketed thumbnail path for a video file: thumbnails/{xx}/{yy}/{sha1}.jpg
pub fn thumb_path_for_file(file_path: &str) -> PathBuf {
    let mut hasher = Sha1::new();
    hasher.update(file_path.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let l1 = &hash[0..2];
    let l2 = &hash[2..4];
    thumbnail_dir()
        .join(l1)
        .join(l2)
        .join(format!("{}.jpg", hash))
}

// ── AppStateHandle ────────────────────────────────────────────────────────────
//
// Thread-safe handle wrapping AppState + DimCache. Writes are debounced 300 ms
// to avoid hammering disk on rapid successive updates (e.g. scrolling).

#[derive(Clone)]
pub struct AppStateHandle(Arc<Inner>);

struct Inner {
    state: AsyncOnceCell<RwLock<AppState>>,
    dim_cache: AsyncOnceCell<RwLock<DimCache>>,
    state_dirty: Mutex<bool>,
    cache_dirty: Mutex<bool>,
}

impl AppStateHandle {
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            state: AsyncOnceCell::new(),
            dim_cache: AsyncOnceCell::new(),
            state_dirty: Mutex::new(false),
            cache_dirty: Mutex::new(false),
        }))
    }

    // ── App state ─────────────────────────────────────────────────────────────

    pub async fn load(&self) {
        self.get_state().await;
    }

    async fn get_state(&self) -> &RwLock<AppState> {
        self.0
            .state
            .get_or_init(|| async {
                let s = load_json::<AppState>(&state_path())
                    .await
                    .unwrap_or_default();
                RwLock::new(s)
            })
            .await
    }

    pub async fn read_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&AppState) -> R,
    {
        let lock = self.get_state().await;
        f(&*lock.read().await)
    }

    pub async fn mutate_state<F>(&self, f: F)
    where
        F: FnOnce(&mut AppState),
    {
        {
            let lock = self.get_state().await;
            f(&mut *lock.write().await);
        }
        self.schedule_state_write();
    }

    pub async fn get_key(&self, key: &str) -> Value {
        let lock = self.get_state().await;
        let s = lock.read().await;
        let v = serde_json::to_value(&*s).unwrap_or(Value::Null);
        v.get(key).cloned().unwrap_or(Value::Null)
    }

    pub async fn set_key(&self, key: &str, value: Value) {
        {
            let lock = self.get_state().await;
            let mut s = lock.write().await;
            let mut v = serde_json::to_value(&*s).unwrap_or(Value::Object(Default::default()));
            if let Value::Object(ref mut map) = v {
                map.insert(key.to_owned(), value);
            }
            if let Ok(patched) = serde_json::from_value::<AppState>(v) {
                *s = patched;
            }
        }
        self.schedule_state_write();
    }

    fn schedule_state_write(&self) {
        let inner = self.0.clone();
        tokio::spawn(async move {
            {
                let mut dirty = inner.state_dirty.lock().await;
                if *dirty {
                    return;
                }
                *dirty = true;
            }
            sleep(Duration::from_millis(300)).await;
            let lock = inner
                .state
                .get()
                .expect("state cell must be initialized before writing");
            let s = lock.read().await.clone();
            {
                let mut dirty = inner.state_dirty.lock().await;
                *dirty = false;
            }
            let _ = save_json(&state_path(), &s).await;
        });
    }

    // ── Dimensions cache ──────────────────────────────────────────────────────

    pub async fn load_dim_cache(&self) -> Result<DimCache, ()> {
        let lock = self.get_dim_cache().await;
        Ok(lock.read().await.clone())
    }

    async fn get_dim_cache(&self) -> &RwLock<DimCache> {
        self.0
            .dim_cache
            .get_or_init(|| async {
                let c = load_json::<DimCache>(&cache_path())
                    .await
                    .unwrap_or_default();
                RwLock::new(c)
            })
            .await
    }

    pub async fn read_dim_cache<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&DimCache) -> R,
    {
        let lock = self.get_dim_cache().await;
        f(&*lock.read().await)
    }

    pub async fn mutate_dim_cache<F>(&self, f: F)
    where
        F: FnOnce(&mut DimCache),
    {
        {
            let lock = self.get_dim_cache().await;
            f(&mut *lock.write().await);
        }
        self.schedule_cache_write();
    }

    pub async fn upsert_dim_entry(&self, file_path: String, entry: DimEntry) {
        self.mutate_dim_cache(|c| {
            c.insert(file_path, entry);
        })
        .await;
    }

    fn schedule_cache_write(&self) {
        let inner = self.0.clone();
        tokio::spawn(async move {
            {
                let mut dirty = inner.cache_dirty.lock().await;
                if *dirty {
                    return;
                }
                *dirty = true;
            }
            sleep(Duration::from_millis(300)).await;
            let lock = inner
                .dim_cache
                .get()
                .expect("dim_cache cell must be initialized before writing");
            let c = lock.read().await.clone();
            {
                let mut dirty = inner.cache_dirty.lock().await;
                *dirty = false;
            }
            let _ = save_json(&cache_path(), &c).await;
        });
    }
}

// ── JSON I/O ──────────────────────────────────────────────────────────────────

async fn load_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&raw).ok()
}

async fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    tokio::fs::write(path, json).await
}
