use std::sync::Arc;

use surrealdb::engine::remote::ws::{Client, Ws};
use surrealdb::types::{Bytes, SurrealValue};
use surrealdb::Surreal;

#[derive(Clone, SurrealValue)]
struct RemoteAudioFile {
    waveform_id: String,
    file_data: Bytes,
    extension: String,
}

pub struct RemoteStorage {
    db: Surreal<Client>,
    rt: Arc<tokio::runtime::Runtime>,
}

impl RemoteStorage {
    pub fn connect(url: &str, rt: Arc<tokio::runtime::Runtime>) -> Option<Self> {
        let addr = url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://");
        println!("[RemoteStorage] Connecting to SurrealDB at {addr}...");
        let db = rt.block_on(async {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                Surreal::new::<Ws>(addr),
            ).await;
            let db = match result {
                Ok(Ok(db)) => db,
                Ok(Err(e)) => {
                    eprintln!("[RemoteStorage] Connection error: {e}");
                    return None;
                }
                Err(_) => {
                    eprintln!("[RemoteStorage] Connection timed out after 5s");
                    return None;
                }
            };
            db.use_ns("layers").use_db("meta").await.ok()?;
            Some(db)
        })?;
        println!("[RemoteStorage] Connected to SurrealDB at {url}");
        Some(RemoteStorage { db, rt })
    }

    pub fn use_project(&self, project_id: &str) {
        let db_name = format!("project_{project_id}");
        let db = self.db.clone();
        let result = self.run_on_rt(async move {
            db.use_ns("layers").use_db(&db_name).await
        });
        if let Err(e) = result {
            log::error!("[RemoteStorage] Failed to switch to project DB: {e}");
        } else {
            log::info!("[RemoteStorage] Using project DB: project_{project_id}");
        }
    }

    /// Spawn an async future on the tokio runtime worker threads and block
    /// the calling thread until it completes. Unlike `rt.block_on()`, this
    /// is safe to call from any thread (including `std::thread::spawn`).
    fn run_on_rt<F, T>(&self, future: F) -> T
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.rt.spawn(async move {
            let result = future.await;
            let _ = tx.send(result);
        });
        rx.recv().expect("runtime task dropped before completing")
    }

    pub fn save_audio(&self, waveform_id: &str, file_bytes: &[u8], extension: &str) {
        let byte_len = file_bytes.len();
        let mb = byte_len as f64 / (1024.0 * 1024.0);
        println!(
            "[RemoteStorage] save_audio: wf={waveform_id} ext={extension} size={mb:.2} MB"
        );
        let data = RemoteAudioFile {
            waveform_id: waveform_id.to_string(),
            file_data: Bytes::from(file_bytes.to_vec()),
            extension: extension.to_string(),
        };
        let db = self.db.clone();
        let wf_id = waveform_id.to_string();
        let result: Result<(), surrealdb::Error> = self.run_on_rt(async move {
            let _: Option<RemoteAudioFile> =
                db.upsert(("audio", &*wf_id)).content(data).await?;
            Ok(())
        });
        match &result {
            Ok(()) => println!("[RemoteStorage] save_audio OK for {waveform_id}"),
            Err(e) => eprintln!("[RemoteStorage] save_audio FAILED for {waveform_id}: {e}"),
        }
    }

    /// Returns (file_bytes, extension) for the given waveform, or None.
    pub fn load_audio(&self, waveform_id: &str) -> Option<(Vec<u8>, String)> {
        println!("[RemoteStorage] load_audio: wf={waveform_id}");
        let db = self.db.clone();
        let wf_id = waveform_id.to_string();
        let result = self.run_on_rt(async move {
            let data: Option<RemoteAudioFile> =
                db.select(("audio", &*wf_id)).await.ok()?;
            data
        });
        let found = result.is_some();
        println!("[RemoteStorage] load_audio: wf={waveform_id} found={found}");
        result.map(|r| (r.file_data.into_inner().to_vec(), r.extension))
    }
}
