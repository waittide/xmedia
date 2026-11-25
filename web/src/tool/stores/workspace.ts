import { defineStore, storeToRefs } from 'pinia'
import { useToolDirectoryStore } from './directory'
import { useToolModalsStore } from './modals'
import { useToolNavigationStore } from './navigation'
import { useToolSelectionStore } from './selection'

export const useToolWorkspaceStore = defineStore('toolWorkspace', () => {
  const directoryStore = useToolDirectoryStore()
  const selectionStore = useToolSelectionStore()
  const modalsStore = useToolModalsStore()
  const navigationStore = useToolNavigationStore()

  return {
    ...storeToRefs(directoryStore),
    ...storeToRefs(selectionStore),
    ...storeToRefs(modalsStore),

    entrySelectionKey: directoryStore.entrySelectionKey,
    parseSelectionKey: directoryStore.parseSelectionKey,
    resolveModalRootId: directoryStore.resolveModalRootId,
    loadAllDomainRoots: directoryStore.loadAllDomainRoots,
    loadDirectory: directoryStore.loadDirectory,
    reloadCurrentScope: directoryStore.reloadCurrentScope,
    activateDomainWorkspace: directoryStore.activateDomainWorkspace,
    triggerManualRescan: directoryStore.triggerManualRescan,
    handleSettingsUpdated: directoryStore.handleSettingsUpdated,

    syncHashFromState: navigationStore.syncHashFromState,
    inspectEntry: navigationStore.inspectEntry,
    closeInspectorAndSync: navigationStore.closeInspectorAndSync,
    toggleSidebar: navigationStore.toggleSidebar,
    loadInitial: navigationStore.loadInitial,
    navigatePrimary: navigationStore.navigatePrimary,
    switchDomain: navigationStore.switchDomain,
    selectRoot: navigationStore.selectRoot,
    enterFolder: navigationStore.enterFolder,
    handleHashChange: navigationStore.handleHashChange,
    openSettingsTab: navigationStore.openSettingsTab,
    exitSettings: navigationStore.exitSettings,

    setViewMode: selectionStore.setViewMode,
    setSortField: selectionStore.setSortField,
    toggleSortDir: selectionStore.toggleSortDir,
    selectCompletenessFilter: selectionStore.selectCompletenessFilter,
    toggleSelectFile: selectionStore.toggleSelectFile,
    toggleSelectAllFiltered: selectionStore.toggleSelectAllFiltered,
    toggleSelectAllRaw: selectionStore.toggleSelectAllRaw,
    clearSelection: selectionStore.clearSelection,

    requestDeleteFiles: modalsStore.requestDeleteFiles,
    openHistoryModal: modalsStore.openHistoryModal,
    openRenameModal: modalsStore.openRenameModal,
    closeRenameModal: modalsStore.closeRenameModal,
    openBatchMetadataModal: modalsStore.openBatchMetadataModal,
    openDryRunModal: modalsStore.openDryRunModal,
    closeDryRunModal: modalsStore.closeDryRunModal,
    openComicSeriesToolsModal: modalsStore.openComicSeriesToolsModal,
    closeComicSeriesToolsModal: modalsStore.closeComicSeriesToolsModal,
    openBookContentEditor: modalsStore.openBookContentEditor,
    openComicPageManager: modalsStore.openComicPageManager,
    openMusicToolbox: modalsStore.openMusicToolbox,
    openVideoToolbox: modalsStore.openVideoToolbox,
    openBookNormalizeModal: modalsStore.openBookNormalizeModal,
    handleRenamed: modalsStore.handleRenamed,
    handleHistoryRestored: modalsStore.handleHistoryRestored,
    handleBatchMetadataCompleted: modalsStore.handleBatchMetadataCompleted,
    handleTxtConvertedToEpub: modalsStore.handleTxtConvertedToEpub,
    handleBookNormalizeCompleted: modalsStore.handleBookNormalizeCompleted,
  }
})
