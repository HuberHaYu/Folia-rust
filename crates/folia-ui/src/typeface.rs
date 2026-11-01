// The one weight mapping for interface text: weights common system faces lack resolve like CSS to ones they have.

use iced::Font;
use iced::font::Weight;

/// Segoe UI and Arial have no 500 or 800 face and keep 900 in a separate family; iced then falls back to
/// another family (a serif on Windows). CSS picks 400 for a missing 500 and 700 for a missing 800 or 900.
pub const fn weight(wanted: Weight) -> Weight {
    match wanted {
        Weight::Medium => Weight::Normal,
        Weight::ExtraBold | Weight::Black => Weight::Bold,
        other => other,
    }
}

/// The interface face at `wanted`, mapped to a weight the system has.
pub const fn font(wanted: Weight) -> Font {
    with(Font::DEFAULT, wanted)
}

/// `base` (another family, italic) at `wanted`, mapped to a weight the system has.
pub const fn with(base: Font, wanted: Weight) -> Font {
    Font { weight: weight(wanted), ..base }
}
