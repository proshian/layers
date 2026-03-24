use std::sync::{Arc, Mutex};
use crate::App;
use crate::entity_id::new_id;
use crate::effects::{EffectChain, EffectChainSlot};
use crate::ui::waveform::{AudioData, WaveformPeaks, WaveformView, WarpMode};
use crate::automation::AutomationData;
use crate::ui::waveform::AudioClipData;
use std::path::PathBuf;

fn make_waveform(x: f32, y: f32) -> WaveformView {
    WaveformView {
        audio: Arc::new(AudioData {
            left_samples: Arc::new(Vec::new()),
            right_samples: Arc::new(Vec::new()),
            left_peaks: Arc::new(WaveformPeaks::empty()),
            right_peaks: Arc::new(WaveformPeaks::empty()),
            sample_rate: 44100,
            filename: "test.wav".to_string(),
        }),
        filename: "test.wav".to_string(),
        position: [x, y],
        size: [200.0, 60.0],
        color: [0.3, 0.5, 0.9, 1.0],
        border_radius: 4.0,
        fade_in_px: 0.0,
        fade_out_px: 0.0,
        fade_in_curve: 0.0,
        fade_out_curve: 0.0,
        volume: 1.0,
        pan: 0.5,
        warp_mode: WarpMode::Off,
        sample_bpm: 120.0,
        pitch_semitones: 0.0,
        is_reversed: false,
        disabled: false,
        sample_offset_px: 0.0,
        automation: AutomationData::new(),
        effect_chain_id: None,
        take_group: None,
    }
}

/// Collect chain latency returns 0 for stubs (no real plugins loaded in headless mode).
#[test]
fn collect_chain_latency_returns_zero_for_stubs() {
    let latency = App::collect_chain_latency(&[]);
    assert_eq!(latency, 0);
}

/// Collect chain latency returns 0 for None-valued plugin handles.
#[test]
fn collect_chain_latency_returns_zero_for_none_plugins() {
    let handle: Arc<Mutex<Option<crate::effects::PluginGuiHandle>>> = Arc::new(Mutex::new(None));
    let latency = App::collect_chain_latency(&[handle]);
    assert_eq!(latency, 0);
}

/// Waveform with effect chain has chain plugins collected correctly.
#[test]
fn waveform_chain_plugins_collected_with_latency() {
    let mut app = App::new_headless();
    let wf_id = new_id();
    let chain_id = new_id();

    let mut wf = make_waveform(100.0, 100.0);
    wf.effect_chain_id = Some(chain_id);
    app.waveforms.insert(wf_id, wf);

    let clip = AudioClipData {
        samples: Arc::new(vec![0.0; 44100]),
        sample_rate: 44100,
        duration_secs: 1.0,
    };
    app.audio_clips.insert(wf_id, clip);

    let slot = EffectChainSlot::new(
        "test_uid".to_string(),
        "Test Plugin".to_string(),
        PathBuf::from("/test/plugin.vst3"),
    );
    let mut chain = EffectChain::new();
    chain.slots.push(slot);
    app.effect_chains.insert(chain_id, chain);

    let group_of = app.build_group_membership();
    let plugins = app.collect_chain_plugins(wf_id, Some(chain_id), &group_of);

    // In headless mode, plugin GUI handles are None so latency should be 0
    let latency = App::collect_chain_latency(&plugins);
    assert_eq!(latency, 0);
    // But chain plugins should still be collected (1 slot = 1 handle)
    assert_eq!(plugins.len(), 1);
}
