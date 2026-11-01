// The platforms' app icons for their marks, decoded once; a platform without one keeps its letter on its colour.

use std::sync::LazyLock;

use iced::widget::image::Handle;

const SIDE: u32 = 128;

const ICONS: [(&str, &[u8]); 5] = [
    ("netease", include_bytes!("../../../assets/platforms/netease.png")),
    ("kugou", include_bytes!("../../../assets/platforms/kugou.png")),
    ("qq", include_bytes!("../../../assets/platforms/qq.png")),
    ("bodian", include_bytes!("../../../assets/platforms/bodian.png")),
    ("qishui", include_bytes!("../../../assets/platforms/qishui.png")),
];

static DECODED: LazyLock<Vec<(&'static str, Handle)>> = LazyLock::new(|| {
    ICONS
        .iter()
        .filter_map(|&(id, bytes)| match folia_library::cover_rgba(bytes, SIDE, SIDE) {
            Ok(cover) => Some((id, Handle::from_rgba(cover.width, cover.height, cover.rgba))),
            Err(err) => {
                tracing::warn!(id, error = &err as &dyn std::error::Error, "cannot decode platform icon");
                None
            }
        })
        .collect()
});

/// The app icon drawn in a platform's mark, or `None` for the letter.
pub fn platform_icon(id: &str) -> Option<Handle> {
    DECODED.iter().find(|(icon, _)| *icon == id).map(|(_, handle)| handle.clone())
}
