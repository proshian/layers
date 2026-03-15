use std::sync::Arc;

use crate::automation::AutomationData;
use crate::component;
use crate::effects;
use crate::hit_testing::{canonical_rect, hit_test, point_in_rect, rects_overlap, targets_in_rect};
use crate::regions::{ExportRegion, LoopRegion};
use crate::ui::waveform::{AudioData, WaveformPeaks, WaveformView};
use crate::{Camera, CanvasObject, HitTarget};

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

// ---- point_in_rect ----

#[test]
fn test_point_in_rect_inside() {
    assert!(point_in_rect([50.0, 30.0], [0.0, 0.0], [100.0, 60.0]));
}

#[test]
fn test_point_in_rect_outside() {
    assert!(!point_in_rect([150.0, 30.0], [0.0, 0.0], [100.0, 60.0]));
}

#[test]
fn test_point_in_rect_on_boundary() {
    // Boundaries are inclusive (<=)
    assert!(point_in_rect([0.0, 0.0], [0.0, 0.0], [100.0, 60.0]));
    assert!(point_in_rect([100.0, 60.0], [0.0, 0.0], [100.0, 60.0]));
}

// ---- rects_overlap ----

#[test]
fn test_rects_overlap_overlapping() {
    assert!(rects_overlap(
        [0.0, 0.0],
        [100.0, 100.0],
        [50.0, 50.0],
        [100.0, 100.0]
    ));
}

#[test]
fn test_rects_overlap_adjacent() {
    // Touching but not overlapping — strict < means adjacent = false
    assert!(!rects_overlap(
        [0.0, 0.0],
        [100.0, 100.0],
        [100.0, 0.0],
        [100.0, 100.0]
    ));
}

#[test]
fn test_rects_overlap_contained() {
    assert!(rects_overlap(
        [0.0, 0.0],
        [200.0, 200.0],
        [50.0, 50.0],
        [10.0, 10.0]
    ));
}

// ---- canonical_rect ----

#[test]
fn test_canonical_rect_positive_direction() {
    let (pos, size) = canonical_rect([10.0, 20.0], [110.0, 120.0]);
    assert_eq!(pos, [10.0, 20.0]);
    assert_eq!(size, [100.0, 100.0]);
}

#[test]
fn test_canonical_rect_negative_direction() {
    let (pos, size) = canonical_rect([110.0, 120.0], [10.0, 20.0]);
    assert_eq!(pos, [10.0, 20.0]);
    assert_eq!(size, [100.0, 100.0]);
}

// ---- hit_test priority ----

#[test]
fn test_hit_test_priority_order() {
    // Instance > Waveform > Object > ComponentDef
    let objects = vec![make_object(0.0, 0.0)];
    let waveforms = vec![make_waveform(0.0, 0.0)];
    let comp = component::ComponentDef {
        id: 1,
        name: "C".to_string(),
        position: [0.0, 0.0],
        size: [200.0, 100.0],
        waveform_indices: vec![], // waveform NOT owned by component
    };
    let inst = component::ComponentInstance {
        component_id: 1,
        position: [0.0, 0.0],
    };
    let camera = Camera::new();

    // With instance present, should hit instance first
    let result = hit_test(
        &objects,
        &waveforms,
        &[],
        &[],
        &[],
        &[],
        &[comp.clone()],
        &[inst],
        &[],
        &[],
        None,
        [50.0, 30.0],
        &camera,
    );
    assert_eq!(result, Some(HitTarget::ComponentInstance(0)));

    // Without instance, waveform (not owned by component) wins over object
    let result = hit_test(
        &objects,
        &waveforms,
        &[],
        &[],
        &[],
        &[],
        &[comp.clone()],
        &[],
        &[],
        &[],
        None,
        [50.0, 30.0],
        &camera,
    );
    assert_eq!(result, Some(HitTarget::Waveform(0)));

    // Without waveform, object wins over component def
    let result = hit_test(
        &objects,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[comp],
        &[],
        &[],
        &[],
        None,
        [50.0, 30.0],
        &camera,
    );
    assert_eq!(result, Some(HitTarget::Object(0)));
}

// ---- targets_in_rect skips component waveforms ----

#[test]
fn test_targets_in_rect_skips_component_waveforms() {
    let waveforms = vec![make_waveform(0.0, 0.0), make_waveform(300.0, 0.0)];
    let comp = component::ComponentDef {
        id: 1,
        name: "C".to_string(),
        position: [0.0, 0.0],
        size: [200.0, 80.0],
        waveform_indices: vec![0], // waveform 0 is owned
    };

    let targets = targets_in_rect(
        &[],
        &waveforms,
        &[],
        &[],
        &[],
        &[],
        &[comp],
        &[],
        &[],
        &[],
        None,
        [0.0, 0.0],
        [600.0, 200.0],
    );

    // Waveform 0 should be skipped (owned by component), waveform 1 should be included
    let has_wf0 = targets.contains(&HitTarget::Waveform(0));
    let has_wf1 = targets.contains(&HitTarget::Waveform(1));
    assert!(!has_wf0, "waveform owned by component should be skipped");
    assert!(has_wf1, "free waveform should be selected");
}
