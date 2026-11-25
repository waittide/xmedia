import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import type { MediaFileDetail, MediaMetadataPatchEnvelope } from '../types'

export interface SaveAndOrganizeEntryInput {
  rootId: string
  relPath: string
  mediaKind: MediaKind
  envelope: MediaMetadataPatchEnvelope
  pendingCoverUrl?: string | null
  pendingBackdropUrl?: string | null
  writeAlbumCover?: boolean
  organizeAfterSave?: boolean
}

export interface SaveAndOrganizeEntryResult {
  detail: MediaFileDetail
  bumpedArtwork: boolean
  artworkWarnings: string[]
  organized: boolean
}

export async function saveAndOrganizeEntry(
  input: SaveAndOrganizeEntryInput,
): Promise<SaveAndOrganizeEntryResult> {
  let updated = await toolApi.writeMetadata(input.rootId, input.relPath, input.envelope)
  let bumpedArtwork = false
  let organized = false
  const artworkWarnings: string[] = []

  if (input.pendingCoverUrl) {
    try {
      const slot = updated.entry.media_kind === 'video' ? 'poster' : 'cover'
      updated = await toolApi.uploadArtworkFromUrl(
        updated.entry.root_id,
        updated.entry.rel_path,
        input.pendingCoverUrl,
        slot,
        updated.entry.media_kind,
      )
      bumpedArtwork = true

      if (updated.entry.media_kind === 'music' && input.writeAlbumCover) {
        try {
          updated = await toolApi.uploadArtworkFromUrl(
            updated.entry.root_id,
            updated.entry.rel_path,
            input.pendingCoverUrl,
            'album_cover',
            'music',
          )
        } catch {
          // Non-fatal sidecar album cover failure
        }
      }
    } catch (artErr) {
      artworkWarnings.push(
        `封面下载写入失败：${artErr instanceof Error ? artErr.message : String(artErr)}`,
      )
    }
  }

  if (input.pendingBackdropUrl && updated.entry.media_kind === 'video') {
    try {
      updated = await toolApi.uploadArtworkFromUrl(
        updated.entry.root_id,
        updated.entry.rel_path,
        input.pendingBackdropUrl,
        'fanart',
        'video',
      )
      bumpedArtwork = true
    } catch (artErr) {
      artworkWarnings.push(
        `背景图下载写入失败：${artErr instanceof Error ? artErr.message : String(artErr)}`,
      )
    }
  }

  if (input.organizeAfterSave) {
    const plan = await toolApi.previewRename(
      updated.entry.root_id,
      [updated.entry.rel_path],
      null,
      updated.entry.media_kind,
    )
    const actionable = plan.find((p) => !p.unchanged && !p.conflict)
    if (actionable) {
      await toolApi.executeRename(updated.entry.root_id, [actionable])
      updated = await toolApi.inspectFile(
        updated.entry.root_id,
        actionable.to_rel_path,
        updated.entry.media_kind,
      )
      organized = true
    }
  }

  return {
    detail: updated,
    bumpedArtwork,
    artworkWarnings,
    organized,
  }
}
