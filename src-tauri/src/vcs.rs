//! Per-note Git version control stored cleanly inside each note package folder.
use crate::storage::StoreError;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tauri::{Manager, State};

type Result<T> = std::result::Result<T, StoreError>;

fn fail(e: impl std::fmt::Display) -> StoreError {
    StoreError {
        code: "vcs",
        message: e.to_string(),
    }
}

fn git_missing() -> StoreError {
    StoreError {
        code: "git_missing",
        message: "Git is not installed. Please install Git (https://git-scm.com) first.".into(),
    }
}

fn git_cmd(cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("LC_ALL", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(crate::files::clean_os_path(dir));
    }
    cmd
}

fn run_git(cwd: Option<&Path>, args: &[&str]) -> Result<Output> {
    git_cmd(cwd).args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            git_missing()
        } else {
            fail(e)
        }
    })
}

fn run_git_ok(cwd: Option<&Path>, args: &[&str]) -> Result<Output> {
    let out = run_git(cwd, args)?;
    if out.status.success() {
        return Ok(out);
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let msg = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("git {} failed", args.first().copied().unwrap_or("command"))
    };
    Err(fail(msg))
}

fn resolve_note_repo(root: &Path, path: &Path) -> Result<(PathBuf, String)> {
    let root = fs::canonicalize(root).map_err(fail)?;
    if !root.is_dir() {
        return Err(fail("Root path is not a folder"));
    }
    let current = fs::canonicalize(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StoreError {
                code: "unsupported_vcs",
                message: "Version control is available for saved root notes.".into(),
            }
        } else {
            fail(e)
        }
    })?;
    if !current.starts_with(&root) || current == root || !current.is_file() {
        return Err(StoreError {
            code: "unsupported_vcs",
            message: "Version control is available for saved root notes.".into(),
        });
    }
    let parent = current
        .parent()
        .ok_or_else(|| fail("No parent directory"))?;
    let file_name = current
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| fail("Invalid file name"))?;
    if parent == root || crate::files::package_main(parent).as_deref() != Some(file_name) {
        return Err(StoreError {
            code: "unsupported_vcs",
            message: "Version control is available for saved root notes.".into(),
        });
    }
    let git_dir = parent.join(".git");
    if let Ok(meta) = fs::symlink_metadata(&git_dir)
        && meta.file_type().is_symlink()
    {
        return Err(fail("Git folder must not be a symbolic link"));
    }
    Ok((crate::files::clean_os_path(parent), file_name.to_owned()))
}

fn validate_rev(rev: &str, allow_parent: bool) -> Result<()> {
    if rev == "HEAD" {
        return Ok(());
    }
    let base = if allow_parent {
        rev.strip_suffix('^').unwrap_or(rev)
    } else {
        rev
    };
    if (7..=64).contains(&base.len()) && base.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(fail("Invalid commit reference"))
    }
}

fn has_git_head(repo_dir: &Path) -> Result<bool> {
    Ok(run_git(Some(repo_dir), &["rev-parse", "--verify", "HEAD"])?
        .status
        .success())
}

