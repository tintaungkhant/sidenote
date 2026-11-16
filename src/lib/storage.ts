/** Typed IPC contracts only. Rust owns validation, IDs, transactions, and revision conflict checks. */
import { invoke } from '@tauri-apps/api/core';
import type { AttachmentId, NoteDocument } from './editor/document';

export type NoteId = string & { readonly noteId: unique symbol };
export type NoteLanguage = 'plaintext' | 'markdown' | 'javascript' | 'typescript' | 'json' | 'html';
export type StoreError = { code: 'validation' | 'database' | 'filesystem' | 'schema' | 'data' | 'clock' | 'not_found' | 'missing_attachment' | 'conflict' | 'unavailable' | 'recovery'; message: string };
export type NoteInput = { title: string; document: NoteDocument; language: NoteLanguage };
export type NoteSummary = { id: NoteId; title: string; language: NoteLanguage; revision: number; createdAt: number; updatedAt: number; deletedAt: number | null };
export type Note = NoteSummary & { document: NoteDocument };
export type AttachmentInfo = { id: AttachmentId; mimeType: 'image/png' | 'image/jpeg'; width: number; height: number };
export type Attachment = AttachmentInfo & { bytes: number[] };
export type SessionTab = { noteId: NoteId; pinned: boolean; anchor: number; head: number; scrollTop: number };
export type Session = { version: 1; tabs: SessionTab[]; activeNoteId: NoteId | null; sidebarVisible: boolean; wordWrap: boolean };
export type SavedSession = { revision: number; state: Session | null };

/** Create a revision-one note; rejects invalid documents or missing image originals. */
export function createNote(input: NoteInput): Promise<Note> { return invoke('create_note', { input }); }
/** Fetch one document on demand rather than loading all note bodies at startup. */
export function loadNote(id: NoteId): Promise<Note> { return invoke('load_note', { id }); }
/** Replace a note atomically; conflict errors leave the newer durable version intact. */
export function updateNote(id: NoteId, expectedRevision: number, input: NoteInput): Promise<Note> { return invoke('update_note', { id, expectedRevision, input }); }
/** Fetch a bounded metadata page without document JSON or attachment bytes. */
export function listNotes(limit = 100, offset = 0): Promise<NoteSummary[]> { return invoke('list_notes', { limit, offset }); }
/** Store a bounded PNG/JPEG original before referencing its returned ID in a note. */
export function putAttachment(bytes: Uint8Array): Promise<AttachmentInfo> { return invoke('put_attachment', { bytes: Array.from(bytes) }); }
/** Read bytes through a note's durable attachment reference, never through filesystem paths. */
export function loadAttachment(noteId: NoteId, attachmentId: AttachmentId): Promise<Attachment> { return invoke('load_attachment', { noteId, attachmentId }); }
/** Load versioned tab/view state independently of note contents; absent state has revision zero. */
export function loadSession(): Promise<SavedSession> { return invoke('load_session'); }
/** Commit session state only against its last acknowledged revision. */
export function saveSession(expectedRevision: number, session: Session): Promise<SavedSession> { return invoke('save_session', { expectedRevision, session }); }

/** Ask native code to start the shared close/quit handshake. */
export function requestExit(): Promise<void> { return invoke('request_exit'); }
/** Replay any close request that arrived before the renderer installed its listener. */
export function exitReady(): Promise<boolean> { return invoke('exit_ready'); }
/** Reset a failed close attempt without discarding in-memory edits. */
export function cancelExit(): Promise<void> { return invoke('cancel_exit'); }
/** Authorize exit after a full flush, or after the user explicitly confirms discarding unsaved work. */
export function finishExit(discard: boolean): Promise<void> { return invoke('finish_exit', { discard }); }
/** Export current source and available image originals through a native save dialog; the result reports cancellation and unavailable originals. */
export function exportRecovery(snapshot: string): Promise<{ exported: boolean; missingOriginals: number }> { return invoke('export_recovery', { snapshot }); }

/** Read native screenshot pixels and store the original before exposing its identity to the editor. */
export function pasteImage(): Promise<Attachment | null> { return invoke("paste_image"); }
