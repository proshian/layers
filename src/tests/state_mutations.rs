use std::sync::Arc;

use crate::audio::AudioClipData;
use crate::automation::AutomationData;
use crate::component;
use crate::ui::waveform::{AudioData, WaveformPeaks, WaveformView};
use crate::{App, HitTarget};

fn make_waveform(x: f32, y: f32) -> WaveformView {
    WaveformView {
        audio: Arc::new(AudioData {
            left_samples: Arc::new(Vec::new()),
            right_samples: Arc::new(Vec::new()),
            left_peaks: Arc::new(WaveformPeaks::empty()),
            right_peaks: Arc::new(WaveformPeaks::empty()),
            sample_rate: 48000,
            filename: "test.wav".to_string(),
        }),
        position: [x, y],
        size: [200.0, 80.0],
        color: [0.0, 1.0, 0.0, 1.0],
        border_radius: 4.0,
        fade_in_px: 0.0,
        fade_out_px: 0.0,
        fade_in_curve: 0.5,
        fade_out_curve: 0.5,
        volume: 1.0,
        disabled: false,
        sample_offset_px: 0.0,
        automation: AutomationData::new(),
    }
}

fn make_waveform_with_samples(x: f32, y: f32, num_samples: usize) -> WaveformView {
    let samples: Vec<f32> = (0..num_samples).map(|i| i as f32 / num_samples as f32).collect();
    WaveformView {
        audio: Arc::new(AudioData {
            left_samples: Arc::new(samples.clone()),
            right_samples: Arc::new(samples),
            left_peaks: Arc::new(WaveformPeaks::empty()),
            right_peaks: Arc::new(WaveformPeaks::empty()),
            sample_rate: 48000,
            filename: "test.wav".to_string(),
        }),
        position: [x, y],
        size: [200.0, 80.0],
        color: [0.0, 1.0, 0.0, 1.0],
        border_radius: 4.0,
        fade_in_px: 0.0,
        fade_out_px: 0.0,
        fade_in_curve: 0.5,
        fade_out_curve: 0.5,
        volume: 1.0,
        disabled: false,
        sample_offset_px: 0.0,
        automation: AutomationData::new(),
    }
}

fn make_audio_clip() -> AudioClipData {
    AudioClipData {
        samples: Arc::new(Vec::new()),
        sample_rate: 48000,
        duration_secs: 0.0,
    }
}

fn make_audio_clip_with_samples(num_samples: usize) -> AudioClipData {
    let samples: Vec<f32> = (0..num_samples).map(|i| i as f32 / num_samples as f32).collect();
    AudioClipData {
        samples: Arc::new(samples),
        sample_rate: 48000,
        duration_secs: num_samples as f32 / 48000.0,
    }
}

// ---- Delete tests ----

#[test]
fn test_delete_waveform_removes_audio_clip() {
    let mut app = App::new_headless();
    app.waveforms.push(make_waveform(0.0, 0.0));
    app.audio_clips.push(make_audio_clip());
    app.waveforms.push(make_waveform(300.0, 0.0));
    app.audio_clips.push(make_audio_clip());

    app.selected.push(HitTarget::Waveform(0));
    app.delete_selected();

    assert_eq!(app.waveforms.len(), 1);
    assert_eq!(app.audio_clips.len(), 1);
}

#[test]
fn test_delete_component_removes_owned_waveforms() {
    let mut app = App::new_headless();
    // Add 3 waveforms: wf0, wf1, wf2
    for i in 0..3 {
        app.waveforms.push(make_waveform(i as f32 * 300.0, 0.0));
        app.audio_clips.push(make_audio_clip());
    }
    // Component owns wf0 and wf1
    app.components.push(component::ComponentDef {
        id: 1,
        name: "C1".to_string(),
        position: [0.0, 0.0],
        size: [500.0, 100.0],
        waveform_indices: vec![0, 1],
    });

    app.selected.push(HitTarget::ComponentDef(0));
    app.delete_selected();

    // Component and its 2 waveforms should be removed, leaving wf2
    assert_eq!(app.components.len(), 0);
    assert_eq!(app.waveforms.len(), 1);
    assert_eq!(app.audio_clips.len(), 1);
}

