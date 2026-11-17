//! Clipboard access is read-only and returns only validated, durably stored raster originals.
use crate::storage::StoreError;
#[cfg(test)]
use crate::{queue::Storage, storage::Attachment};
use image::ImageEncoder;
#[cfg(test)]
use tauri::State;

/// Preserve platform errors without treating an inaccessible clipboard as an empty one.
fn failed(message: impl ToString) -> StoreError {
    StoreError {
        code: "validation",
        message: message.to_string(),
    }
}

/// Check native pixel dimensions and exact RGBA length before encoding a lossless PNG.
fn encode(width: usize, height: usize, bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    let pixels = width
        .checked_mul(height)
        .filter(|n| *n > 0 && *n <= 40_000_000)
        .ok_or_else(|| failed("Clipboard image exceeds 40 megapixels or has invalid dimensions"))?;
    if pixels.checked_mul(4) != Some(bytes.len()) {
        return Err(failed("Invalid clipboard RGBA pixels"));
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            bytes,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(failed)?;
    if png.len() > 20 * 1024 * 1024 {
        return Err(failed("Clipboard PNG exceeds 20 MiB"));
    }
    Ok(png)
}

/// RustDesk advertises encoded PNG as `image/png`, which arboard's Windows reader does not check.
/// Keep the clipboard locked between the size check and read; never accept arbitrary custom formats.
#[cfg(windows)]
fn mime_png() -> Result<Option<Vec<u8>>, StoreError> {
    let _guard = clipboard_win::Clipboard::new_attempts(10).map_err(failed)?;
    let format = clipboard_win::register_format("image/png")
        .ok_or_else(|| failed("Cannot register clipboard PNG format"))?
        .get();
    if !clipboard_win::is_format_avail(format) {
        return Ok(None);
    }
    let size = clipboard_win::raw::size(format)
        .ok_or_else(|| failed("Cannot read clipboard PNG size"))?
        .get();
    if size > 20 * 1024 * 1024 {
        return Err(failed("Clipboard PNG exceeds 20 MiB"));
    }
    let mut bytes = Vec::new();
    clipboard_win::raw::get_vec(format, &mut bytes).map_err(failed)?;
    // Store::put_attachment performs bounded decoding and derives dimensions before any insertion.
    Ok(Some(bytes))
}

/// Prefer lossless encoded MIME data when present; preserve the existing platform screenshot path otherwise.
fn read_image() -> Result<Option<Vec<u8>>, StoreError> {
    #[cfg(windows)]
    if let Some(bytes) = mime_png()? {
        return Ok(Some(bytes));
    }
    let mut clipboard = arboard::Clipboard::new().map_err(failed)?;
    match clipboard.get_image() {
        Ok(image) => encode(image.width, image.height, &image.bytes).map(Some),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(error) => Err(failed(error)),
    }
}

/// Read screenshots through native Windows/macOS formats off the UI thread; no image permits text fallback.
#[cfg(test)]
#[allow(dead_code)]
#[tauri::command]
pub async fn paste_image(state: State<'_, Storage>) -> Result<Option<Attachment>, StoreError> {
    let bytes = tauri::async_runtime::spawn_blocking(read_image)
        .await
        .map_err(failed)??;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    state
        .submit(move |store| {
            let info = store.put_attachment(bytes.clone())?;
            Ok(Some(Attachment { info, bytes }))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Read the user's current clipboard without changing it; validate through an isolated temporary store.
    #[test]
    #[ignore = "requires a real screenshot on the native clipboard"]
    fn live_clipboard_original() {
        let bytes = read_image().unwrap().expect("No native clipboard image");
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::storage::Store::open(directory.path()).unwrap();
        let original = store.put_attachment(bytes).unwrap();
        assert!(original.width > 0 && original.height > 0);
        println!(
            "Validated native clipboard original: {} x {}",
            original.width, original.height
        );
    }

    /// Rejections happen before encoding; valid dimensions are pixel dimensions, independent of display scale.
    #[test]
    fn native_pixel_validation() {
        assert!(encode(0, 1, &[]).is_err());
        assert!(encode(usize::MAX, 2, &[]).is_err());
        assert!(encode(40_000_001, 1, &[]).is_err());
        assert!(encode(1, 1, &[0; 3]).is_err());
        let png = encode(2, 1, &[255; 8]).unwrap();
        let image = image::load_from_memory(&png).unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));
    }
}

/// File editor owns asset persistence; clipboard pixels never enter SQLite.
#[tauri::command]
pub async fn file_clipboard() -> Result<Option<Vec<u8>>, StoreError> {
    let bytes = tauri::async_runtime::spawn_blocking(read_image)
        .await
        .map_err(failed)??;
    if let Some(bytes) = &bytes {
        crate::files::validate_image(bytes)?;
    }
    Ok(bytes)
}
