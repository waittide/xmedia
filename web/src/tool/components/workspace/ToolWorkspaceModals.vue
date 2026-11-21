<script setup lang="ts">
import { defineAsyncComponent } from 'vue'
import type { MediaKind } from '@/types/api'
import type {
  AutoScrapeOptionsDto,
  BatchMetadataEditReportDto,
  MediaFileDetail,
  MediaFileEntry,
  NormalizeBooksBatchReportDto,
  PublicSettingsDto,
} from '../../types'

const AutoScrapeOptionsModal = defineAsyncComponent(() => import('../AutoScrapeOptionsModal.vue'))
const BatchMetadataModal = defineAsyncComponent(() => import('../BatchMetadataModal.vue'))
const BatchRenameModal = defineAsyncComponent(() => import('../BatchRenameModal.vue'))
const MetadataHistoryModal = defineAsyncComponent(() => import('../MetadataHistoryModal.vue'))
const BookContentEditorModal = defineAsyncComponent(() => import('../BookContentEditorModal.vue'))
const BookNormalizeEpubModal = defineAsyncComponent(() => import('../BookNormalizeEpubModal.vue'))
const ComicPageManagerModal = defineAsyncComponent(() => import('../ComicPageManagerModal.vue'))
const ComicSeriesToolsModal = defineAsyncComponent(() => import('../ComicSeriesToolsModal.vue'))
const MusicToolboxModal = defineAsyncComponent(() => import('../MusicToolboxModal.vue'))
const DryRunScrapeModal = defineAsyncComponent(() => import('../DryRunScrapeModal.vue'))
const VideoToolboxModal = defineAsyncComponent(() => import('../VideoToolboxModal.vue'))

defineProps<{
  activeDomain: MediaKind
  settings: PublicSettingsDto | null
  selectedEntriesForRename: MediaFileEntry[]
  selectedComicEntries: MediaFileEntry[]
  effectiveModalRootId: string
  effectiveHistoryRootId: string
  effectiveBookNormalizeRootId: string
  effectiveComicRootId: string
  effectiveMusicRootId: string
  effectiveVideoRootId: string
  effectiveDryRunRootId: string
  batchMetadataModalOpen: boolean
  historyModalOpen: boolean
  renameModalOpen: boolean
  renameTargetEntries: MediaFileEntry[] | null
  renameSkippedOrganizedCount: number
  bookEditorOpen: boolean
  bookEditorEntry: MediaFileEntry | null
  bookNormalizeOpen: boolean
  bookNormalizeEntries: MediaFileEntry[]
  comicPageManagerOpen: boolean
  comicPageManagerEntry: MediaFileEntry | null
  comicSeriesToolsOpen: boolean
  musicToolsOpen: boolean
  musicToolsEntries: MediaFileEntry[]
  videoToolsOpen: boolean
  videoToolsEntries: MediaFileEntry[]
  autoScrapeModalOpen: boolean
  autoScrapeTargetEntries: MediaFileEntry[]
  dryRunModalOpen: boolean
  dryRunTargetEntries: MediaFileEntry[] | null
  dryRunScrapeOptions: AutoScrapeOptionsDto | undefined
}>()

const emit = defineEmits<{
  'close-batch-metadata': []
  'batch-metadata-completed': [report: BatchMetadataEditReportDto]
  'open-history-from-batch': []
  'close-history': []
  'history-restored': []
  'close-rename': []
  renamed: [movedCount: number, movedMap: Record<string, string>]
  'close-book-editor': []
  'book-content-saved': [updated: MediaFileDetail]
  'txt-converted': [epubDetail: MediaFileDetail, deletedSourceTxt: boolean]
  'close-book-normalize': []
  'book-normalize-completed': [report: NormalizeBooksBatchReportDto]
  'close-comic-page-manager': []
  'detail-updated-with-art': [detail: MediaFileDetail]
  'close-comic-series-tools': []
  'reload-scope': []
  'close-music-tools': []
  'close-video-tools': []
  'close-auto-scrape': []
  'auto-scrape-confirmed': [entries: MediaFileEntry[], options: AutoScrapeOptionsDto]
  'auto-scrape-to-dry-run': [entries: MediaFileEntry[], options: AutoScrapeOptionsDto]
  'open-scraper-settings': []
  'close-dry-run': []
  'dry-run-confirmed': [entries: MediaFileEntry[], options: AutoScrapeOptionsDto | undefined]
  'dry-run-manual-scrape': [entries: MediaFileEntry[]]
}>()
</script>