#[test]
fn test_delete_component_fixes_other_component_indices() {
    let mut app = App::new_headless();
    // Add 4 waveforms: wf0, wf1, wf2, wf3
    for i in 0..4 {
        app.waveforms.push(make_waveform(i as f32 * 300.0, 0.0));
        app.audio_clips.push(make_audio_clip());
    }
    // Component A owns wf0, wf1
    app.components.push(component::ComponentDef {
        id: 1,
        name: "A".to_string(),
        position: [0.0, 0.0],
        size: [500.0, 100.0],
        waveform_indices: vec![0, 1],
    });
    // Component B owns wf2, wf3
    app.components.push(component::ComponentDef {
        id: 2,
        name: "B".to_string(),
        position: [0.0, 200.0],
        size: [500.0, 100.0],
        waveform_indices: vec![2, 3],
    });

    // Delete component A (index 0)
    app.selected.push(HitTarget::ComponentDef(0));
    app.delete_selected();

    // Component B's waveform indices should be decremented by 2
    // (two waveforms removed before them)
    assert_eq!(app.components.len(), 1);
    assert_eq!(app.components[0].name, "B");
    assert_eq!(app.components[0].waveform_indices, vec![0, 1]);
}

// ---- Split tests ----

// split_sample_at_cursor requires hit testing via mouse_pos + camera.
// We test it by setting up mouse_pos to land on a waveform.

#[test]
fn test_split_waveform_inserts_both_halves() {
    let mut app = App::new_headless();
    let num_samples = 48000; // 1 second of audio
    app.waveforms
        .push(make_waveform_with_samples(0.0, 0.0, num_samples));
    app.audio_clips
        .push(make_audio_clip_with_samples(num_samples));

    let wf_len_before = app.waveforms.len();
    let clip_len_before = app.audio_clips.len();

    // Position mouse in the middle of the waveform
    // Camera is at default position (-100, -50) with zoom 1.0
    // screen_to_world(screen) = screen / zoom + position = screen + (-100, -50)
    // We need world pos inside waveform at [0, 0] with size [200, 80]
    // world = [100, 40] -> screen = [100 - (-100), 40 - (-50)] = [200, 90]
    app.mouse_pos = [200.0, 90.0];

    app.split_sample_at_cursor();

    assert_eq!(
        app.waveforms.len(),
        wf_len_before + 1,
        "split should add one waveform"
    );
    assert_eq!(
        app.audio_clips.len(),
        clip_len_before + 1,
        "split should add one audio clip"
    );
}

#[test]
fn test_split_waveform_fixes_component_indices() {
    let mut app = App::new_headless();
    let num_samples = 48000;

    // wf0: the one we'll split
    app.waveforms
        .push(make_waveform_with_samples(0.0, 0.0, num_samples));
    app.audio_clips
        .push(make_audio_clip_with_samples(num_samples));

    // wf1: belongs to a component
    app.waveforms.push(make_waveform(0.0, 200.0));
    app.audio_clips.push(make_audio_clip());

    app.components.push(component::ComponentDef {
        id: 1,
        name: "C".to_string(),
        position: [0.0, 200.0],
        size: [200.0, 80.0],
        waveform_indices: vec![1],
    });

    // Split wf0
    app.mouse_pos = [200.0, 90.0];
    app.split_sample_at_cursor();

    // After split, wf0 -> left half at index 0, right half at index 1
    // Old wf1 is now at index 2
    // Component should have its index updated from 1 to 2
    assert_eq!(app.components[0].waveform_indices, vec![2]);
}

