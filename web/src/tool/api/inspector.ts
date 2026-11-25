import type { MediaKind } from '@/types/api'
import type {
  BatchMetadataEditInputDto,
  BatchMetadataEditReportDto,
  MediaFileDetail,
  MediaMetadataPatchEnvelope,
  MetadataHistoryBatchDto,
  UndoMetadataReportDto,
} from '../types'
import { toolRequest } from './client'

export type ArtworkSlot =
  | 'cover'
  | 'series_cover'
  | 'album_cover'
  | 'poster'
  | 'fanart'
  | 'thumb'
  | 'clearlogo'

export function buildArtworkUrl(
  rootId: string,
  relPath: string,
  kind: MediaKind,
  cacheBust = 0,
  slot?: string,
): string {
  const params = new URLSearchParams({
    root_id: rootId,
    path: relPath,
    kind,
    v: String(cacheBust),
  })
  if (slot) {
    params.set('slot', slot)
  }
  return `/api/v1/tool/files/artwork?${params.toString()}`
}

export const inspectorApi = {
  inspectFile(rootId: string, relPath: string, kind: MediaKind | null): Promise<MediaFileDetail> {
    const q = new URLSearchParams({ root_id: rootId, path: relPath })
    if (kind) q.set('kind', kind)
    return toolRequest<MediaFileDetail>(`/files/inspect?${q.toString()}`)
  },

  writeMetadata(
    rootId: string,
    relPath: string,
    envelope: MediaMetadataPatchEnvelope,
  ): Promise<MediaFileDetail> {
    return toolRequest<MediaFileDetail>('/files/metadata', {
      method: 'PATCH',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        ...envelope,
      }),
    })
  },

  batchEditMetadata(
    rootId: string,
    input: BatchMetadataEditInputDto,
  ): Promise<BatchMetadataEditReportDto> {
    return toolRequest<BatchMetadataEditReportDto>('/files/metadata/batch', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        ...input,
      }),
    })
  },

  listMetadataHistory(rootId?: string): Promise<MetadataHistoryBatchDto[]> {
    const qs = rootId ? `?root_id=${encodeURIComponent(rootId)}` : ''
    return toolRequest<MetadataHistoryBatchDto[]>(`/files/metadata/history${qs}`)
  },

  undoMetadataBatch(batchId: string): Promise<UndoMetadataReportDto> {
    return toolRequest<UndoMetadataReportDto>('/files/metadata/undo', {
      method: 'POST',
      body: JSON.stringify({ batch_id: batchId }),
    })
  },

  uploadArtwork(
    rootId: string,
    relPath: string,
    file: File,
    slot: ArtworkSlot,
    kind: MediaKind,
  ): Promise<MediaFileDetail> {
    const fd = new FormData()
    fd.append('root_id', rootId)
    fd.append('path', relPath)
    fd.append('slot', slot)
    fd.append('kind', kind)
    fd.append('file', file)
    return toolRequest<MediaFileDetail>('/files/artwork', {
      method: 'POST',
      body: fd,
    })
  },

  uploadArtworkFromUrl(
    rootId: string,
    relPath: string,
    url: string,
    slot: ArtworkSlot,
    kind: MediaKind,
  ): Promise<MediaFileDetail> {
    return toolRequest<MediaFileDetail>('/files/artwork/from-url', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        url,
        slot,
        kind,
      }),
    })
  },
}