<template>
  <BatchMetadataModal
    v-if="batchMetadataModalOpen && effectiveModalRootId"
    :open="batchMetadataModalOpen"
    :root-id="effectiveModalRootId"
    :domain="activeDomain"
    :entries="selectedEntriesForRename"
    @close="emit('close-batch-metadata')"
    @completed="emit('batch-metadata-completed', $event)"
    @open-history="emit('open-history-from-batch')"
  />

  <MetadataHistoryModal
    v-if="historyModalOpen"
    :open="historyModalOpen"
    :root-id="effectiveHistoryRootId"
    @close="emit('close-history')"
    @restored="emit('history-restored')"
  />

  <BatchRenameModal
    v-if="renameModalOpen && settings && effectiveModalRootId"
    :open="renameModalOpen"
    :root-id="effectiveModalRootId"
    :entries="renameTargetEntries ?? selectedEntriesForRename"
    :skipped-organized-count="renameSkippedOrganizedCount"
    :templates="settings.rename_templates"
    @close="emit('close-rename')"
    @renamed="(count, map) => emit('renamed', count, map)"
  />

  <BookContentEditorModal
    v-if="bookEditorOpen"
    :open="bookEditorOpen"
    :entry="bookEditorEntry"
    @close="emit('close-book-editor')"
    @saved="emit('book-content-saved', $event)"
    @converted="(detail, deleted) => emit('txt-converted', detail, deleted)"
  />

  <BookNormalizeEpubModal
    v-if="bookNormalizeOpen && effectiveBookNormalizeRootId"
    :open="bookNormalizeOpen"
    :root-id="effectiveBookNormalizeRootId"
    :entries="bookNormalizeEntries"
    @close="emit('close-book-normalize')"
    @completed="emit('book-normalize-completed', $event)"
  />

  <ComicPageManagerModal
    v-if="comicPageManagerOpen"
    :open="comicPageManagerOpen"
    :entry="comicPageManagerEntry"
    @close="emit('close-comic-page-manager')"
    @saved="emit('detail-updated-with-art', $event)"
  />

  <ComicSeriesToolsModal
    v-if="comicSeriesToolsOpen"
    :open="comicSeriesToolsOpen"
    :root-id="effectiveComicRootId"
    :entries="selectedComicEntries"
    @close="emit('close-comic-series-tools')"
    @completed="emit('reload-scope')"
  />

  <MusicToolboxModal
    v-if="musicToolsOpen"
    :open="musicToolsOpen"
    :root-id="effectiveMusicRootId"
    :entries="musicToolsEntries"
    @close="emit('close-music-tools')"
    @updated="emit('detail-updated-with-art', $event)"
    @completed="emit('reload-scope')"
  />

  <VideoToolboxModal
    v-if="videoToolsOpen"
    :open="videoToolsOpen"
    :root-id="effectiveVideoRootId"
    :entries="videoToolsEntries"
    @close="emit('close-video-tools')"
    @updated="emit('detail-updated-with-art', $event)"
    @completed="emit('reload-scope')"
  />

  <AutoScrapeOptionsModal
    v-if="autoScrapeModalOpen"
    :open="autoScrapeModalOpen"
    :domain="activeDomain"
    :entries="autoScrapeTargetEntries"
    @close="emit('close-auto-scrape')"
    @start="(entries, options) => emit('auto-scrape-confirmed', entries, options)"
    @open-dry-run="(entries, options) => emit('auto-scrape-to-dry-run', entries, options)"
    @open-settings="emit('open-scraper-settings')"
  />

  <DryRunScrapeModal
    v-if="dryRunModalOpen"
    :open="dryRunModalOpen"
    :root-id="effectiveDryRunRootId"
    :entries="dryRunTargetEntries ?? selectedEntriesForRename"
    :options="dryRunScrapeOptions"
    @close="emit('close-dry-run')"
    @confirm="(entries, options) => emit('dry-run-confirmed', entries, options)"
    @manual-scrape="emit('dry-run-manual-scrape', $event)"
  />
</template>
