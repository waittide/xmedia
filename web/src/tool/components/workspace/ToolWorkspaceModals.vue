<script setup lang="ts">
import { defineAsyncComponent } from 'vue'
import { useToolScrapeStore } from '../../stores/scrape'
import { useToolWorkspaceStore } from '../../stores/workspace'

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

const workspaceStore = useToolWorkspaceStore()
const scrapeStore = useToolScrapeStore()
</script>

<template>
  <BatchMetadataModal
    v-if="workspaceStore.batchMetadataModalOpen && workspaceStore.effectiveModalRootId"
    :open="workspaceStore.batchMetadataModalOpen"
    :root-id="workspaceStore.effectiveModalRootId"
    :domain="workspaceStore.activeDomain"
    :entries="workspaceStore.selectedEntriesForRename"
    @close="workspaceStore.batchMetadataModalOpen = false"
    @completed="workspaceStore.handleBatchMetadataCompleted"
    @open-history="
      workspaceStore.batchMetadataModalOpen = false;
      workspaceStore.openHistoryModal('metadata')
    "
  />

  <MetadataHistoryModal
    v-if="workspaceStore.historyModalOpen"
    :open="workspaceStore.historyModalOpen"
    :root-id="workspaceStore.effectiveHistoryRootId"
    :initial-tab="workspaceStore.historyInitialTab"
    @close="workspaceStore.historyModalOpen = false"
    @restored="workspaceStore.handleHistoryRestored"
  />

  <BatchRenameModal
    v-if="
      workspaceStore.renameModalOpen &&
      workspaceStore.settings &&
      workspaceStore.effectiveModalRootId
    "
    :open="workspaceStore.renameModalOpen"
    :root-id="workspaceStore.effectiveModalRootId"
    :entries="workspaceStore.renameTargetEntries ?? workspaceStore.selectedEntriesForRename"
    :skipped-organized-count="workspaceStore.renameSkippedOrganizedCount"
    :templates="workspaceStore.settings.rename_templates"
    @close="workspaceStore.closeRenameModal()"
    @renamed="(count, map) => workspaceStore.handleRenamed(count, map)"
    @open-history="
      workspaceStore.closeRenameModal();
      workspaceStore.openHistoryModal('rename')
    "
  />

  <BookContentEditorModal
    v-if="workspaceStore.bookEditorOpen"
    :open="workspaceStore.bookEditorOpen"
    :entry="workspaceStore.bookEditorEntry"
    @close="workspaceStore.bookEditorOpen = false"
    @saved="scrapeStore.handleBookContentSaved"
    @converted="(detail, deleted) => workspaceStore.handleTxtConvertedToEpub(detail, deleted)"
  />

  <BookNormalizeEpubModal
    v-if="workspaceStore.bookNormalizeOpen && workspaceStore.effectiveBookNormalizeRootId"
    :open="workspaceStore.bookNormalizeOpen"
    :root-id="workspaceStore.effectiveBookNormalizeRootId"
    :entries="workspaceStore.bookNormalizeEntries"
    @close="workspaceStore.bookNormalizeOpen = false"
    @completed="workspaceStore.handleBookNormalizeCompleted"
  />

  <ComicPageManagerModal
    v-if="workspaceStore.comicPageManagerOpen"
    :open="workspaceStore.comicPageManagerOpen"
    :entry="workspaceStore.comicPageManagerEntry"
    @close="workspaceStore.comicPageManagerOpen = false"
    @saved="(detail) => scrapeStore.handleDetailUpdated(detail, true)"
  />

  <ComicSeriesToolsModal
    v-if="workspaceStore.comicSeriesToolsOpen"
    :open="workspaceStore.comicSeriesToolsOpen"
    :root-id="workspaceStore.effectiveComicRootId"
    :entries="workspaceStore.selectedComicEntries"
    @close="workspaceStore.comicSeriesToolsOpen = false"
    @completed="workspaceStore.reloadCurrentScope()"
  />

  <MusicToolboxModal
    v-if="workspaceStore.musicToolsOpen"
    :open="workspaceStore.musicToolsOpen"
    :root-id="workspaceStore.effectiveMusicRootId"
    :entries="workspaceStore.musicToolsEntries"
    @close="workspaceStore.musicToolsOpen = false"
    @updated="(detail) => scrapeStore.handleDetailUpdated(detail, true)"
    @completed="workspaceStore.reloadCurrentScope()"
  />

  <VideoToolboxModal
    v-if="workspaceStore.videoToolsOpen"
    :open="workspaceStore.videoToolsOpen"
    :root-id="workspaceStore.effectiveVideoRootId"
    :entries="workspaceStore.videoToolsEntries"
    @close="workspaceStore.videoToolsOpen = false"
    @updated="(detail) => scrapeStore.handleDetailUpdated(detail, true)"
    @completed="workspaceStore.reloadCurrentScope()"
  />

  <AutoScrapeOptionsModal
    v-if="workspaceStore.autoScrapeModalOpen"
    :open="workspaceStore.autoScrapeModalOpen"
    :domain="workspaceStore.activeDomain"
    :entries="workspaceStore.autoScrapeTargetEntries"
    @close="workspaceStore.autoScrapeModalOpen = false"
    @start="(entries, options) => scrapeStore.handleAutoScrapeConfirmed(entries, options)"
    @open-dry-run="(entries, options) => scrapeStore.handleAutoScrapeToDryRun(entries, options)"
    @open-settings="
      workspaceStore.autoScrapeModalOpen = false;
      workspaceStore.openSettingsTab('scrapers')
    "
  />

  <DryRunScrapeModal
    v-if="workspaceStore.dryRunModalOpen"
    :open="workspaceStore.dryRunModalOpen"
    :root-id="workspaceStore.effectiveDryRunRootId"
    :entries="workspaceStore.dryRunTargetEntries ?? workspaceStore.selectedEntriesForRename"
    :options="workspaceStore.dryRunScrapeOptions"
    @close="workspaceStore.closeDryRunModal()"
    @confirm="(entries, options) => scrapeStore.enqueueScrapeFiles(entries, undefined, options)"
    @manual-scrape="scrapeStore.handleDryRunManualScrape"
  />
</template>
