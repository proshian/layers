use std::path::{Path, PathBuf};

use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::types::{Bytes, SurrealValue};
use surrealdb::Surreal;

use super::models::*;

// ---------------------------------------------------------------------------
// Audio stored as original encoded file (like RemoteAudioFile in remote.rs)
// ---------------------------------------------------------------------------

#[derive(Clone, SurrealValue)]
pub struct StoredAudioData {
    pub waveform_id: String,
    pub file_data: Bytes,
    pub extension: String,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub struct Storage {
    rt: tokio::runtime::Runtime,
    temp_projects_dir: PathBuf,
    index_db: Surreal<Db>,
    project_db: Option<Surreal<Db>>,
    current_project_path: Option<PathBuf>,
    is_temp: bool,
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn open_db(rt: &tokio::runtime::Runtime, path: &Path) -> Option<Surreal<Db>> {
    rt.block_on(async {
        let db = Surreal::new::<RocksDb>(path.to_str()?).await.ok()?;
        db.use_ns("layers").use_db("canvas").await.ok()?;
        Some(db)
    })
}

impl Storage {
    /// Opens the global index DB and prepares the temp projects directory.
    pub fn open(base_path: &Path) -> Option<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_stack_size(10 * 1024 * 1024)
            .build()
            .ok()?;

        let temp_projects_dir = base_path.join("projects");
        std::fs::create_dir_all(&temp_projects_dir).ok()?;

        let index_db_path = base_path.join("index.db");
        let index_db = open_db(&rt, &index_db_path)?;

        log::info!("Storage opened at {:?}", base_path);
        Some(Storage {
            rt,
            temp_projects_dir,
            index_db,
            project_db: None,
            current_project_path: None,
            is_temp: false,
        })
    }

    // -----------------------------------------------------------------------
    // Project lifecycle
    // -----------------------------------------------------------------------

    /// Opens a per-project DB inside `path/db/`.
    pub fn open_project(&mut self, path: &Path) -> bool {
        self.close_project();
        let db_path = path.join("db");
        std::fs::create_dir_all(&db_path).ok();
        match open_db(&self.rt, &db_path) {
            Some(db) => {
                self.project_db = Some(db);
                self.current_project_path = Some(path.to_path_buf());
                // Check if this is a temp project by looking at the index
                let key = path.to_string_lossy().to_string();
                self.is_temp = self
                    .rt
                    .block_on(async {
                        let entry: Option<ProjectIndexEntry> =
                            self.index_db.select(("projects", &*key)).await.ok()?;
                        entry.map(|e| e.is_temp)
                    })
                    .unwrap_or(false);
                log::info!("Opened project DB at {:?} (temp={})", db_path, self.is_temp);
                true
            }
            None => {
                log::error!("Failed to open project DB at {:?}", db_path);
                false
            }
        }
    }

    /// Drops the current project DB.
    pub fn close_project(&mut self) {
        self.project_db = None;
        self.current_project_path = None;
        self.is_temp = false;
    }

    /// Creates a new temp project under `~/.layers/projects/tmp-<ts>/`.
    pub fn create_temp_project(&mut self) -> Option<PathBuf> {
        let ts = now_ts();
        let dir = self.temp_projects_dir.join(format!("tmp-{ts}"));
        std::fs::create_dir_all(&dir).ok()?;
        if !self.open_project(&dir) {
            return None;
        }
        self.is_temp = true;

        // Add to global index
        let entry = ProjectIndexEntry {
            name: "Untitled".to_string(),
            path: dir.to_string_lossy().to_string(),
            is_temp: true,
            created_at: ts,
            updated_at: ts,
        };
        let key = dir.to_string_lossy().to_string();
        self.upsert_index_entry(&key, entry);
        log::info!("Created temp project at {:?}", dir);
        Some(dir)
    }

