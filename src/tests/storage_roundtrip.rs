use crate::automation::AutomationData;
use crate::storage::{f32_slice_to_u8, u8_slice_to_f32};

#[test]
fn test_f32_u8_roundtrip() {
    let original: Vec<f32> = vec![0.0, 1.0, -1.0, 0.5, std::f32::consts::PI];
    let bytes = f32_slice_to_u8(&original);
    let restored = u8_slice_to_f32(&bytes);
    assert_eq!(original, restored);
}

#[test]
fn test_u8_to_f32_odd_bytes_returns_empty() {
    let odd_bytes = vec![1u8, 2, 3, 4, 5]; // 5 bytes, not multiple of 4
    let result = u8_slice_to_f32(&odd_bytes);
    assert!(result.is_empty());
}

#[test]
fn test_automation_points_survive_save_load() {
    // Simulate the save path: extract pairs from AutomationData
    let mut data = AutomationData::new();
    data.volume_lane_mut().insert_point(0.1, 0.3);
    data.volume_lane_mut().insert_point(0.5, 0.8);
    data.pan_lane_mut().insert_point(0.2, 0.7);

    // Extract stored format (what gets serialized)
    let vol_stored: Vec<[f32; 2]> = data
        .volume_lane()
        .points
        .iter()
        .map(|p| [p.t, p.value])
        .collect();
    let pan_stored: Vec<[f32; 2]> = data
        .pan_lane()
        .points
        .iter()
        .map(|p| [p.t, p.value])
        .collect();

    // Reconstruct (what happens on load)
    let restored = AutomationData::from_stored(&vol_stored, &pan_stored);

    assert_eq!(restored.volume_lane().points.len(), 2);
    assert_eq!(restored.pan_lane().points.len(), 1);

    // Verify values match at stored points
    for (orig, rest) in data
        .volume_lane()
        .points
        .iter()
        .zip(restored.volume_lane().points.iter())
    {
        assert!((orig.t - rest.t).abs() < 1e-6);
        assert!((orig.value - rest.value).abs() < 1e-6);
    }
}

#[test]
fn test_f32_u8_roundtrip_with_single_value() {
    let single = vec![42.0f32];
    let bytes = f32_slice_to_u8(&single);
    assert_eq!(bytes.len(), 4);
    let restored = u8_slice_to_f32(&bytes);
    assert_eq!(restored, single);
}
