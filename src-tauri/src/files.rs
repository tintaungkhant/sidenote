//! File documents and independent hot-exit storage. Never opens the legacy notes database.
use crate::storage::StoreError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

type Result<T> = std::result::Result<T, StoreError>;
#[derive(Deserialize)]
pub struct SourceReference {
    from: usize,
    to: usize,
    path: String,
}
fn rewrite(
    source: &str,
    references: &[SourceReference],
    mappings: &std::collections::BTreeMap<String, String>,
) -> Result<String> {
    fn byte_offset(text: &str, utf16: usize) -> Result<usize> {
        let mut count = 0;
        for (offset, ch) in text.char_indices() {
            if count == utf16 {
                return Ok(offset);
            }
            count += ch.len_utf16();
        }
        if count == utf16 {
            Ok(text.len())
        } else {
            Err(fail("Invalid source reference boundary"))
        }
    }
    let mut result = source.to_owned();
    let mut end = source.len();
    for reference in references.iter().rev() {
        let from = byte_offset(source, reference.from)?;
        let to = byte_offset(source, reference.to)?;
        if from >= to || to > end {
            return Err(fail("Invalid or overlapping source references"));
        }
        end = from;
        if let Some(target) = mappings.get(&reference.path) {
            let slice = &source[from..to];
            if !slice.contains(&reference.path) {
                return Err(fail("Image reference does not match source"));
            }
            result.replace_range(from..to, &slice.replacen(&reference.path, target, 1));
        }
    }
    Ok(result)
}
const MAX_TEXT: u64 = 8 * 1024 * 1024;
pub struct Files(pub Mutex<FileStore>);
pub struct FileStore {
    recovery: PathBuf,
}
fn fail(e: impl std::fmt::Display) -> StoreError {
    StoreError {
        code: "filesystem",
        message: e.to_string(),
    }
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError {
                    code: "not_found",
                    message: e.to_string(),
                }
            } else {
                fail(e)
            }
        })?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() as u64 > limit {
        return Err(fail("File exceeds size limit"));
    }
    Ok(bytes)
}
fn text(path: &Path) -> Result<String> {
    String::from_utf8(read(path, MAX_TEXT)?).map_err(fail)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| fail("No parent directory"))?)
            .map_err(fail)?;
    tmp.write_all(bytes).map_err(fail)?;
    tmp.as_file().sync_all().map_err(fail)?;
    tmp.persist(path).map_err(fail)?;
    Ok(())
}
fn atomic_noclobber(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| fail("No parent directory"))?)
            .map_err(fail)?;
    tmp.write_all(bytes).map_err(fail)?;
    tmp.as_file().sync_all().map_err(fail)?;
    tmp.persist_noclobber(path).map_err(fail)?;
    Ok(())
}
fn not_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(fail(e)),
    }
}
fn is_available(path: &Path) -> Result<bool> {
    Ok(not_present(path)? && not_present(&companion(path)?)?)
}
fn sanitize_stem_ext(preferred: Option<&str>) -> Result<(String, String)> {
    let raw = preferred.map(str::trim).filter(|s| !s.is_empty());
    let name = raw.unwrap_or("Untitled-1.md");
    if name == "."
        || name == ".."
        || name.chars().count() > 200
        || name.ends_with('.')
        || name.ends_with(' ')
    {
        return Err(fail("Invalid file name"));
    }
    let lower = name.to_ascii_lowercase();
    if lower == ".simple-note.json" || lower.ends_with(".simple-note.json") {
        return Err(fail("Reserved metadata file name"));
    }
    if name.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
    }) {
        return Err(fail("File name contains unsupported characters"));
    }
    let (stem, ext) = match name.rfind('.') {
        Some(pos) => (&name[..pos], &name[pos + 1..]),
        None => (name, "md"),
    };
    if stem.is_empty() || ext.is_empty() || stem.ends_with(' ') || stem.ends_with('.') {
        return Err(fail("Invalid file name"));
    }
    let base = stem.split('.').next().unwrap_or(stem).trim_end_matches(' ');
    let upper = base.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit()
            && upper.as_bytes()[3] != b'0');
    if reserved {
        return Err(fail("Reserved system file name"));
    }
    Ok((stem.to_owned(), ext.to_owned()))
}
fn strip_numeric_suffix(stem: &str) -> &str {
    if let Some((prefix, suffix)) = stem.rsplit_once('-')
        && !prefix.is_empty()
        && !suffix.is_empty()
        && suffix.bytes().all(|b| b.is_ascii_digit())
    {
        return prefix;
    }
    stem
}
/// Unlike rename-with-replace, Windows ReplaceFile refuses to resurrect a deleted destination.
fn replace_existing(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(fail)?;
    tmp.write_all(bytes).map_err(fail)?;
    tmp.as_file().sync_all().map_err(fail)?;
    let temporary_path = tmp.into_temp_path(); // Close the handle before Windows replaces the directory entry.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let temporary: Vec<u16> = temporary_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Both buffers are NUL-terminated and remain alive for the call. No backup or reserved pointers.
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
                destination.as_ptr(),
                temporary.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(fail(std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        // Retain the old inode until the new file is ready. Never knowingly recreate a missing source.
        if !path.is_file() {
            return Err(fail("File was deleted; use Save As"));
        }
        temporary_path.persist(path).map_err(fail)?;
        Ok(())
    }
}
fn companion(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().unwrap();
    let package = parent.join(".simple-note.json");
    // Only use package metadata when it names this main file; adjacent standalone files stay independent.
    if package.exists() {
        let raw = text(&package)?;
        if serde_json::from_str::<Value>(&raw)
            .map_err(fail)?
            .as_object()
            .and_then(|v| v.get("main"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            == path.file_name().map(|n| n.to_string_lossy().into_owned())
        {
            return Ok(package);
        }
    }
    Ok(parent.join(format!(
        "{}.simple-note.json",
        path.file_name().unwrap().to_string_lossy()
    )))
}
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiskDocument {
    path: String,
    text: String,
    metadata: Value,
    revision: String,
}
fn load(path: &Path) -> Result<DiskDocument> {
    // Read first so callers can distinguish deletion from permissions and malformed sidecars.
    let source = text(path)?;
    let path = fs::canonicalize(path).map_err(fail)?;
    let sidecar = companion(&path)?;
    let raw = match text(&sidecar) {
        Ok(s) => s,
        Err(_) if !sidecar.exists() => String::new(),
        Err(e) => return Err(e),
    };
    let metadata = if raw.is_empty() {
        json!({"version":1,"images":{}})
    } else {
        serde_json::from_str(&raw).map_err(fail)?
    };
    Ok(DiskDocument {
        path: path.to_string_lossy().into_owned(),
        revision: serde_json::to_string(&(&source, &raw)).map_err(fail)?,
        text: source,
        metadata,
    })
}
fn validate_metadata(value: &Value) -> Result<()> {
    if value.get("version").and_then(Value::as_u64) != Some(1)
        || !value.get("images").is_some_and(Value::is_object)
    {
        return Err(fail(
            "Unsupported companion metadata; original file preserved",
        ));
    }
    if serde_json::to_vec(value).map_err(fail)?.len() as u64 > MAX_TEXT {
        return Err(fail("Metadata exceeds 8 MiB"));
    }
    #[derive(Deserialize)]
    struct ImageMetadata {
        id: String,
        annotations: Vec<crate::storage::Annotation>,
    }
    for (path, data) in value["images"].as_object().unwrap() {
        let image: ImageMetadata = serde_json::from_value(data.clone()).map_err(fail)?;
        if path.len() > 4096
            || image.id.is_empty()
            || image.id.len() > 128
            || image.annotations.len() > 10000
        {
            return Err(fail("Invalid image metadata"));
        }
        for annotation in &image.annotations {
            crate::storage::validate_annotation(annotation, 40_000_000, 40_000_000)?;
        }
    }
    Ok(())
}
fn validate_snapshot(value: &Value) -> Result<()> {
    let buffers = value["buffers"]
        .as_array()
        .filter(|b| b.len() <= 1000)
        .ok_or_else(|| fail("Invalid recovery buffers"))?;
    if value["version"] != 1 {
        return Err(fail("Unsupported recovery version"));
    }
    for buffer in buffers {
        if !buffer["text"].is_string() {
            return Err(fail("Invalid recovery source"));
        }
        validate_metadata(&buffer["metadata"])?;
    }
    Ok(())
}
fn save(path: &Path, expected: &str, source: &str, metadata: &Value) -> Result<DiskDocument> {
    validate_metadata(metadata)?;
    if source.len() as u64 > MAX_TEXT {
        return Err(fail("Text exceeds 8 MiB"));
    }
    let current = load(path)?; // A deleted file is never recreated by Save.
    if current.revision != expected {
        return Err(fail(
            "File or companion changed externally. Reload or Save As to keep your edits.",
        ));
    }
    let sidecar = companion(path)?;
    // Recovery is committed by the renderer before this operation. Metadata first keeps new references resolvable.
    atomic(
        &sidecar,
        &serde_json::to_vec_pretty(metadata).map_err(fail)?,
    )?;
    replace_existing(path, source.as_bytes())?;
    load(path)
}
fn asset_path(base: &Path, reference: &str) -> Result<PathBuf> {
    if reference.contains('\\')
        || reference.contains(':')
        || reference.starts_with('/')
        || reference.split('/').any(|s| s == "..")
    {
        return Err(fail(
            "Image paths must be relative and stay inside the document folder",
        ));
    }
    let root = fs::canonicalize(base).map_err(fail)?;
    let path = fs::canonicalize(root.join(reference)).map_err(fail)?;
    if !path.starts_with(root) {
        return Err(fail("Image points outside the document folder"));
    }
    Ok(path)
}
impl FileStore {
    pub fn new(directory: PathBuf) -> Result<Self> {
        let recovery = directory.join("file-editor-recovery");
        fs::create_dir_all(recovery.join("assets")).map_err(fail)?;
        Ok(Self { recovery })
    }
    fn save(
        &self,
        path: &Path,
        expected: &str,
        source: &str,
        metadata: &Value,
        assets: &[SourceReference],
    ) -> Result<DiskDocument> {
        if load(path)?.revision != expected {
            return Err(fail("File changed externally. Reload or Save As."));
        }
        let mut source = source.to_owned();
        let mut metadata = metadata.clone();
        validate_metadata(&metadata)?;
        let mut mappings = std::collections::BTreeMap::new();
        for asset in assets {
            let reference = &asset.path;
            if let Some(relative) = reference.strip_prefix("recovery:") {
                if !relative
                    .strip_prefix("assets/")
                    .is_some_and(|name| !name.is_empty() && !name.contains('/'))
                {
                    return Err(fail("Invalid recovery asset reference"));
                }
                let bytes = read(&asset_path(&self.recovery, relative)?, 20 * 1024 * 1024)?;
                validate_image(&bytes)?;
                let destination = format!("./{relative}");
                let assets_folder = path.parent().unwrap().join("assets");
                fs::create_dir_all(&assets_folder).map_err(fail)?;
                let root = fs::canonicalize(path.parent().unwrap()).map_err(fail)?;
                if !fs::canonicalize(&assets_folder)
                    .map_err(fail)?
                    .starts_with(root)
                {
                    return Err(fail("Assets directory points outside the document folder"));
                }
                let filename = Path::new(relative)
                    .file_name()
                    .ok_or_else(|| fail("Invalid asset name"))?;
                let target = assets_folder.join(filename);
                if target.exists() {
                    if read(&target, 20 * 1024 * 1024)? != bytes {
                        return Err(fail("Asset name collision"));
                    }
                } else {
                    atomic(&target, &bytes)?;
                }
                mappings.insert(reference.clone(), destination.clone());
                if let Some(value) = metadata["images"]
                    .as_object_mut()
                    .unwrap()
                    .remove(reference)
                {
                    metadata["images"][&destination] = value;
                }
            }
        }
        source = rewrite(&source, assets, &mappings)?;
        save(path, expected, &source, &metadata)
    }
    fn package(
        &self,
        destination: &Path,
        source: &str,
        metadata: &Value,
        origin: Option<&Path>,
        assets: &[SourceReference],
    ) -> Result<DiskDocument> {
        validate_metadata(metadata)?;
        if source.len() as u64 > MAX_TEXT {
            return Err(fail("Text exceeds 8 MiB"));
        }
        let name = destination
            .file_name()
            .ok_or_else(|| fail("Choose a file name and extension"))?;
        let stem = destination
            .file_stem()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fail("Choose a document name"))?;
        let parent = destination
            .parent()
            .ok_or_else(|| fail("Choose a location"))?;
        let folder = parent.join(stem);
        if folder.exists() {
            return Err(fail(
                "Document folder already exists. Choose another name or location; nothing was overwritten.",
            ));
        }
        let stage = tempfile::tempdir_in(parent).map_err(fail)?;
        let mut rewritten = source.to_owned();
        let mut meta = metadata.clone();
        let images = meta["images"].as_object_mut().unwrap();
        let mut relocated = serde_json::Map::new();
        fs::create_dir(stage.path().join("assets")).map_err(fail)?;
        let mut mappings = std::collections::BTreeMap::new();
        for (index, asset) in assets.iter().enumerate() {
            let reference = &asset.path;
            if mappings.contains_key(reference) {
                continue;
            }
            let (base, relative) = if let Some(r) = reference.strip_prefix("recovery:") {
                (self.recovery.as_path(), r)
            } else {
                (
                    origin.ok_or_else(|| fail("Image has no source folder"))?,
                    reference.as_str(),
                )
            };
            let original = asset_path(base, relative)?;
            let bytes = read(&original, 20 * 1024 * 1024)?;
            validate_image(&bytes)?;
            let ext = if image::guess_format(&bytes).map_err(fail)? == image::ImageFormat::Png {
                "png"
            } else {
                "jpg"
            };
            let target = format!("./assets/image-{}.{ext}", index + 1);
            atomic(&stage.path().join(&target), &bytes)?;
            // Replace only explicit references supplied by the source parser, never arbitrary source substrings.
            mappings.insert(reference.clone(), target.clone());
            if let Some(value) = images.remove(reference) {
                relocated.insert(target, value);
            }
        }
        rewritten = rewrite(&rewritten, assets, &mappings)?;
        images.extend(relocated);
        meta["main"] = json!(name.to_string_lossy());
        atomic(&stage.path().join(name), rewritten.as_bytes())?;
        atomic(
            &stage.path().join(".simple-note.json"),
            &serde_json::to_vec_pretty(&meta).map_err(fail)?,
        )?;
        // Publish a fully prepared folder without replacing an existing document.
        fs::rename(stage.path(), &folder).map_err(fail)?;
        load(&folder.join(name))
    }
    fn create_in_root(
        &self,
        root: &Path,
        preferred: Option<&str>,
        source: &str,
        metadata: &Value,
        origin: Option<&Path>,
        assets: &[SourceReference],
    ) -> Result<DiskDocument> {
        validate_metadata(metadata)?;
        if source.len() as u64 > MAX_TEXT {
            return Err(fail("Text exceeds 8 MiB"));
        }
        let root = fs::canonicalize(root).map_err(fail)?;
        if !root.is_dir() {
            return Err(fail("Root path is not a folder"));
        }
        let (stem, ext) = sanitize_stem_ext(preferred)?;
        let initial = root.join(format!("{stem}.{ext}"));
        let target = if is_available(&initial)? {
            initial
        } else {
            let base = strip_numeric_suffix(&stem);
            let mut found = None;
            for n in 1..=100_000 {
                let candidate = root.join(format!("{base}-{n}.{ext}"));
                if is_available(&candidate)? {
                    found = Some(candidate);
                    break;
                }
            }
            found.ok_or_else(|| fail("Too many files with that name in root folder"))?
        };
        let mut rewritten = source.to_owned();
        let mut meta = metadata.clone();
        if !assets.is_empty() {
            let assets_folder = root.join("assets");
            fs::create_dir_all(&assets_folder).map_err(fail)?;
            let canonical_assets = fs::canonicalize(&assets_folder).map_err(fail)?;
            if !canonical_assets.starts_with(&root)
                || canonical_assets.parent() != Some(root.as_path())
            {
                return Err(fail("Assets folder points outside the root folder"));
            }
            let mut mappings = std::collections::BTreeMap::new();
            for asset in assets {
                let reference = &asset.path;
                if mappings.contains_key(reference) {
                    continue;
                }
                if let Some(relative) = reference.strip_prefix("recovery:") {
                    if !relative
                        .strip_prefix("assets/")
                        .is_some_and(|name| !name.is_empty() && !name.contains('/'))
                    {
                        return Err(fail("Invalid recovery asset reference"));
                    }
                    let bytes = read(&asset_path(&self.recovery, relative)?, 20 * 1024 * 1024)?;
                    validate_image(&bytes)?;
                    let img_ext =
                        if image::guess_format(&bytes).map_err(fail)? == image::ImageFormat::Png {
                            "png"
                        } else {
                            "jpg"
                        };
                    let filename = Path::new(relative)
                        .file_name()
                        .ok_or_else(|| fail("Invalid asset name"))?;
                    let candidate = assets_folder.join(filename);
                    let destination = if candidate.exists() {
                        if read(&candidate, 20 * 1024 * 1024)? == bytes {
                            format!("./{relative}")
                        } else {
                            let new_rel = format!("assets/{}.{img_ext}", uuid::Uuid::new_v4());
                            atomic_noclobber(&root.join(&new_rel), &bytes)?;
                            format!("./{new_rel}")
                        }
                    } else {
                        atomic_noclobber(&candidate, &bytes)?;
                        format!("./{relative}")
                    };
                    mappings.insert(reference.clone(), destination.clone());
                    if let Some(value) = meta["images"].as_object_mut().unwrap().remove(reference) {
                        meta["images"][&destination] = value;
                    }
                } else {
                    let origin = origin.ok_or_else(|| fail("Image has no source folder"))?;
                    let original = asset_path(origin, reference)?;
                    let bytes = read(&original, 20 * 1024 * 1024)?;
                    validate_image(&bytes)?;
                    let same_root = fs::canonicalize(origin).ok().as_ref() == Some(&root);
                    if same_root && asset_path(&root, reference).is_ok() {
                        continue;
                    }
                    let img_ext =
                        if image::guess_format(&bytes).map_err(fail)? == image::ImageFormat::Png {
                            "png"
                        } else {
                            "jpg"
                        };
                    let new_rel = format!("assets/{}.{img_ext}", uuid::Uuid::new_v4());
                    atomic_noclobber(&root.join(&new_rel), &bytes)?;
                    let destination = format!("./{new_rel}");
                    mappings.insert(reference.clone(), destination.clone());
                    if let Some(value) = meta["images"].as_object_mut().unwrap().remove(reference) {
                        meta["images"][&destination] = value;
                    }
                }
            }
            rewritten = rewrite(&rewritten, assets, &mappings)?;
        }
        if let Some(obj) = meta.as_object_mut() {
            obj.remove("main");
        }
        atomic_noclobber(&target, rewritten.as_bytes())?;
        let sidecar = root.join(format!(
            "{}.simple-note.json",
            target.file_name().unwrap().to_string_lossy()
        ));
        atomic(&sidecar, &serde_json::to_vec_pretty(&meta).map_err(fail)?)?;
        load(&target)
    }
    fn rename(&self, root: &Path, path: &Path, name: &str) -> Result<DiskDocument> {
        let root = fs::canonicalize(root).map_err(fail)?;
        if !root.is_dir() {
            return Err(fail("Root path is not a folder"));
        }
        let current = fs::canonicalize(path).map_err(fail)?;
        if !current.starts_with(&root) || current == root {
            return Err(fail("File is outside the workspace root"));
        }
        let trimmed = name.trim();
        let (stem, parsed_ext) = sanitize_stem_ext(Some(trimmed))?;
        let ext = if trimmed.contains('.') {
            parsed_ext
        } else {
            current
                .extension()
                .and_then(|e| e.to_str())
                .filter(|e| !e.is_empty())
                .map(str::to_owned)
                .unwrap_or(parsed_ext)
        };
        let final_name = format!("{stem}.{ext}");
        let parent = current
            .parent()
            .ok_or_else(|| fail("No parent directory"))?;
        let target = parent.join(&final_name);
        if target == current {
            return load(&current);
        }
        if !is_available(&target)? {
            return Err(fail("A file with that name already exists"));
        }
        let _ = load(&current)?;
        let old_sidecar = companion(&current)?;
        fs::rename(&current, &target).map_err(fail)?;
        if old_sidecar.file_name() == Some(std::ffi::OsStr::new(".simple-note.json")) {
            let mut meta: Value = serde_json::from_str(&text(&old_sidecar)?).map_err(fail)?;
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("main".into(), json!(final_name));
            }
            atomic(
                &old_sidecar,
                &serde_json::to_vec_pretty(&meta).map_err(fail)?,
            )?;
        } else if old_sidecar.exists() {
            let new_sidecar = parent.join(format!("{final_name}.simple-note.json"));
            fs::rename(&old_sidecar, &new_sidecar).map_err(fail)?;
        }
        load(&target)
    }
    fn snapshot(&self, snapshot: &str) -> Result<()> {
        if snapshot.len() > 64 * 1024 * 1024 {
            return Err(fail("Recovery exceeds 64 MiB"));
        }
        let v: Value = serde_json::from_str(snapshot).map_err(fail)?;
        validate_snapshot(&v)?;
        atomic(&self.recovery.join("session.json"), snapshot.as_bytes())
    }
}
pub fn validate_image(bytes: &[u8]) -> Result<()> {
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(fail("Image exceeds 20 MiB"));
    }
    let format = image::guess_format(bytes).map_err(fail)?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return Err(fail("Only PNG and JPEG are supported"));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(fail)?;
    if u64::from(w) * u64::from(h) > 40_000_000 {
        return Err(fail("Image exceeds 40 megapixels"));
    }
    reader.decode().map_err(fail)?;
    Ok(())
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Operation {
    Open {
        path: String,
    },
    Save {
        path: String,
        expected: String,
        text: String,
        metadata: Value,
        assets: Vec<SourceReference>,
    },
    Package {
        destination: String,
        text: String,
        metadata: Value,
        origin: Option<String>,
        assets: Vec<SourceReference>,
    },
    CreateInRoot {
        root: String,
        preferred: Option<String>,
        text: String,
        metadata: Value,
        origin: Option<String>,
        assets: Vec<SourceReference>,
    },
    Rename {
        root: String,
        path: String,
        name: String,
    },
    Recover,
    Snapshot {
        snapshot: String,
    },
    Image {
        document: Option<String>,
        reference: String,
    },
    PutImage {
        document: Option<String>,
        bytes: Vec<u8>,
    },
    Browse {
        root: String,
        query: String,
    },
}
fn browse(root: &Path, query: &str) -> Result<Value> {
    fn visit(
        root: &Path,
        path: &Path,
        query: &str,
        entries: &mut Vec<Value>,
        visited: &mut usize,
        depth: usize,
    ) -> Result<()> {
        if depth > 64 {
            return Err(fail(
                "Folder nesting exceeds 64 levels; open a smaller folder",
            ));
        }
        if entries.len() >= 10000 {
            return Err(fail("Folder exceeds 10,000 entries; open a smaller folder"));
        }
        let mut children = fs::read_dir(path)
            .map_err(fail)?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(fail)?;
        children.sort_by_key(|e| e.file_name());
        for entry in children {
            *visited += 1;
            if *visited > 20000 {
                return Err(fail("Folder exceeds 20,000 entries; open a smaller folder"));
            }
            let kind = entry.file_type().map_err(fail)?;
            if kind.is_symlink() {
                continue;
            }
            let p = entry.path();
            let relative = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if kind.is_dir() {
                if matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "node_modules" | "target" | "assets")
                ) {
                    continue;
                }
                if query.is_empty() {
                    entries.push(json!({"path":p,"relative":relative,"directory":true}));
                }
                visit(root, &p, query, entries, visited, depth + 1)?;
            } else if kind.is_file() {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if name == ".simple-note.json" || name.ends_with(".simple-note.json") {
                    continue;
                }
                if query.is_empty() {
                    entries.push(json!({"path":p,"relative":relative,"directory":false}));
                } else if let Ok(source) = text(&p) {
                    if source.contains('\0') {
                        continue;
                    }
                    for (index, line) in source.lines().enumerate() {
                        if line.to_lowercase().contains(&query.to_lowercase()) {
                            entries.push(json!({"path":p,"relative":relative,"directory":false,"line":index+1,"preview":line.chars().take(240).collect::<String>()}));
                            if entries.len() >= 10000 {
                                return Err(fail(
                                    "Search exceeds 10,000 results; narrow the query",
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, query, &mut entries, &mut 0, 0)?;
    Ok(json!(entries))
}
#[tauri::command]
pub async fn file_operation(
    app: tauri::AppHandle,
    state: State<'_, Files>,
    operation: Operation,
) -> Result<Value> {
    let recovery = state.0.lock().map_err(fail)?.recovery.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // One mutex serializes recovery, asset creation, and document commits.
        let state = app.state::<Files>();
        let store = state.0.lock().map_err(fail)?;
        match operation {
            Operation::Open { path } => {
                let doc = load(Path::new(&path))?;
                validate_metadata(&doc.metadata)?;
                Ok(json!(doc))
            }
            Operation::Save {
                path,
                expected,
                text,
                metadata,
                assets,
            } => Ok(json!(store.save(
                Path::new(&path),
                &expected,
                &text,
                &metadata,
                &assets
            )?)),
            Operation::Package {
                destination,
                text,
                metadata,
                origin,
                assets,
            } => Ok(json!(store.package(
                Path::new(&destination),
                &text,
                &metadata,
                origin.as_deref().map(Path::new),
                &assets
            )?)),
            Operation::CreateInRoot {
                root,
                preferred,
                text,
                metadata,
                origin,
                assets,
            } => Ok(json!(store.create_in_root(
                Path::new(&root),
                preferred.as_deref(),
                &text,
                &metadata,
                origin.as_deref().map(Path::new),
                &assets
            )?)),
            Operation::Rename { root, path, name } => Ok(json!(store.rename(
                Path::new(&root),
                Path::new(&path),
                &name
            )?)),
            Operation::Recover => {
                let p = recovery.join("session.json");
                if !p.exists() {
                    Ok(Value::Null)
                } else {
                    let value =
                        serde_json::from_slice(&read(&p, 64 * 1024 * 1024)?).map_err(fail)?;
                    validate_snapshot(&value)?;
                    Ok(value)
                }
            }
            Operation::Snapshot { snapshot } => {
                store.snapshot(&snapshot)?;
                Ok(Value::Null)
            }
            Operation::Image {
                document,
                reference,
            } => {
                let (base, relative) = if let Some(r) = reference.strip_prefix("recovery:") {
                    (recovery, r)
                } else {
                    (
                        Path::new(
                            document
                                .as_deref()
                                .ok_or_else(|| fail("Image has no document folder"))?,
                        )
                        .parent()
                        .unwrap()
                        .to_owned(),
                        reference.as_str(),
                    )
                };
                let bytes = read(&asset_path(&base, relative)?, 20 * 1024 * 1024)?;
                validate_image(&bytes)?;
                Ok(json!(bytes))
            }
            Operation::PutImage { document, bytes } => {
                validate_image(&bytes)?;
                let ext = if image::guess_format(&bytes).map_err(fail)? == image::ImageFormat::Png {
                    "png"
                } else {
                    "jpg"
                };
                let relative = format!("assets/{}.{ext}", uuid::Uuid::new_v4());
                let base = if let Some(path) = &document {
                    // Deleted documents may retain pasted assets in recovery, never resurrect them.
                    if !Path::new(path).is_file() {
                        return Err(fail(
                            "Document was deleted. Save As before inserting an image.",
                        ));
                    }
                    Path::new(path).parent().unwrap().to_owned()
                } else {
                    recovery
                };
                fs::create_dir_all(base.join("assets")).map_err(fail)?;
                if fs::canonicalize(base.join("assets"))
                    .map_err(fail)?
                    .parent()
                    != Some(fs::canonicalize(&base).map_err(fail)?.as_path())
                {
                    return Err(fail("Assets folder must not be a symbolic link"));
                }
                atomic(&base.join(&relative), &bytes)?;
                Ok(json!(if document.is_some() {
                    format!("./{relative}")
                } else {
                    format!("recovery:{relative}")
                }))
            }
            Operation::Browse { root, query } => browse(Path::new(&root), &query),
        }
    })
    .await
    .map_err(fail)?
}
#[tauri::command]
pub async fn file_picker(
    app: tauri::AppHandle,
    kind: String,
    name: Option<String>,
) -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let picker = app.dialog().file();
        let selected = match kind.as_str() {
            "folder" => picker.blocking_pick_folder(),
            "save" => picker
                .set_file_name(name.unwrap_or_else(|| "Untitled.md".into()))
                .add_filter(
                    "Text and source",
                    &["md", "txt", "js", "ts", "php", "html", "css", "json"],
                )
                .blocking_save_file(),
            "open" => picker.blocking_pick_file(),
            _ => return Err(fail("Invalid picker")),
        };
        selected
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(fail)
            })
            .transpose()
    })
    .await
    .map_err(fail)?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }
    fn metadata(reference: &str) -> Value {
        json!({"version":1,"images":{reference:{"id":"stable","annotations":[{"kind":"caption","text":"A caption"}]}}})
    }
    fn reference(source: &str, path: &str) -> SourceReference {
        let from = source
            .find("![image]")
            .or_else(|| source.find("// [image:"))
            .unwrap();
        let to = source[from..]
            .find('\n')
            .map(|n| from + n)
            .unwrap_or(source.len());
        SourceReference {
            from: source[..from].encode_utf16().count(),
            to: source[..to].encode_utf16().count(),
            path: path.into(),
        }
    }
    #[test]
    fn package_reopens_after_moving_with_portable_assets_and_annotations() {
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::new(root.path().join("app")).unwrap();
        let image = png();
        fs::write(store.recovery.join("assets/a.png"), &image).unwrap();
        let path = "recovery:assets/a.png";
        let source = format!("😀 title\n![image]({path})\n");
        let disk = store
            .package(
                &root.path().join("example.md"),
                &source,
                &metadata(path),
                None,
                &[reference(&source, path)],
            )
            .unwrap();
        assert_eq!(disk.text, "😀 title\n![image](./assets/image-1.png)\n");
        assert_eq!(
            disk.metadata["images"]["./assets/image-1.png"]["id"],
            "stable"
        );
        assert!(root.path().join("example/.simple-note.json").exists());
        fs::rename(root.path().join("example"), root.path().join("shared")).unwrap();
        let reopened = load(&root.path().join("shared/example.md")).unwrap();
        assert_eq!(reopened.text, disk.text);
        assert_eq!(reopened.metadata, disk.metadata);
        assert_eq!(
            read(
                &asset_path(&root.path().join("shared"), "./assets/image-1.png").unwrap(),
                20 * 1024 * 1024
            )
            .unwrap(),
            image
        );
        assert!(!root.path().join("app/notes.sqlite3").exists());
    }
    #[test]
    fn package_collision_or_missing_asset_never_overwrites_an_existing_document() {
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::new(root.path().join("app")).unwrap();
        let empty = json!({"version":1,"images":{}});
        let destination = root.path().join("work.ts");
        store
            .package(&destination, "const x = 1;", &empty, None, &[])
            .unwrap();
        assert!(
            store
                .package(&destination, "replacement", &empty, None, &[])
                .is_err()
        );
        assert_eq!(
            text(&root.path().join("work/work.ts")).unwrap(),
            "const x = 1;"
        );
        let source = "![image](recovery:assets/missing.png)";
        assert!(
            store
                .package(
                    &root.path().join("missing.md"),
                    source,
                    &empty,
                    None,
                    &[reference(source, "recovery:assets/missing.png")]
                )
                .is_err()
        );
        assert!(!root.path().join("missing").exists());
    }
    #[test]
    fn standalone_save_stays_in_place_and_rejects_external_edits_sidecar_changes_and_deletion() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("plain.ts");
        fs::write(&path, "const x = 1;\r\n").unwrap();
        let opened = load(&path).unwrap();
        let empty = json!({"version":1,"images":{}});
        let saved = save(&path, &opened.revision, "const x = 2;\r\n", &empty).unwrap();
        assert_eq!(text(&path).unwrap(), "const x = 2;\r\n");
        assert!(root.path().join("plain.ts.simple-note.json").exists());
        assert!(!root.path().join("plain").exists());
        fs::write(&path, "external").unwrap();
        assert!(save(&path, &saved.revision, "local", &empty).is_err());
        assert_eq!(text(&path).unwrap(), "external");
        let latest = load(&path).unwrap();
        fs::write(
            companion(&path).unwrap(),
            "{\"version\":1,\"images\":{},\"external\":true}",
        )
        .unwrap();
        assert!(save(&path, &latest.revision, "local", &empty).is_err());
        fs::remove_file(&path).unwrap();
        assert!(save(&path, &latest.revision, "local", &empty).is_err());
        assert!(!path.exists());
    }
    #[test]
    fn saving_pasted_recovery_assets_rewrites_only_source_reference_ranges() {
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::new(root.path().join("app")).unwrap();
        fs::write(store.recovery.join("assets/a.png"), png()).unwrap();
        let path = root.path().join("main.ts");
        fs::write(&path, "").unwrap();
        let opened = load(&path).unwrap();
        let image = "recovery:assets/a.png";
        let source = format!("const s = '[image:{image}]';\n// [image:{image}]\n");
        let saved = store
            .save(
                &path,
                &opened.revision,
                &source,
                &metadata(image),
                &[reference(&source, image)],
            )
            .unwrap();
        assert!(
            saved
                .text
                .contains(&format!("const s = '[image:{image}]';"))
        );
        assert!(saved.text.contains("// [image:./assets/a.png]"));
        assert!(root.path().join("assets/a.png").exists());
        assert!(store.recovery.join("assets/a.png").exists());
        assert_eq!(
            saved.metadata["images"]["./assets/a.png"]["annotations"][0]["text"],
            "A caption"
        );
    }
    #[test]
    fn hot_exit_is_atomic_separate_and_invalid_recovery_preserves_last_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::new(root.path().to_owned()).unwrap();
        let snapshot = r#"{"version":1,"buffers":[{"text":"unsaved","metadata":{"version":1,"images":{}}}],"active":"tab"}"#;
        store.snapshot(snapshot).unwrap();
        assert!(store.snapshot("corrupt").is_err());
        assert_eq!(
            text(&store.recovery.join("session.json")).unwrap(),
            snapshot
        );
        let reopened = FileStore::new(root.path().to_owned()).unwrap();
        assert_eq!(
            text(&reopened.recovery.join("session.json")).unwrap(),
            snapshot
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn workspace_browse_and_search_read_real_files_and_reflect_renames_and_deletions() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        let path = root.path().join("src/main.php");
        fs::write(&path, "<?php\n// needle\necho 1;").unwrap();
        assert_eq!(
            browse(root.path(), "").unwrap().as_array().unwrap().len(),
            2
        );
        let found = browse(root.path(), "NEEDLE").unwrap();
        assert_eq!(found[0]["line"], 2);
        fs::rename(&path, root.path().join("src/renamed.php")).unwrap();
        assert!(load(&path).is_err());
        assert_eq!(
            browse(root.path(), "needle").unwrap()[0]["relative"],
            "src/renamed.php"
        );
        fs::remove_file(root.path().join("src/renamed.php")).unwrap();
        assert!(
            browse(root.path(), "needle")
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn unsafe_paths_bad_metadata_and_unreadable_images_fail_without_losing_source() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("bad.png"), b"invalid").unwrap();
        assert!(validate_image(b"invalid").is_err());
        assert!(asset_path(root.path(), "../outside.png").is_err());
        assert!(asset_path(root.path(), "C:/outside.png").is_err());
        assert!(validate_metadata(&json!({"version":2,"images":{}})).is_err());
        assert!(
            validate_metadata(&json!({"version":1,"images":{"x":{"id":"x","annotations":42}}}))
                .is_err()
        );
        let mut mappings = std::collections::BTreeMap::new();
        mappings.insert("a".into(), "b".into());
        assert!(
            rewrite(
                "😀a",
                &[SourceReference {
                    from: 1,
                    to: 3,
                    path: "a".into()
                }],
                &mappings
            )
            .is_err()
        );
    }
    #[test]
    fn create_in_root_autonames_without_overwriting_and_materializes_recovery_images() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let store = FileStore::new(dir.path().join("app")).unwrap();
        let image = png();
        fs::write(store.recovery.join("assets/a.png"), &image).unwrap();
        let path = "recovery:assets/a.png";
        let source = format!("First note\n![image]({path})\n");
        let first = store
            .create_in_root(
                &root,
                Some("Untitled-1.md"),
                &source,
                &metadata(path),
                None,
                &[reference(&source, path)],
            )
            .unwrap();
        assert_eq!(first.text, "First note\n![image](./assets/a.png)\n");
        assert!(root.join("Untitled-1.md").exists());
        assert!(root.join("Untitled-1.md.simple-note.json").exists());
        assert!(root.join("assets/a.png").exists());
        let empty = json!({"version":1,"images":{}});
        let second = store
            .create_in_root(
                &root,
                Some("Untitled-1.md"),
                "Second note",
                &empty,
                None,
                &[],
            )
            .unwrap();
        assert!(root.join("Untitled-2.md").exists());
        assert_eq!(second.text, "Second note");
        assert_eq!(
            text(&root.join("Untitled-1.md")).unwrap(),
            "First note\n![image](./assets/a.png)\n"
        );
        let listed = browse(&root, "").unwrap();
        let relatives: Vec<_> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["relative"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(relatives, vec!["Untitled-1.md", "Untitled-2.md"]);
    }
    #[test]
    fn rename_stays_within_root_moves_sidecar_and_refuses_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        let outside = dir.path().join("outside.md");
        fs::create_dir(&root).unwrap();
        fs::write(&outside, "outside").unwrap();
        let store = FileStore::new(dir.path().join("app")).unwrap();
        let empty = json!({"version":1,"images":{}});
        let doc = store
            .create_in_root(&root, Some("Untitled-1.md"), "hello", &empty, None, &[])
            .unwrap();
        store
            .create_in_root(&root, Some("taken.md"), "taken", &empty, None, &[])
            .unwrap();
        let renamed = store
            .rename(&root, Path::new(&doc.path), "sample.md")
            .unwrap();
        assert!(!root.join("Untitled-1.md").exists());
        assert!(!root.join("Untitled-1.md.simple-note.json").exists());
        assert!(root.join("sample.md").exists());
        assert!(root.join("sample.md.simple-note.json").exists());
        assert_eq!(renamed.text, "hello");
        assert!(
            store
                .rename(&root, Path::new(&renamed.path), "../escape.md")
                .is_err()
        );
        assert!(
            store
                .rename(&root, Path::new(&renamed.path), "taken.md")
                .is_err()
        );
        assert!(store.rename(&root, &outside, "hijack.md").is_err());
    }
}
