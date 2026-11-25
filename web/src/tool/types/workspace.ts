import type { MediaKind } from '@/types/api'

export interface CompletenessFlags {
  has_title: boolean
  has_creator: boolean
  has_cover: boolean
  has_summary_or_lyrics: boolean
  has_nfo_or_tag_block: boolean
  is_normalized?: boolean
  is_organized?: boolean
}

export interface FolderEntryDto {
  name: string
  rel_path: string
  child_dir_count: number
  media_file_count: number
}

export interface MediaFileEntry {
  root_id: string
  rel_path: string
  file_name: string
  extension: string
  media_kind: MediaKind
  file_size: number
  modified_unix_secs: number
  display_title: string
  display_subtitle: string
  year: number | null
  completeness: CompletenessFlags
}

export interface RawFileEntryDto {
  rel_path: string
  file_name: string
  extension: string
  file_size: number
  modified_unix_secs: number
  is_media: boolean
  media_kind: MediaKind | null
}

export interface DirectorySnapshot {
  root_id: string
  root_name: string
  current_rel_path: string
  parent_rel_path: string | null
  folders: FolderEntryDto[]
  files: MediaFileEntry[]
  raw_files?: RawFileEntryDto[]
}

export type RenameConflictPolicy = 'skip' | 'auto_suffix' | 'overwrite'

export interface RenamePlanItem {
  from_rel_path: string
  to_rel_path: string
  companion_moves: [string, string][]
  conflict: string | null
  unchanged: boolean
  will_overwrite?: boolean
}

export interface RenameMoveRecord {
  from_rel_path: string
  to_rel_path: string
  companion_moves: [string, string][]
}

export interface RenameHistoryBatchDto {
  batch_id: string
  root_id: string
  created_at_unix: number
  moved_count: number
  moves: RenameMoveRecord[]
}

export interface UndoRenameReportDto {
  batch_id: string
  restored_count: number
  skipped_conflicts: string[]
}

export interface DeleteFileErrorDto {
  rel_path: string
  error: string
}

export interface DeleteFilesReportDto {
  deleted_count: number
  deleted_paths: string[]
  deleted_companions: string[]
  failed: DeleteFileErrorDto[]
}