    /// Moves/copies the current project folder to `dest`, reopens DB there.
    pub fn save_project_to(&mut self, dest: &Path) -> bool {
        let src = match &self.current_project_path {
            Some(p) => p.clone(),
            None => return false,
        };
        if src == dest {
            return true;
        }

        // Close current DB so RocksDB lock is released
        let was_temp = self.is_temp;
        self.project_db = None;

        // Create dest and copy contents
        if let Err(e) = copy_dir_all(&src, dest) {
            log::error!("Failed to copy project to {:?}: {e}", dest);
            // Try to reopen at old location
            self.open_project(&src);
            self.is_temp = was_temp;
            return false;
        }

        // Remove old temp dir
        if was_temp {
            let _ = std::fs::remove_dir_all(&src);
            // Remove old index entry
            self.delete_index_entry(&src.to_string_lossy());
        }

        // Reopen at new location
        if !self.open_project(dest) {
            log::error!("Failed to reopen project at {:?}", dest);
            return false;
        }
        self.is_temp = false;

        // Update index
        let entry = ProjectIndexEntry {
            name: String::new(), // will be updated on next save_project_state
            path: dest.to_string_lossy().to_string(),
            is_temp: false,
            created_at: now_ts(),
            updated_at: now_ts(),
        };
        let key = dest.to_string_lossy().to_string();
        self.upsert_index_entry(&key, entry);
        true
    }

    pub fn is_temp_project(&self) -> bool {
        self.is_temp
    }

    pub fn current_project_path(&self) -> Option<&Path> {
        self.current_project_path.as_deref()
    }

    // -----------------------------------------------------------------------
    // project.json
    // -----------------------------------------------------------------------

    pub fn write_project_json(&self, name: &str) {
        let path = match &self.current_project_path {
            Some(p) => p.join("project.json"),
            None => return,
        };
        let meta = ProjectMeta {
            name: name.to_string(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&meta) {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("Failed to write project.json: {e}");
            }
        }
    }

    pub fn read_project_json(path: &Path) -> Option<ProjectMeta> {
        let json_path = path.join("project.json");
        let contents = std::fs::read_to_string(&json_path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    // -----------------------------------------------------------------------
    // Project state (per-project DB)
    // -----------------------------------------------------------------------

    pub fn save_project_state(&self, state: ProjectState) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };

        self.write_project_json(&state.name);

        let result = self.rt.block_on(async {
            let _: Option<ProjectState> = db.upsert(("state", "main")).content(state).await?;
            Ok::<(), surrealdb::Error>(())
        });
        if let Err(e) = result {
            log::error!("Failed to save project state: {e}");
        }

        // Update index entry name + timestamp
        if let Some(path) = &self.current_project_path {
            let key = path.to_string_lossy().to_string();
            self.update_index_timestamp(&key);
        }
    }

    pub fn load_project_state(&self) -> Option<ProjectState> {
        let db = self.project_db.as_ref()?;
        self.rt.block_on(async {
            let state: Option<ProjectState> = db.select(("state", "main")).await.ok()?;
            state
        })
    }

    // -----------------------------------------------------------------------
    // Audio data (per-project DB) — keyed by EntityId string
    // -----------------------------------------------------------------------

