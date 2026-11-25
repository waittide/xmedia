import { booksApi, buildEpubAssetUrl } from './api/books'
import type { StreamProgressCallback, StreamProgressPayload } from './api/client'
import { comicsApi } from './api/comics'
import { buildArtworkUrl, inspectorApi, type ArtworkSlot } from './api/inspector'
import { musicApi } from './api/music'
import { resolveScraperCoverUrl, scrapeApi } from './api/scrape'
import { videoApi } from './api/video'
import { workspaceApi } from './api/workspace'

export type { ArtworkSlot, StreamProgressCallback, StreamProgressPayload }
export {
  booksApi,
  buildArtworkUrl,
  buildEpubAssetUrl,
  comicsApi,
  inspectorApi,
  musicApi,
  resolveScraperCoverUrl,
  scrapeApi,
  videoApi,
  workspaceApi,
}

export const toolApi = {
  ...workspaceApi,
  ...inspectorApi,
  ...scrapeApi,
  ...booksApi,
  ...comicsApi,
  ...musicApi,
  ...videoApi,
}