#[test]
fn test_split_waveform_fixes_selected_indices() {
    let mut app = App::new_headless();
    let num_samples = 48000;

    // wf0: the one we'll split
    app.waveforms
        .push(make_waveform_with_samples(0.0, 0.0, num_samples));
    app.audio_clips
        .push(make_audio_clip_with_samples(num_samples));

    // wf1: another waveform after
    app.waveforms.push(make_waveform(0.0, 200.0));
    app.audio_clips.push(make_audio_clip());

    // Select wf1
    app.selected.push(HitTarget::Waveform(1));

    // Split wf0
    app.mouse_pos = [200.0, 90.0];
    app.split_sample_at_cursor();

    // wf1 was at index 1, after split of wf0 it should be at index 2
    assert!(
        app.selected.contains(&HitTarget::Waveform(2)),
        "selected waveform after split point should be incremented"
    );
}

// ---- Duplicate tests ----

#[test]
fn test_duplicate_waveform_syncs_audio_clips() {
    let mut app = App::new_headless();
    app.waveforms.push(make_waveform(0.0, 0.0));
    app.audio_clips.push(make_audio_clip());

    app.selected.push(HitTarget::Waveform(0));
    app.duplicate_selected();

    assert_eq!(app.waveforms.len(), 2);
    assert_eq!(app.audio_clips.len(), 2);
}

#[test]
fn test_duplicate_component_creates_new_waveforms() {
    let mut app = App::new_headless();
    app.next_component_id = 2;

    // wf0 and wf1 owned by component
    app.waveforms.push(make_waveform(0.0, 0.0));
    app.audio_clips.push(make_audio_clip());
    app.waveforms.push(make_waveform(0.0, 100.0));
    app.audio_clips.push(make_audio_clip());

    app.components.push(component::ComponentDef {
        id: 1,
        name: "C".to_string(),
        position: [0.0, 0.0],
        size: [200.0, 200.0],
        waveform_indices: vec![0, 1],
    });

    app.selected.push(HitTarget::ComponentDef(0));
    app.duplicate_selected();

    // Should have 2 components, 4 waveforms, 4 audio clips
    assert_eq!(app.components.len(), 2);
    assert_eq!(app.waveforms.len(), 4);
    assert_eq!(app.audio_clips.len(), 4);

    // New component's waveform indices should point to the new waveforms
    let new_comp = &app.components[1];
    assert_eq!(new_comp.waveform_indices.len(), 2);
    // Should not overlap with original indices
    for &wi in &new_comp.waveform_indices {
        assert!(
            !app.components[0].waveform_indices.contains(&wi),
            "new component's waveform indices should not overlap original"
        );
    }
}

// ---- Copy/Paste tests ----

#[test]
fn test_paste_waveform_adds_audio_clip() {
    let mut app = App::new_headless();
    app.waveforms.push(make_waveform(0.0, 0.0));
    app.audio_clips.push(make_audio_clip());

    app.selected.push(HitTarget::Waveform(0));
    app.copy_selected();

    let wf_before = app.waveforms.len();
    let clip_before = app.audio_clips.len();

    app.paste_clipboard();

    assert_eq!(app.waveforms.len(), wf_before + 1);
    assert_eq!(app.audio_clips.len(), clip_before + 1);
}

#[test]
fn test_paste_component_creates_independent_copy() {
    let mut app = App::new_headless();
    app.next_component_id = 2;

    app.waveforms.push(make_waveform(0.0, 0.0));
    app.audio_clips.push(make_audio_clip());
    app.components.push(component::ComponentDef {
        id: 1,
        name: "C".to_string(),
        position: [0.0, 0.0],
        size: [200.0, 80.0],
        waveform_indices: vec![0],
    });

    app.selected.push(HitTarget::ComponentDef(0));
    app.copy_selected();
    app.paste_clipboard();

    assert_eq!(app.components.len(), 2);
    // The pasted component should have a different id
    assert_ne!(app.components[0].id, app.components[1].id);
    // And different waveform indices
    let orig_indices = &app.components[0].waveform_indices;
    let new_indices = &app.components[1].waveform_indices;
    for wi in new_indices {
        assert!(
            !orig_indices.contains(wi),
            "pasted component should not share waveform indices"
        );
    }
}
