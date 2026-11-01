// The sign-in code as a QR matrix the interface can paint, so it never sees the address it encodes.

use crate::OmniError;

/// A sign-in code: the key to poll with and its QR modules, row by row, `true` where dark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginQr {
    pub key: String,
    pub width: usize,
    pub modules: Vec<bool>,
}

impl LoginQr {
    pub(crate) fn encode(key: String, url: &str) -> Result<Self, OmniError> {
        let code = qrcode::QrCode::new(url.as_bytes())?;
        let modules = code.to_colors().into_iter().map(|color| color == qrcode::Color::Dark).collect();
        Ok(Self { key, width: code.width(), modules })
    }

    pub fn dark(&self, x: usize, y: usize) -> bool {
        x < self.width && self.modules.get(y * self.width + x).copied().unwrap_or(false)
    }
}
