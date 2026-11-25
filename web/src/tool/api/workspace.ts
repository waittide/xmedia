import type { MediaKind } from '@/types/api'
import type {
  DeleteFilesReportDto,
  DirectorySnapshot,
  InboxScanReportDto,
  ManagedRootDto,
  PublicSettingsDto,
  RenameConflictPolicy,
  RenameHistoryBatchDto,
  RenamePlanItem,
  ToolStatusDto,
  UndoRenameReportDto,
  UpdateToolSettingsInput,
  WebhookTestReportDto,
} from '../types'
import {
  clearLegacyToolToken,
  toolRequest,
  toolStreamRequest,
  type StreamProgressCallback,
  type ToolRequestError,
} from './client'

export const workspaceApi = {
  getStatus(): Promise<ToolStatusDto> {
    return toolRequest<ToolStatusDto>('/status')
  },

  async unlock(password: string): Promise<string> {
    const res = await toolRequest<{ token: string }>('/auth/unlock', {
      method: 'POST',
      body: JSON.stringify({ password }),
    })
    clearLegacyToolToken()
    return res.token
  },

  async lock(): Promise<void> {
    clearLegacyToolToken()
    await toolRequest<{ ok: boolean }>('/auth/lock', { method: 'POST' })
  },

  getSettings(): Promise<PublicSettingsDto> {
    return toolRequest<PublicSettingsDto>('/settings')
  },

  updateSettings(input: UpdateToolSettingsInput): Promise<PublicSettingsDto> {
    return toolRequest<PublicSettingsDto>('/settings', {
      method: 'PATCH',
      body: JSON.stringify(input),
    })
  },

  addRoot(
    name: string,
    path: string,
    defaultKind: MediaKind | null,
    readOnly = false,
  ): Promise<ManagedRootDto> {
    return toolRequest<ManagedRootDto>('/roots', {
      method: 'POST',
      body: JSON.stringify({ name, path, default_kind: defaultKind, read_only: readOnly }),
    })
  },

  setRootReadOnly(id: string, readOnly: boolean): Promise<ManagedRootDto> {
    return toolRequest<ManagedRootDto>(`/roots/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify({ read_only: readOnly }),
    })
  },

  removeRoot(id: string): Promise<void> {
    return toolRequest<void>(`/roots/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    })
  },

  scanInbox(): Promise<InboxScanReportDto> {
    return toolRequest<InboxScanReportDto>('/inbox/scan', { method: 'POST' })
  },

  testWebhook(): Promise<WebhookTestReportDto> {
    return toolRequest<WebhookTestReportDto>('/settings/test-webhook', { method: 'POST' })
  },

  triggerWebhookRescan(rootId?: string): Promise<WebhookTestReportDto> {
    return toolRequest<WebhookTestReportDto>('/settings/trigger-webhook', {
      method: 'POST',
      body: JSON.stringify(rootId ? { root_id: rootId } : {}),
    })
  },

  browse(
    rootId: string,
    relPath: string,
    kind: MediaKind | null,
    recursive = true,
    onProgress?: StreamProgressCallback,
    signal?: AbortSignal,
  ): Promise<DirectorySnapshot> {
    const q = new URLSearchParams({
      root_id: rootId,
      path: relPath,
      recursive: recursive ? 'true' : 'false',
    })
    if (kind) q.set('kind', kind)
    if (onProgress) {
      return toolStreamRequest<DirectorySnapshot>(
        `/fs/browse/stream?${q.toString()}`,
        { method: 'GET', signal },
        onProgress,
      ).catch((err: ToolRequestError) => {
        if (err?.streamRouteMissing) {
          return toolRequest<DirectorySnapshot>(`/fs/browse?${q.toString()}`, { signal })
        }
        throw err
      })
    }
    return toolRequest<DirectorySnapshot>(`/fs/browse?${q.toString()}`, { signal })
  },

  previewRename(
    rootId: string,
    paths: string[],
    template: string | null,
    kind: MediaKind | null,
    episodeTemplate?: string | null,
    onProgress?: StreamProgressCallback,
    conflictPolicy: RenameConflictPolicy = 'skip',
  ): Promise<RenamePlanItem[]> {
    const body = JSON.stringify({
      root_id: rootId,
      paths,
      template: template || undefined,
      episode_template: episodeTemplate || undefined,
      kind: kind || undefined,
      conflict_policy: conflictPolicy,
    })
    if (onProgress) {
      return toolStreamRequest<RenamePlanItem[]>(
        '/files/rename/preview/stream',
        { method: 'POST', body },
        onProgress,
      ).catch((err: ToolRequestError) => {
        if (err?.streamRouteMissing) {
          return toolRequest<RenamePlanItem[]>('/files/rename/preview', {
            method: 'POST',
            body,
          })
        }
        throw err
      })
    }
    return toolRequest<RenamePlanItem[]>('/files/rename/preview', {
      method: 'POST',
      body,
    })
  },

  executeRename(
    rootId: string,
    plans: RenamePlanItem[],
    onProgress?: StreamProgressCallback,
  ): Promise<{ moved_count: number }> {
    const body = JSON.stringify({
      root_id: rootId,
      plans,
    })
    if (onProgress) {
      return toolStreamRequest<{ moved_count: number }>(
        '/files/rename/execute/stream',
        { method: 'POST', body },
        onProgress,
      ).catch((err: ToolRequestError) => {
        if (err?.streamRouteMissing) {
          return toolRequest<{ moved_count: number }>('/files/rename/execute', {
            method: 'POST',
            body,
          })
        }
        throw err
      })
    }
    return toolRequest<{ moved_count: number }>('/files/rename/execute', {
      method: 'POST',
      body,
    })
  },

  listRenameHistory(rootId?: string | null): Promise<RenameHistoryBatchDto[]> {
    const q = rootId ? `?root_id=${encodeURIComponent(rootId)}` : ''
    return toolRequest<RenameHistoryBatchDto[]>(`/files/rename/history${q}`)
  },

  undoRenameBatch(batchId: string): Promise<UndoRenameReportDto> {
    return toolRequest<UndoRenameReportDto>('/files/rename/undo', {
      method: 'POST',
      body: JSON.stringify({ batch_id: batchId }),
    })
  },

  deleteFiles(
    rootId: string,
    paths: string[],
    deleteCompanions: boolean,
  ): Promise<DeleteFilesReportDto> {
    return toolRequest<DeleteFilesReportDto>('/files/delete', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        paths,
        delete_companions: deleteCompanions,
      }),
    })
  },
}