    pub fn save_audio(
        &self,
        waveform_id: &str,
        file_bytes: &[u8],
        extension: &str,
    ) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };
        let data = StoredAudioData {
            waveform_id: waveform_id.to_string(),
            file_data: Bytes::from(file_bytes.to_vec()),
            extension: extension.to_string(),
        };
        let result = self.rt.block_on(async {
            let _: Option<StoredAudioData> =
                db.upsert(("audio", waveform_id)).content(data).await?;
            Ok::<(), surrealdb::Error>(())
        });
        if let Err(e) = result {
            log::error!("Failed to save audio data for waveform {waveform_id}: {e}");
        }
    }

    /// Returns (file_bytes, extension) for the given waveform, or None.
    pub fn load_audio(&self, waveform_id: &str) -> Option<(Vec<u8>, String)> {
        let db = self.project_db.as_ref()?;
        self.rt.block_on(async {
            let data: Option<StoredAudioData> =
                db.select(("audio", waveform_id)).await.ok()?;
            data
        })
        .map(|d| (d.file_data.into_inner().to_vec(), d.extension))
    }

    // -----------------------------------------------------------------------
    // Peaks data (per-project DB) — keyed by EntityId string
    // -----------------------------------------------------------------------

    pub fn save_peaks(
        &self,
        waveform_id: &str,
        block_size: u64,
        left_peaks: &[f32],
        right_peaks: &[f32],
    ) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };
        let data = StoredPeaks {
            waveform_id: waveform_id.to_string(),
            block_size,
            left_peaks: peaks_f32_to_u8(left_peaks),
            right_peaks: peaks_f32_to_u8(right_peaks),
        };
        let result = self.rt.block_on(async {
            let _: Option<StoredPeaks> =
                db.upsert(("peaks", waveform_id)).content(data).await?;
            Ok::<(), surrealdb::Error>(())
        });
        if let Err(e) = result {
            log::error!("Failed to save peaks for waveform {waveform_id}: {e}");
        }
    }

    /// Returns (block_size, left_peaks_f32, right_peaks_f32) or None.
    pub fn load_peaks(&self, waveform_id: &str) -> Option<(u64, Vec<f32>, Vec<f32>)> {
        let db = self.project_db.as_ref()?;
        let stored = self.rt.block_on(async {
            let data: Option<StoredPeaks> =
                db.select(("peaks", waveform_id)).await.ok()?;
            data
        })?;
        Some((
            stored.block_size,
            peaks_u8_to_f32(&stored.left_peaks),
            peaks_u8_to_f32(&stored.right_peaks),
        ))
    }

    /// Clear all audio and peaks records (called before full rewrite on save).
    pub fn clear_audio_and_peaks(&self) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };
        let _ = self.rt.block_on(async {
            let _: Vec<StoredAudioData> = db.delete("audio").await?;
            let _: Vec<StoredPeaks> = db.delete("peaks").await?;
            Ok::<(), surrealdb::Error>(())
        });
    }

    // -----------------------------------------------------------------------
    // Global index
    // -----------------------------------------------------------------------

    pub fn list_projects(&self) -> Vec<ProjectIndexEntry> {
        let mut entries = self.rt
            .block_on(async {
                let entries: Vec<ProjectIndexEntry> =
                    self.index_db.select("projects").await.ok()?;
                Some(entries)
            })
            .unwrap_or_default();
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        entries
    }

    pub fn delete_project(&mut self, path: &str) {
        // Remove from index
        self.delete_index_entry(path);
        // Remove folder
        let _ = std::fs::remove_dir_all(path);
        log::info!("Deleted project at {path}");
    }

    pub fn update_index_name(&self, path: &str, name: &str) {
        let _result = self.rt.block_on(async {
            // Load existing entry, update name
            let existing: Option<ProjectIndexEntry> =
                self.index_db.select(("projects", path)).await.ok()?;
            if let Some(mut entry) = existing {
                entry.name = name.to_string();
                entry.updated_at = now_ts();
                let _: Option<ProjectIndexEntry> = self
                    .index_db
                    .upsert(("projects", path))
                    .content(entry)
                    .await
                    .ok()?;
            }
            Some(())
        });
    }

    // -----------------------------------------------------------------------
    // Index helpers (private)
    // -----------------------------------------------------------------------

    fn upsert_index_entry(&self, key: &str, entry: ProjectIndexEntry) {
        let _ = self.rt.block_on(async {
            let _: Option<ProjectIndexEntry> = self
                .index_db
                .upsert(("projects", key))
                .content(entry)
                .await?;
            Ok::<(), surrealdb::Error>(())
        });
    }

    fn delete_index_entry(&self, key: &str) {
        let _ = self.rt.block_on(async {
            let _: Option<ProjectIndexEntry> = self.index_db.delete(("projects", key)).await?;
            Ok::<(), surrealdb::Error>(())
        });
    }

    fn update_index_timestamp(&self, key: &str) {
        let _ = self.rt.block_on(async {
            let existing: Option<ProjectIndexEntry> =
                self.index_db.select(("projects", key)).await.ok()?;
            if let Some(mut entry) = existing {
                entry.updated_at = now_ts();
                let _: Option<ProjectIndexEntry> = self
                    .index_db
                    .upsert(("projects", key))
                    .content(entry)
                    .await
                    .ok()?;
            }
            Some(())
        });
    }
}

// ---------------------------------------------------------------------------
// Peak quantization helpers
// ---------------------------------------------------------------------------

fn peaks_f32_to_u8(peaks: &[f32]) -> Vec<u8> {
    peaks.iter().map(|&p| (p.clamp(0.0, 1.0) * 255.0) as u8).collect()
}

fn peaks_u8_to_f32(peaks: &[u8]) -> Vec<f32> {
    peaks.iter().map(|&b| b as f32 / 255.0).collect()
}

// ---------------------------------------------------------------------------
// Utility: recursive dir copy
// ---------------------------------------------------------------------------

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Default base path
// ---------------------------------------------------------------------------

pub fn default_base_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".layers")
}
