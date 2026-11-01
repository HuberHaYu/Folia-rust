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

    /// Reads the modules back from a platform's PNG of the code: the dark area is cropped, the module size taken
    /// from the top-left finder pattern (seven modules wide), and each module sampled at its centre.
    pub(crate) fn from_png(key: String, png: &[u8]) -> Result<Self, OmniError> {
        let picture = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .map_err(|err| OmniError::QrImage(err.to_string()))?
            .to_luma8();
        let (w, h) = picture.dimensions();
        let dark = |x: u32, y: u32| picture.get_pixel(x, y).0[0] < 128;
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for y in 0..h {
            for x in 0..w {
                if dark(x, y) {
                    let (x0, y0, x1, y1) = bounds.unwrap_or((x, y, x, y));
                    bounds = Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)));
                }
            }
        }
        let (x0, y0, x1, y1) = bounds.ok_or_else(|| OmniError::QrImage("no dark modules".to_owned()))?;
        let run = (x0..=x1).take_while(|x| dark(*x, y0)).count() as f32;
        let module = run / 7.0;
        let side = ((x1 - x0 + 1).max(y1 - y0 + 1)) as f32;
        let width = (side / module).round() as usize;
        if module < 1.0 || !(21..=177).contains(&width) {
            return Err(OmniError::QrImage(format!("{width} modules of {module}px")));
        }
        let mut modules = Vec::with_capacity(width * width);
        for row in 0..width {
            for column in 0..width {
                let x = x0 as f32 + (column as f32 + 0.5) * module;
                let y = y0 as f32 + (row as f32 + 0.5) * module;
                modules.push(x < w as f32 && y < h as f32 && dark(x as u32, y as u32));
            }
        }
        Ok(Self { key, width, modules })
    }

    pub fn dark(&self, x: usize, y: usize) -> bool {
        x < self.width && self.modules.get(y * self.width + x).copied().unwrap_or(false)
    }
}
