use crate::automation::{
    interp_automation, volume_value_to_gain, AutomationData, AutomationLane, AutomationParam,
};

#[test]
fn test_value_at_empty_returns_default() {
    let lane = AutomationLane::new(AutomationParam::Volume);
    assert_eq!(lane.value_at(0.5), 0.5);
}

#[test]
fn test_value_at_exact_point() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    lane.insert_point(0.3, 0.7);
    assert_eq!(lane.value_at(0.3), 0.7);
}

#[test]
fn test_value_at_interpolates_between_points() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    lane.insert_point(0.0, 0.0);
    lane.insert_point(1.0, 1.0);
    let v = lane.value_at(0.5);
    assert!((v - 0.5).abs() < 1e-6, "expected ~0.5, got {v}");
}

#[test]
fn test_value_at_before_first_point() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    lane.insert_point(0.5, 0.8);
    assert_eq!(lane.value_at(0.0), 0.8);
}

#[test]
fn test_value_at_after_last_point() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    lane.insert_point(0.2, 0.3);
    assert_eq!(lane.value_at(1.0), 0.3);
}

#[test]
fn test_insert_point_maintains_sorted_order() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    lane.insert_point(0.8, 0.5);
    lane.insert_point(0.2, 0.5);
    lane.insert_point(0.5, 0.5);
    let ts: Vec<f32> = lane.points.iter().map(|p| p.t).collect();
    assert_eq!(ts, vec![0.2, 0.5, 0.8]);
}

#[test]
fn test_insert_point_clamps_to_0_1() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    lane.insert_point(1.5, 2.0);
    assert_eq!(lane.points[0].t, 1.0);
    assert_eq!(lane.points[0].value, 1.0);

    lane.insert_point(-0.5, -1.0);
    assert_eq!(lane.points[0].t, 0.0);
    assert_eq!(lane.points[0].value, 0.0);
}

#[test]
fn test_remove_point_bounds_check() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    // Should not panic on empty lane
    lane.remove_point(0);
    lane.remove_point(100);
    assert!(lane.points.is_empty());
}

#[test]
fn test_volume_value_to_gain_key_values() {
    let g0 = volume_value_to_gain(0.0);
    assert!((g0).abs() < 1e-6, "gain at 0.0 should be 0.0, got {g0}");

    let g05 = volume_value_to_gain(0.5);
    assert!((g05 - 1.0).abs() < 1e-6, "gain at 0.5 should be 1.0, got {g05}");

    let g1 = volume_value_to_gain(1.0);
    assert!((g1 - 4.0).abs() < 1e-6, "gain at 1.0 should be 4.0, got {g1}");
}

#[test]
fn test_interp_automation_matches_lane_value_at() {
    let mut lane = AutomationLane::new(AutomationParam::Volume);
    lane.insert_point(0.0, 0.2);
    lane.insert_point(0.5, 0.8);
    lane.insert_point(1.0, 0.4);

    let pairs: Vec<(f32, f32)> = lane.points.iter().map(|p| (p.t, p.value)).collect();

    for t_int in 0..=10 {
        let t = t_int as f32 / 10.0;
        let lane_val = lane.value_at(t);
        let interp_val = interp_automation(t, &pairs, 0.5);
        assert!(
            (lane_val - interp_val).abs() < 1e-6,
            "mismatch at t={t}: lane={lane_val}, interp={interp_val}"
        );
    }
}

#[test]
fn test_from_stored_roundtrip() {
    let mut data = AutomationData::new();
    data.volume_lane_mut().insert_point(0.1, 0.3);
    data.volume_lane_mut().insert_point(0.7, 0.9);
    data.pan_lane_mut().insert_point(0.4, 0.6);

    let vol_pts: Vec<[f32; 2]> = data
        .volume_lane()
        .points
        .iter()
        .map(|p| [p.t, p.value])
        .collect();
    let pan_pts: Vec<[f32; 2]> = data
        .pan_lane()
        .points
        .iter()
        .map(|p| [p.t, p.value])
        .collect();

    let restored = AutomationData::from_stored(&vol_pts, &pan_pts);

    assert_eq!(restored.volume_lane().points.len(), 2);
    assert_eq!(restored.pan_lane().points.len(), 1);
    assert!((restored.volume_lane().value_at(0.1) - 0.3).abs() < 1e-6);
    assert!((restored.volume_lane().value_at(0.7) - 0.9).abs() < 1e-6);
    assert!((restored.pan_lane().value_at(0.4) - 0.6).abs() < 1e-6);
}
