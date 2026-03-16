use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use crate::entity_id::EntityId;
use crate::user::UserId;

/// Metadata about an audio file stored in the project manifest.
/// The actual audio data is transferred peer-to-peer, not through the server.
#[derive(Clone, Debug)]
pub struct AudioFileManifest {
    pub id: EntityId,
    pub filename: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub duration_secs: f32,
    pub sample_rate: u32,
}

/// Download status for a remote audio file.
#[derive(Clone, Debug)]
pub enum DownloadStatus {
    /// File is available locally.
    Available(PathBuf),
    /// File is being downloaded from a peer.
    Downloading { progress: f32 },
    /// File is needed but no peer has it available yet.
    Pending,
    /// Download failed.
    Failed(String),
}

/// Manages peer-to-peer audio file transfers between clients.
pub struct P2PManager {
    /// Known peers and their addresses.
    pub peers: HashMap<UserId, SocketAddr>,
    /// Manifest of all audio files in the project.
    pub manifest: HashMap<EntityId, AudioFileManifest>,
    /// Local cache directory for downloaded audio files.
    pub cache_dir: PathBuf,
    /// Download status for each audio file.
    pub download_status: HashMap<EntityId, DownloadStatus>,
}

impl P2PManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            peers: HashMap::new(),
            manifest: HashMap::new(),
            cache_dir,
            download_status: HashMap::new(),
        }
    }

    /// Register a new audio file in the manifest.
    pub fn register_audio(&mut self, manifest: AudioFileManifest) {
        let id = manifest.id;
        self.download_status.insert(id, DownloadStatus::Available(
            self.cache_dir.join(&manifest.filename),
        ));
        self.manifest.insert(id, manifest);
    }

    /// Check if an audio file is available locally.
    pub fn is_available(&self, id: &EntityId) -> bool {
        matches!(self.download_status.get(id), Some(DownloadStatus::Available(_)))
    }

    /// Get the local path for an audio file, if available.
    pub fn local_path(&self, id: &EntityId) -> Option<&PathBuf> {
        match self.download_status.get(id) {
            Some(DownloadStatus::Available(path)) => Some(path),
            _ => None,
        }
    }

    /// Get download progress for an audio file (0.0 to 1.0), or None if not downloading.
    pub fn download_progress(&self, id: &EntityId) -> Option<f32> {
        match self.download_status.get(id) {
            Some(DownloadStatus::Downloading { progress }) => Some(*progress),
            _ => None,
        }
    }

    /// Request download of a missing audio file from peers.
    pub fn request_download(&mut self, id: EntityId) {
        if !self.is_available(&id) {
            self.download_status.insert(id, DownloadStatus::Pending);
        }
    }

    /// Add a peer for file transfers.
    pub fn add_peer(&mut self, user_id: UserId, addr: SocketAddr) {
        self.peers.insert(user_id, addr);
    }

    /// Remove a peer.
    pub fn remove_peer(&mut self, user_id: &UserId) {
        self.peers.remove(user_id);
    }
}

/// Compute SHA-256 hash of audio data.
pub fn hash_audio_data(samples: &[f32]) -> String {
    use std::fmt::Write;
    // Simple hash for now — will use sha2 crate when added as dependency
    let mut hash: u64 = 0;
    for &s in samples {
        hash = hash.wrapping_mul(31).wrapping_add(s.to_bits() as u64);
    }
    let mut hex = String::with_capacity(16);
    let _ = write!(hex, "{:016x}", hash);
    hex
}
