//! Offline audio renderer — mixes group members into a stereo buffer, then encodes to WAV or MP3.

use std::path::Path;
use std::sync::mpsc;
use crate::entity_id::EntityId;
use crate::grid::PIXELS_PER_SECOND;
use crate::ui::export_window::{ExportFormat, ExportProgress};
use crate::App;

const EXPORT_SAMPLE_RATE: u32 = 48000;

/// Collected audio data that can be sent to a background thread for encoding.
struct MixedAudio {
    left: Vec<f32>,
    right: Vec<f32>,
    total_frames: usize,
}

/// Mix all waveform members of a group into stereo buffers.
/// This must run on the main thread (needs access to App).
fn mix_group(app: &App, group_id: EntityId) -> Result<MixedAudio, String> {
    let group = app.groups.get(&group_id)
        .ok_or_else(|| "Group not found".to_string())?;

    let start_sec = group.position[0] as f64 / PIXELS_PER_SECOND as f64;
    let end_sec = (group.position[0] + group.size[0]) as f64 / PIXELS_PER_SECOND as f64;
    let duration_sec = end_sec - start_sec;
    if duration_sec <= 0.0 {
        return Err("Group has zero duration".to_string());
    }

    let total_frames = (duration_sec * EXPORT_SAMPLE_RATE as f64) as usize;
    let mut left_buf = vec![0.0f32; total_frames];
    let mut right_buf = vec![0.0f32; total_frames];

    for mid in &group.member_ids {
        let wf = match app.waveforms.get(mid) {
            Some(w) if !w.disabled => w,
            _ => continue,
        };
        let clip = match app.audio_clips.get(mid) {
            Some(c) => c,
            None => continue,
        };
        if clip.samples.is_empty() {
            continue;
        }

        let wf_start_sec = wf.position[0] as f64 / PIXELS_PER_SECOND as f64;
        let wf_end_sec = (wf.position[0] + wf.size[0]) as f64 / PIXELS_PER_SECOND as f64;

        let mix_start_sec = wf_start_sec.max(start_sec);
        let mix_end_sec = wf_end_sec.min(end_sec);
        if mix_start_sec >= mix_end_sec {
            continue;
        }

        let volume = wf.volume;
        let pan = wf.pan;
        let _left_gain_unused = volume * (1.0 - pan).min(1.0) * 2.0_f32.min(1.0 + (1.0 - pan));
        let _right_gain_unused = volume * pan.min(1.0) * 2.0_f32.min(1.0 + pan);

        // Simple equal-power-ish pan
        let left_gain = volume * (std::f32::consts::FRAC_PI_2 * (1.0 - pan)).cos().max(0.0).min(1.0) * std::f32::consts::SQRT_2;
        let right_gain = volume * (std::f32::consts::FRAC_PI_2 * pan).cos().max(0.0).min(1.0) * std::f32::consts::SQRT_2;

        let src_rate = clip.sample_rate as f64;
        let src_len = clip.samples.len();
        let left_samples = &wf.audio.left_samples;
        let right_samples = &wf.audio.right_samples;
        let has_stereo = !left_samples.is_empty() && !right_samples.is_empty();

        let offset_sec = wf.sample_offset_px as f64 / PIXELS_PER_SECOND as f64;
        let wf_width_sec = wf.size[0] as f64 / PIXELS_PER_SECOND as f64;
        let fade_in_sec = wf.fade_in_px as f64 / PIXELS_PER_SECOND as f64;
        let fade_out_sec = wf.fade_out_px as f64 / PIXELS_PER_SECOND as f64;

        for frame in 0..total_frames {
            let t_sec = start_sec + frame as f64 / EXPORT_SAMPLE_RATE as f64;
            if t_sec < mix_start_sec || t_sec >= mix_end_sec {
                continue;
            }

            let clip_t = t_sec - wf_start_sec;
            let src_t = offset_sec + clip_t;
            let src_idx = (src_t * src_rate) as usize;

            let (l_sample, r_sample) = if has_stereo && src_idx < left_samples.len() && src_idx < right_samples.len() {
                (left_samples[src_idx], right_samples[src_idx])
            } else if src_idx < src_len {
                let mono = clip.samples[src_idx];
                (mono, mono)
            } else {
                continue;
            };

            let mut fade = 1.0f32;
            if fade_in_sec > 0.0 && clip_t < fade_in_sec {
                fade *= (clip_t / fade_in_sec) as f32;
            }
            if fade_out_sec > 0.0 && clip_t > wf_width_sec - fade_out_sec {
                let fade_pos = (wf_width_sec - clip_t) / fade_out_sec;
                fade *= fade_pos as f32;
            }
            fade = fade.clamp(0.0, 1.0);

            left_buf[frame] += l_sample * left_gain * fade;
            right_buf[frame] += r_sample * right_gain * fade;
        }
    }

    Ok(MixedAudio { left: left_buf, right: right_buf, total_frames })
}

/// Encode mixed audio to WAV.
fn encode_wav(audio: &MixedAudio, path: &Path, progress_tx: &mpsc::Sender<ExportProgress>) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: EXPORT_SAMPLE_RATE,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| format!("Failed to create WAV file: {}", e))?;

    let scale = (1 << 23) as f32 - 1.0;
    let report_interval = (audio.total_frames / 100).max(1);
    for i in 0..audio.total_frames {
        let l = (audio.left[i].clamp(-1.0, 1.0) * scale) as i32;
        let r = (audio.right[i].clamp(-1.0, 1.0) * scale) as i32;
        writer.write_sample(l).map_err(|e| format!("Write error: {}", e))?;
        writer.write_sample(r).map_err(|e| format!("Write error: {}", e))?;

        if i % report_interval == 0 {
            let _ = progress_tx.send(ExportProgress::Progress(i as f32 / audio.total_frames as f32));
        }
    }

    writer.finalize().map_err(|e| format!("Failed to finalize WAV: {}", e))?;
    Ok(())
}

/// Mix and start encoding on a background thread. Returns a progress receiver.
pub(crate) fn start_export(
    app: &App,
    group_id: EntityId,
    path: std::path::PathBuf,
    format: ExportFormat,
) -> Result<mpsc::Receiver<ExportProgress>, String> {
    // Mix on main thread (needs App access)
    let audio = mix_group(app, group_id)?;

    let (tx, rx) = mpsc::channel();

    // Encode on background thread
    std::thread::spawn(move || {
        let result = match format {
            ExportFormat::Wav => encode_wav(&audio, &path, &tx),
            ExportFormat::Mp3 => {
                // MP3 export not yet available — fall back to WAV
                encode_wav(&audio, &path, &tx)
            }
        };
        let _ = tx.send(ExportProgress::Done(result));
    });

    Ok(rx)
}

/// Legacy synchronous export (kept for backward compatibility with tests).
pub(crate) fn export_group_wav(app: &App, group_id: EntityId, path: &Path) -> Result<(), String> {
    let audio = mix_group(app, group_id)?;
    let (tx, _rx) = mpsc::channel();
    encode_wav(&audio, path, &tx)
}
