//! Cross-platform screenshot capture.

use image::RgbaImage;
use screenshots::Screen;

#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
    #[error("no display available (headless?)")]
    NoDisplay,
    #[error("screenshot capture failed: {0}")]
    Capture(String),
}

pub struct Capturer {
    _phantom: std::marker::PhantomData<()>,
}

impl Capturer {
    pub fn new() -> Result<Self, ScreenshotError> {
        let probe = Screen::all();
        match probe {
            Ok(v) if !v.is_empty() => Ok(Self {
                _phantom: std::marker::PhantomData,
            }),
            _ => Err(ScreenshotError::NoDisplay),
        }
    }

    /// Capture the primary display as an RGBA image.
    pub fn grab_primary(&self) -> Result<RgbaImage, ScreenshotError> {
        let screens = Screen::all().map_err(|e| ScreenshotError::Capture(e.to_string()))?;
        let primary = screens.into_iter().next().ok_or(ScreenshotError::NoDisplay)?;
        primary.capture().map_err(|e| ScreenshotError::Capture(e.to_string()))
    }
}
