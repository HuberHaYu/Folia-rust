// Grain frame index. The shader hashes this integer instead of a growing playback time.

/// `time_seconds * 48`, wrapped into `0..4096`, so the hash input stays a small integer.
pub fn grain_frame(time_seconds: f32) -> u32 {
    if !time_seconds.is_finite() || time_seconds <= 0.0 {
        return 0;
    }
    (time_seconds * 48.0).rem_euclid(4096.0) as u32
}
