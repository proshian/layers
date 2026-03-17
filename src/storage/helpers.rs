pub fn f32_slice_to_u8(data: &[f32]) -> Vec<u8> {
    bytemuck::cast_slice(data).to_vec()
}

pub fn u8_slice_to_f32(data: &[u8]) -> Vec<f32> {
    if data.len() % 4 != 0 {
        return Vec::new();
    }
    data.chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
