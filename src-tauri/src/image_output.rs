//! Validated flattened PNG output. These commands never access or change the note database.
use crate::storage::StoreError;
use std::{
    borrow::Cow,
    io::{Cursor, Write},
    path::Path,
};
use tauri::ipc::{InvokeBody, Request};
use tauri_plugin_dialog::DialogExt;

const MAX_PNG: usize = 64 * 1024 * 1024;

/// Preserve an actionable operation error without exposing a generic successful output result.
fn failed(message: impl std::fmt::Display) -> StoreError {
    StoreError {
        code: "image_output",
        message: message.to_string(),
    }
}

/// Accept bounded binary IPC only, avoiding large JSON number arrays and renderer-provided filesystem paths.
fn payload(body: &InvokeBody) -> Result<Vec<u8>, StoreError> {
    match body {
        InvokeBody::Raw(bytes) if !bytes.is_empty() && bytes.len() <= MAX_PNG => Ok(bytes.clone()),
        _ => Err(failed(
            "Image output requires PNG bytes between 1 byte and 64 MiB",
        )),
    }
}

/// Fully decode PNG within the pixel/allocation budget before touching the clipboard or a destination.
fn decode(bytes: &[u8]) -> Result<image::RgbaImage, StoreError> {
    if bytes.is_empty()
        || bytes.len() > MAX_PNG
        || image::guess_format(bytes).ok() != Some(image::ImageFormat::Png)
    {
        return Err(failed("Output must be a PNG within 64 MiB"));
    }
    let (width, height) =
        image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png)
            .into_dimensions()
            .map_err(failed)?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 40_000_000 {
        return Err(failed(
            "Image output exceeds 40 megapixels or has invalid dimensions",
        ));
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode().map_err(failed)?.into_rgba8())
}

/// Use straight RGBA pixels and exact source dimensions for the installed native clipboard implementation.
fn clipboard_image(image: image::RgbaImage) -> arboard::ImageData<'static> {
    arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Owned(image.into_raw()),
    }
}

/// Write a full validated PNG atomically; failures preserve any existing destination and remove the temporary file.
fn write_png(bytes: &[u8], destination: Option<&Path>) -> Result<bool, StoreError> {
    let Some(path) = destination else {
        return Ok(false);
    };
    let parent = path
        .parent()
        .ok_or_else(|| failed("PNG destination has no parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(failed)?;
    temporary.write_all(bytes).map_err(failed)?;
    temporary.as_file().sync_all().map_err(failed)?;
    temporary.persist(path).map_err(failed)?;
    Ok(true)
}

/// Copy validated pixels off the UI thread; clipboard access failures leave all editable source intact.
#[tauri::command]
pub async fn copy_image(request: Request<'_>) -> Result<(), StoreError> {
    let bytes = payload(request.body())?;
    tauri::async_runtime::spawn_blocking(move || {
        let image = clipboard_image(decode(&bytes)?);
        arboard::Clipboard::new()
            .map_err(failed)?
            .set_image(image)
            .map_err(failed)
    })
    .await
    .map_err(failed)?
}

/// Ask the OS to choose/confirm a destination; cancellation writes nothing and is reported distinctly.
#[tauri::command]
pub async fn export_image(app: tauri::AppHandle, request: Request<'_>) -> Result<bool, StoreError> {
    let bytes = payload(request.body())?;
    tauri::async_runtime::spawn_blocking(move || {
        // Validation completes before the picker; the decoded buffer is dropped before file I/O.
        drop(decode(&bytes)?);
        let destination = app
            .dialog()
            .file()
            .add_filter("PNG image", &["png"])
            .set_file_name("annotated-image.png")
            .blocking_save_file();
        let path = destination
            .map(|path| path.into_path())
            .transpose()
            .map_err(failed)?;
        write_png(&bytes, path.as_deref())
    })
    .await
    .map_err(failed)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    /// Encode synthetic alpha-bearing pixels to exercise PNG decoding and native payloads without a live clipboard.
    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                &[255, 0, 0, 0, 0, 255, 0, 128, 0, 0, 255, 255, 20, 30, 40, 64],
                2,
                2,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        bytes
    }

    /// Clipboard dimensions and straight RGBA alpha come from fully decoded PNG, never CSS/display scale.
    #[test]
    fn png_and_clipboard_payload() {
        let bytes = png();
        let pixels = clipboard_image(decode(&bytes).unwrap());
        assert_eq!((pixels.width, pixels.height), (2, 2));
        assert_eq!(
            pixels.bytes.as_ref(),
            &[255, 0, 0, 0, 0, 255, 0, 128, 0, 0, 255, 255, 20, 30, 40, 64]
        );
        assert_eq!(payload(&InvokeBody::Raw(bytes.clone())).unwrap(), bytes);
        assert!(payload(&InvokeBody::Raw(vec![])).is_err());
        assert!(
            payload(&InvokeBody::Json(
                serde_json::json!({"path":"untrusted.png"})
            ))
            .is_err()
        );
    }

    /// Reject unsupported/truncated input before side effects; decoding never mutates the submitted source.
    #[test]
    fn malformed_output() {
        assert!(decode(&[]).is_err());
        assert!(decode(b"not an image").is_err());
        let bytes = png();
        let original = bytes.clone();
        assert!(decode(&bytes[..bytes.len() / 2]).is_err());
        decode(&bytes).unwrap();
        assert_eq!(bytes, original);
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[255, 255, 255], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert!(decode(&jpeg).is_err());
    }

    /// Cancellation creates nothing; confirmed replacement is complete, PNG-decodable, and leaves no temporary files.
    #[test]
    fn atomic_export_and_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.png");
        let bytes = png();
        drop(decode(&bytes).unwrap());
        assert!(!write_png(&bytes, None).unwrap());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        std::fs::write(&path, b"previous file").unwrap();
        assert!(write_png(&bytes, Some(&path)).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let decoded = decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    /// A replacement failure preserves the destination and removes any partially written temporary file.
    #[test]
    fn write_failure_preserves_destination() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("existing-directory");
        std::fs::create_dir(&destination).unwrap();
        let retained = destination.join("keep.txt");
        std::fs::write(&retained, b"keep").unwrap();
        assert!(write_png(&png(), Some(&destination)).is_err());
        assert_eq!(std::fs::read(&retained).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
