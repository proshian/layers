use std::sync::Arc;

use crate::audio::AudioClipData;
use crate::automation::AutomationData;
use crate::history::MAX_UNDO_HISTORY;
use crate::regions::LoopRegion;
use crate::ui::waveform::{AudioData, WaveformPeaks};
use crate::{App, CanvasObject, HitTarget, WaveformView};

fn make_object(x: f32, y: f32) -> CanvasObject {
    CanvasObject {
        position: [x, y],
        size: [100.0, 60.0],
        color: [1.0, 0.0, 0.0, 1.0],
        border_radius: 0.0,
    }
}

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

#[test]
fn test_undo_restores_objects() {
    let mut app = App::new_headless();
    app.objects.push(make_object(0.0, 0.0));
    app.push_undo();

    app.objects.push(make_object(100.0, 100.0));
    app.objects.push(make_object(200.0, 200.0));
    assert_eq!(app.objects.len(), 3);

    app.undo();
    assert_eq!(app.objects.len(), 1);
}

#[test]
fn test_redo_after_undo() {
    let mut app = App::new_headless();
    app.push_undo();

    app.objects.push(make_object(10.0, 20.0));
    let post_mutation = app.objects.clone();

    app.undo();
    assert!(app.objects.is_empty());

    app.redo();
    assert_eq!(app.objects, post_mutation);
}

#[test]
fn test_undo_empty_stack_noop() {
    let mut app = App::new_headless();
    let obj_count = app.objects.len();
    app.undo();
    assert_eq!(app.objects.len(), obj_count);
}

#[test]
fn test_redo_empty_stack_noop() {
    let mut app = App::new_headless();
    app.redo();
    assert!(app.objects.is_empty());
}

#[test]
fn test_push_clears_redo() {
    let mut app = App::new_headless();
    app.push_undo();
    app.objects.push(make_object(0.0, 0.0));
    app.undo();
    assert!(!app.redo_stack.is_empty());

    // A new push_undo should clear the redo stack
    app.push_undo();
    assert!(app.redo_stack.is_empty());
}

#[test]
fn test_max_history_limit() {
    let mut app = App::new_headless();
    for i in 0..55 {
        app.push_undo();
        app.objects.push(make_object(i as f32, 0.0));
    }
    assert!(app.undo_stack.len() <= MAX_UNDO_HISTORY);
}

#[test]
fn test_multiple_undo_redo_cycle() {
    let mut app = App::new_headless();

    // State 0: empty
    app.push_undo();
    app.objects.push(make_object(0.0, 0.0));
    let state1 = app.objects.clone();

    app.push_undo();
    app.objects.push(make_object(1.0, 1.0));
    let state2 = app.objects.clone();

    app.push_undo();
    app.objects.push(make_object(2.0, 2.0));
    let state3 = app.objects.clone();

    assert_eq!(app.objects.len(), 3);

    // Undo back through all 3 states
    app.undo();
    assert_eq!(app.objects, state2);

    app.undo();
    assert_eq!(app.objects, state1);

    app.undo();
    assert!(app.objects.is_empty());

    // Redo forward through all 3 states
    app.redo();
    assert_eq!(app.objects, state1);

    app.redo();
    assert_eq!(app.objects, state2);

    app.redo();
    assert_eq!(app.objects, state3);
}

#[test]
fn test_undo_clears_selection() {
    let mut app = App::new_headless();
    app.objects.push(make_object(0.0, 0.0));
    app.push_undo();

    app.selected.push(HitTarget::Object(0));
    assert!(!app.selected.is_empty());

    app.undo();
    assert!(app.selected.is_empty());
}

#[test]
fn test_undo_restores_waveform_positions() {
    let mut app = App::new_headless();
    app.waveforms.push(make_waveform(50.0, 50.0));
    app.audio_clips.push(AudioClipData {
        samples: Arc::new(Vec::new()),
        sample_rate: 48000,
        duration_secs: 0.0,
    });
    app.push_undo();

    // Move waveform
    app.waveforms[0].position = [300.0, 400.0];

    app.undo();
    assert_eq!(app.waveforms[0].position, [50.0, 50.0]);
}

#[test]
fn test_undo_restores_loop_regions() {
    let mut app = App::new_headless();
    app.push_undo();

    app.loop_regions.push(LoopRegion {
        position: [100.0, 0.0],
        size: [200.0, 30.0],
        enabled: true,
    });
    assert_eq!(app.loop_regions.len(), 1);

    app.undo();
    assert!(app.loop_regions.is_empty());
}
