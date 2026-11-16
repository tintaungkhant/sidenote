# Tauri + SvelteKit + TypeScript

This template should help get you started developing with Tauri, SvelteKit and TypeScript in Vite.

## Backup and restore

Open **Backup and restore** in the app to export a complete `.sqlite3` workspace backup or select one to restore. Restore validates the file and asks for confirmation before replacing local notes, Trash, images, annotations, and tabs. A pre-restore copy remains in the backup directory shown in the app. Emergency recovery JSON is a separate source export and cannot be restored as a SQLite backup.

The app attempts a local snapshot after the first successful write of each UTC day and retains seven valid daily snapshots. Backup errors are reported separately from note saves. If startup cannot read the database, use **Recover from a backup…**; the unreadable originals remain preserved.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