fn git_config_nonempty(repo_dir: &Path, key: &str) -> Result<bool> {
    let out = run_git(Some(repo_dir), &["config", "--get", key])?;
    Ok(out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn read_commit_note(repo_dir: &Path, rev: &str, fallback_main: &str) -> Result<(String, Value)> {
    validate_rev(rev, true)?;
    let meta_spec = format!("{rev}:.simple-note.json");
    let meta_out = run_git(Some(repo_dir), &["show", &meta_spec])?;
    let meta: Value = if meta_out.status.success() {
        serde_json::from_slice(&meta_out.stdout).map_err(fail)?
    } else {
        json!({"version": 1, "images": {}})
    };
    let main_name = meta
        .get("main")
        .and_then(Value::as_str)
        .filter(|name| {
            !name.is_empty()
                && !name.contains(['/', '\\'])
                && *name != ".simple-note.json"
                && !name.ends_with(".simple-note.json")
        })
        .unwrap_or(fallback_main);
    let note_spec = format!("{rev}:{main_name}");
    let note_out = run_git_ok(Some(repo_dir), &["show", &note_spec])?;
    Ok((String::from_utf8_lossy(&note_out.stdout).into_owned(), meta))
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VcsOperation {
    Check,
    Status {
        root: String,
        path: String,
    },
    WorkspaceStatus {
        root: String,
    },
    Commit {
        root: String,
        path: String,
        message: String,
    },
    Diff {
        root: String,
        path: String,
        hash: Option<String>,
        mode: Option<String>,
    },
    Restore {
        root: String,
        path: String,
        hash: String,
    },
    Discard {
        root: String,
        path: String,
    },
}

fn check_git() -> Result<Value> {
    match run_git(None, &["--version"]) {
        Ok(out) if out.status.success() => Ok(json!({
            "available": true,
            "version": String::from_utf8_lossy(&out.stdout).trim(),
        })),
        Ok(_) => Ok(json!({ "available": false, "version": Value::Null })),
        Err(e) if e.code == "git_missing" => {
            Ok(json!({ "available": false, "version": Value::Null }))
        }
        Err(e) => Err(e),
    }
}

fn ensure_git() -> Result<()> {
    let out = run_git(None, &["--version"])?;
    if out.status.success() {
        Ok(())
    } else {
        Err(git_missing())
    }
}

pub(crate) fn status(root: &Path, path: &Path) -> Result<Value> {
    match run_git(None, &["--version"]) {
        Ok(out) if out.status.success() => {}
        Ok(_) => {
            return Ok(json!({
                "available": false,
                "supported": true,
                "initialized": false,
                "dirty": false,
                "commits": []
            }));
        }
        Err(e) if e.code == "git_missing" => {
            return Ok(json!({
                "available": false,
                "supported": true,
                "initialized": false,
                "dirty": false,
                "commits": []
            }));
        }
        Err(e) => return Err(e),
    }
    let (repo_dir, _) = match resolve_note_repo(root, path) {
        Ok(resolved) => resolved,
        Err(e) if e.code == "unsupported_vcs" => {
            return Ok(json!({
                "available": true,
                "supported": false,
                "initialized": false,
                "dirty": false,
                "commits": []
            }));
        }
        Err(e) => return Err(e),
    };
    if !repo_dir.join(".git").is_dir() {
        return Ok(json!({
            "available": true,
            "supported": true,
            "initialized": false,
            "dirty": true,
            "commits": []
        }));
    }
    let has_head = has_git_head(&repo_dir)?;
    let porcelain = run_git_ok(
        Some(&repo_dir),
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    let dirty = !has_head || !String::from_utf8_lossy(&porcelain.stdout).trim().is_empty();
    let mut commits = Vec::new();
    if has_head {
        let log = run_git_ok(
            Some(&repo_dir),
            &["log", "-n", "200", "--format=%H%x1f%h%x1f%ct%x1f%an%x1f%s"],
        )?;
        let raw = String::from_utf8_lossy(&log.stdout);
        for line in raw.lines() {
            let mut parts = line.splitn(5, '\x1f');
            if let (Some(hash), Some(short_hash), Some(ts), Some(author), Some(subject)) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) && let Ok(timestamp) = ts.trim().parse::<i64>()
            {
                commits.push(json!({
                    "hash": hash,
                    "shortHash": short_hash,
                    "timestamp": timestamp,
                    "author": author,
                    "subject": subject,
                }));
            }
        }
    }
    let branch = if has_head {
        run_git(Some(&repo_dir), &["branch", "--show-current"])
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                    if !s.is_empty() {
                        Some(s)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "main".to_owned())
    } else {
        "main".to_owned()
    };
    Ok(json!({
        "available": true,
        "supported": true,
        "initialized": has_head,
        "dirty": dirty,
        "branch": branch,
        "commits": commits
    }))
}

pub(crate) fn commit(root: &Path, path: &Path, message: &str) -> Result<Value> {
    ensure_git()?;
    let (repo_dir, _) = resolve_note_repo(root, path)?;
    if !repo_dir.join(".git").exists() {
        run_git_ok(Some(&repo_dir), &["init"])?;
        run_git_ok(Some(&repo_dir), &["config", "core.autocrlf", "false"])?;
        run_git_ok(Some(&repo_dir), &["config", "core.quotepath", "false"])?;
    }
    let has_head = has_git_head(&repo_dir)?;
    run_git_ok(Some(&repo_dir), &["add", "-A"])?;
    let porcelain = run_git_ok(
        Some(&repo_dir),
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    if has_head && String::from_utf8_lossy(&porcelain.stdout).trim().is_empty() {
        return Err(StoreError {
            code: "nothing_to_commit",
            message: "No changes to commit.".into(),
        });
    }
    let trimmed = message.trim();
    let msg = if trimmed.is_empty() {
        if has_head {
            "Update note"
        } else {
            "Initial commit"
        }
    } else {
        trimmed
    };
    let has_name = git_config_nonempty(&repo_dir, "user.name")?;
    let has_email = git_config_nonempty(&repo_dir, "user.email")?;
    let mut args = vec!["-c", "commit.gpgsign=false"];
    if !has_name {
        args.extend(["-c", "user.name=Sidenote"]);
    }
    if !has_email {
        args.extend(["-c", "user.email=local@sidenote"]);
    }
    args.extend(["commit", "-m", msg]);
    run_git_ok(Some(&repo_dir), &args)?;
    status(root, path)
}

pub(crate) fn diff(
    root: &Path,
    path: &Path,
    hash: Option<&str>,
    mode: Option<&str>,
) -> Result<Value> {
    ensure_git()?;
    let (repo_dir, current_main) = resolve_note_repo(root, path)?;
    match hash {
        None => {
            let has_head = repo_dir.join(".git").is_dir() && has_git_head(&repo_dir)?;
            let (old_text, old_meta) = if has_head {
                read_commit_note(&repo_dir, "HEAD", &current_main)?
            } else {
                (String::new(), json!({"version": 1, "images": {}}))
            };
            let current_doc = crate::files::load(&repo_dir.join(&current_main))?;
            let meta_changed = old_meta.get("images") != current_doc.metadata.get("images");
            let assets_changed = if repo_dir.join(".git").is_dir() {
                let assets_status = run_git_ok(
                    Some(&repo_dir),
                    &[
                        "status",
                        "--porcelain",
                        "--untracked-files=all",
                        "--",
                        "assets",
                    ],
                )?;
                !String::from_utf8_lossy(&assets_status.stdout)
                    .trim()
                    .is_empty()
            } else {
                fs::read_dir(repo_dir.join("assets"))
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(false)
            };
            Ok(json!({
                "oldText": old_text,
                "newText": current_doc.text,
                "isInitial": !has_head,
                "metaChanged": meta_changed,
                "assetsChanged": assets_changed
            }))
        }
        Some(h) if mode == Some("working") => {
            validate_rev(h, false)?;
            let (old_text, old_meta) = read_commit_note(&repo_dir, h, &current_main)?;
            let current_doc = crate::files::load(&repo_dir.join(&current_main))?;
            let meta_changed = old_meta.get("images") != current_doc.metadata.get("images");
            let assets_diff =
                run_git_ok(Some(&repo_dir), &["diff", "--name-only", h, "--", "assets"])?;
            let assets_untracked = run_git_ok(
                Some(&repo_dir),
                &[
                    "status",
                    "--porcelain",
                    "--untracked-files=all",
                    "--",
                    "assets",
                ],
            )?;
            let assets_changed = !String::from_utf8_lossy(&assets_diff.stdout)
                .trim()
                .is_empty()
                || !String::from_utf8_lossy(&assets_untracked.stdout)
                    .trim()
                    .is_empty();
            Ok(json!({
                "oldText": old_text,
                "newText": current_doc.text,
                "isInitial": false,
                "metaChanged": meta_changed,
                "assetsChanged": assets_changed
            }))
        }
        Some(h) => {
            validate_rev(h, false)?;
            let (new_text, new_meta) = read_commit_note(&repo_dir, h, &current_main)?;
            let parent_rev = format!("{h}^");
            let has_parent = run_git(Some(&repo_dir), &["rev-parse", "--verify", &parent_rev])?
                .status
                .success();
            let (old_text, meta_changed, assets_changed) = if has_parent {
                let (old_text, old_meta) = read_commit_note(&repo_dir, &parent_rev, &current_main)?;
                let meta_changed = old_meta.get("images") != new_meta.get("images");
                let assets_diff = run_git_ok(
                    Some(&repo_dir),
                    &["diff", "--name-only", &parent_rev, h, "--", "assets"],
                )?;
                let assets_changed = !String::from_utf8_lossy(&assets_diff.stdout)
                    .trim()
                    .is_empty();
                (old_text, meta_changed, assets_changed)
            } else {
                let meta_changed = !new_meta["images"]
                    .as_object()
                    .map(|m| m.is_empty())
                    .unwrap_or(true);
                let assets_tree = run_git_ok(
                    Some(&repo_dir),
                    &["ls-tree", "-r", "--name-only", h, "--", "assets"],
                )?;
                let assets_changed = !String::from_utf8_lossy(&assets_tree.stdout)
                    .trim()
                    .is_empty();
                (String::new(), meta_changed, assets_changed)
            };
            Ok(json!({
                "oldText": old_text,
                "newText": new_text,
                "isInitial": !has_parent,
                "metaChanged": meta_changed,
                "assetsChanged": assets_changed
            }))
        }
    }
}

pub(crate) fn restore(root: &Path, path: &Path, hash: &str) -> Result<Value> {
    ensure_git()?;
    validate_rev(hash, false)?;
    let (repo_dir, current_main) = resolve_note_repo(root, path)?;
    let (restored_text, mut restored_meta) = read_commit_note(&repo_dir, hash, &current_main)?;
    if let Some(obj) = restored_meta.as_object_mut() {
        obj.insert("main".into(), json!(current_main));
    }
    let assets_dir = repo_dir.join("assets");
    if assets_dir.exists() {
        crate::files::remove_dir_all_force(&assets_dir)?;
    }
    fs::create_dir_all(&assets_dir).map_err(fail)?;
    let assets_spec = format!("{hash}:assets");
    if run_git(Some(&repo_dir), &["rev-parse", "--verify", &assets_spec])?
        .status
        .success()
    {
        run_git_ok(Some(&repo_dir), &["checkout", hash, "--", "assets"])?;
        let _ = run_git(Some(&repo_dir), &["reset", "-q", "HEAD", "--", "assets"]);
    }
    crate::files::atomic(
        &repo_dir.join(".simple-note.json"),
        &serde_json::to_vec_pretty(&restored_meta).map_err(fail)?,
    )?;
    crate::files::replace_existing(&repo_dir.join(&current_main), restored_text.as_bytes())?;
    Ok(json!(crate::files::load(&repo_dir.join(&current_main))?))
}

pub(crate) fn discard(root: &Path, path: &Path) -> Result<Value> {
    ensure_git()?;
    let (repo_dir, _) = resolve_note_repo(root, path)?;
    if !repo_dir.join(".git").exists() || !has_git_head(&repo_dir)? {
        return Err(fail("Note has no committed history to restore"));
    }
    run_git_ok(Some(&repo_dir), &["checkout", "HEAD", "--", "."])?;
    let _ = run_git(Some(&repo_dir), &["clean", "-fd"]);
    status(root, path)
}

pub(crate) fn workspace_status(root: &Path) -> Result<Value> {
    match run_git(None, &["--version"]) {
        Ok(out) if out.status.success() => {}
        Ok(_) => {
            return Ok(json!({
                "available": false,
                "notes": []
            }));
        }
        Err(e) if e.code == "git_missing" => {
            return Ok(json!({
                "available": false,
                "notes": []
            }));
        }
        Err(e) => return Err(e),
    }
    let canonical_root = fs::canonicalize(root).map_err(fail)?;
    if !canonical_root.is_dir() {
        return Ok(json!({
            "available": true,
            "notes": []
        }));
    }
    let mut notes = Vec::new();

    fn scan_dir(dir: &Path, root: &Path, notes: &mut Vec<Value>, depth: usize) {
        if depth > 64 || notes.len() >= 5000 {
            return;
        }
        let Ok(read_dir) = fs::read_dir(dir) else {
            return;
        };
        let mut subdirs = Vec::new();
        for entry in read_dir.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.')
                || name_str == "node_modules"
                || name_str == "target"
                || name_str == "assets"
            {
                continue;
            }
            let p = entry.path();
            if let Some(main_name) = crate::files::package_main(&p) {
                let main_path = p.join(&main_name);
                let relative = main_path
                    .strip_prefix(root)
                    .unwrap_or(&main_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let git_dir = p.join(".git");
                let is_git = git_dir.is_dir()
                    && !fs::symlink_metadata(&git_dir)
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false);
                let (initialized, dirty, branch) = if is_git {
                    let has_head = has_git_head(&p).unwrap_or(false);
                    let porcelain = run_git_ok(
                        Some(&p),
                        &["status", "--porcelain", "--untracked-files=all"],
                    );
                    let is_dirty = match porcelain {
                        Ok(out) => {
                            !has_head || !String::from_utf8_lossy(&out.stdout).trim().is_empty()
                        }
                        Err(_) => true,
                    };
                    let b = if has_head {
                        run_git(Some(&p), &["branch", "--show-current"])
                            .ok()
                            .and_then(|out| {
                                if out.status.success() {
                                    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                                    if !s.is_empty() {
                                        Some(s)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| "main".to_owned())
                    } else {
                        "main".to_owned()
                    };
                    (has_head, is_dirty, b)
                } else {
                    (false, true, "main".to_owned())
                };
                notes.push(json!({
                    "path": crate::files::clean_os_path(&main_path).to_string_lossy(),
                    "relative": relative,
                    "name": main_name,
                    "initialized": initialized,
                    "dirty": dirty,
                    "branch": branch,
                }));
            } else {
                subdirs.push(p);
            }
        }
        for sub in subdirs {
            scan_dir(&sub, root, notes, depth + 1);
        }
    }

    scan_dir(&canonical_root, &canonical_root, &mut notes, 0);

    Ok(json!({
        "available": true,
        "notes": notes,
    }))
}

#[tauri::command]
pub async fn vcs_operation(
    app: tauri::AppHandle,
    _state: State<'_, crate::files::Files>,
    operation: VcsOperation,
) -> Result<Value> {
    tauri::async_runtime::spawn_blocking(move || match operation {
        VcsOperation::Check => check_git(),
        VcsOperation::Status { root, path } => {
            let state = app.state::<crate::files::Files>();
            let _guard = state.0.lock().map_err(fail)?;
            status(Path::new(&root), Path::new(&path))
        }
        VcsOperation::WorkspaceStatus { root } => {
            let state = app.state::<crate::files::Files>();
            let _guard = state.0.lock().map_err(fail)?;
            workspace_status(Path::new(&root))
        }
        VcsOperation::Commit {
            root,
            path,
            message,
        } => {
            let state = app.state::<crate::files::Files>();
            let _guard = state.0.lock().map_err(fail)?;
            commit(Path::new(&root), Path::new(&path), &message)
        }
        VcsOperation::Diff {
            root,
            path,
            hash,
            mode,
        } => {
            let state = app.state::<crate::files::Files>();
            let _guard = state.0.lock().map_err(fail)?;
            diff(
                Path::new(&root),
                Path::new(&path),
                hash.as_deref(),
                mode.as_deref(),
            )
        }
        VcsOperation::Restore { root, path, hash } => {
            let state = app.state::<crate::files::Files>();
            let _guard = state.0.lock().map_err(fail)?;
            restore(Path::new(&root), Path::new(&path), &hash)
        }
        VcsOperation::Discard { root, path } => {
            let state = app.state::<crate::files::Files>();
            let _guard = state.0.lock().map_err(fail)?;
            discard(Path::new(&root), Path::new(&path))
        }
    })
    .await
    .map_err(fail)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::FileStore;

    #[test]
    fn check_and_uninitialized_status_report_git_presence_and_dirty_initial_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let store = FileStore::new(dir.path().join("app")).unwrap();
        let empty = json!({"version":1,"images":{}});
        let doc = store
            .create_in_root(&root, Some("note.md"), "v1", &empty, None, &[], false)
            .unwrap();

        let checked = check_git().unwrap();
        assert_eq!(checked["available"], true);

        let st = status(&root, Path::new(&doc.path)).unwrap();
        assert_eq!(st["available"], true);
        assert_eq!(st["supported"], true);
        assert_eq!(st["initialized"], false);
        assert_eq!(st["dirty"], true);
        assert!(st["commits"].as_array().unwrap().is_empty());

        let plain = root.join("plain.md");
        fs::write(&plain, "plain").unwrap();
        let plain_st = status(&root, &plain).unwrap();
        assert_eq!(plain_st["supported"], false);
    }

    #[test]
    fn commit_defaults_clean_guard_diff_across_rename_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let store = FileStore::new(dir.path().join("app")).unwrap();
        let meta_v1 = json!({"version":1,"images":{"./assets/a.png":{"id":"img1","annotations":[{"kind":"caption","text":"first"}]}}});
        let doc = store
            .create_in_root(
                &root,
                Some("alpha.md"),
                "line 1\nline 2",
                &meta_v1,
                None,
                &[],
                false,
            )
            .unwrap();
        fs::write(root.join("alpha/assets/a.png"), b"png-v1").unwrap();

        let first = commit(&root, Path::new(&doc.path), "   ").unwrap();
        assert_eq!(first["initialized"], true);
        assert_eq!(first["dirty"], false);
        let commits1 = first["commits"].as_array().unwrap();
        assert_eq!(commits1.len(), 1);
        assert_eq!(commits1[0]["subject"], "Initial commit");
        let hash1 = commits1[0]["hash"].as_str().unwrap().to_owned();

        let clean_err = commit(&root, Path::new(&doc.path), "again").unwrap_err();
        assert_eq!(clean_err.code, "nothing_to_commit");

        fs::write(root.join("alpha/alpha.md"), "line 1\nline 2 edited\nline 3").unwrap();
        fs::write(root.join("alpha/assets/b.png"), b"png-v2").unwrap();
        let meta_v2 = json!({"version":1,"main":"alpha.md","images":{"./assets/b.png":{"id":"img2","annotations":[]}}});
        fs::write(
            root.join("alpha/.simple-note.json"),
            serde_json::to_vec_pretty(&meta_v2).unwrap(),
        )
        .unwrap();

        let working_diff = diff(&root, Path::new(&doc.path), None, None).unwrap();
        assert_eq!(working_diff["oldText"], "line 1\nline 2");
        assert_eq!(working_diff["newText"], "line 1\nline 2 edited\nline 3");
        assert_eq!(working_diff["metaChanged"], true);
        assert_eq!(working_diff["assetsChanged"], true);

        let second = commit(&root, Path::new(&doc.path), "").unwrap();
        assert_eq!(second["dirty"], false);
        assert_eq!(second["commits"][0]["subject"], "Update note");

        let renamed = store
            .rename(&root, Path::new(&doc.path), "beta.md")
            .unwrap();
        fs::write(root.join("beta/beta.md"), "custom line").unwrap();
        let third = commit(&root, Path::new(&renamed.path), "Custom checkpoint").unwrap();
        let commits3 = third["commits"].as_array().unwrap();
        assert_eq!(commits3.len(), 3);
        assert_eq!(commits3[0]["subject"], "Custom checkpoint");

        let d2 = diff(
            &root,
            Path::new(&renamed.path),
            Some(&commits3[1]["hash"].as_str().unwrap()),
            None,
        )
        .unwrap();
        assert_eq!(d2["oldText"], "line 1\nline 2");
        assert_eq!(d2["newText"], "line 1\nline 2 edited\nline 3");

        let restored = restore(&root, Path::new(&renamed.path), &hash1).unwrap();
        assert_eq!(restored["text"], "line 1\nline 2");
        assert_eq!(restored["metadata"]["main"], "beta.md");
        assert_eq!(
            restored["metadata"]["images"]["./assets/a.png"]["annotations"][0]["text"],
            "first"
        );
        assert!(root.join("beta/beta.md").exists());
        assert!(root.join("beta/assets/a.png").exists());
        assert!(!root.join("beta/assets/b.png").exists());

        let after_restore = status(&root, Path::new(&renamed.path)).unwrap();
        assert_eq!(after_restore["commits"].as_array().unwrap().len(), 3);
        assert_eq!(after_restore["dirty"], true);
    }

    #[test]
    fn workspace_status_reports_multiple_notes_and_discard_works() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let store = FileStore::new(dir.path().join("app")).unwrap();
        let empty = json!({"version":1,"images":{}});

        let _doc1 = store
            .create_in_root(&root, Some("note1.md"), "v1", &empty, None, &[], false)
            .unwrap();

        let doc2 = store
            .create_in_root(&root, Some("note2.md"), "v2", &empty, None, &[], false)
            .unwrap();
        commit(&root, Path::new(&doc2.path), "Initial note 2").unwrap();

        let doc3 = store
            .create_in_root(&root, Some("note3.md"), "v3", &empty, None, &[], false)
            .unwrap();
        commit(&root, Path::new(&doc3.path), "Initial note 3").unwrap();
        fs::write(&doc3.path, "v3 edited content").unwrap();

        let ws = workspace_status(&root).unwrap();
        assert_eq!(ws["available"], true);
        let notes = ws["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 3);

        let n1 = notes.iter().find(|n| n["name"] == "note1.md").unwrap();
        assert_eq!(n1["initialized"], false);
        assert_eq!(n1["dirty"], true);

        let n2 = notes.iter().find(|n| n["name"] == "note2.md").unwrap();
        assert_eq!(n2["initialized"], true);
        assert_eq!(n2["dirty"], false);

        let n3 = notes.iter().find(|n| n["name"] == "note3.md").unwrap();
        assert_eq!(n3["initialized"], true);
        assert_eq!(n3["dirty"], true);

        // Discard note 3 changes
        discard(&root, Path::new(&doc3.path)).unwrap();
        let after_discard = status(&root, Path::new(&doc3.path)).unwrap();
        assert_eq!(after_discard["dirty"], false);
        assert_eq!(fs::read_to_string(&doc3.path).unwrap(), "v3");
    }
}
