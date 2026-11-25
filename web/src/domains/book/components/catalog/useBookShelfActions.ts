import { api } from '@/domains/book/api'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import type { ShelfDto } from '@/domains/book/types'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

export function useBookShelfActions(options?: {
  onShelfDeleted?: (shelfId: string) => void
}) {
  const { t } = useBookI18n()
  const catalogStore = useCatalogStore()
  const dialogStore = useDialogStore()
  const snackbarStore = useSnackbarStore()

  async function createShelf(): Promise<void> {
    const name = await dialogStore.prompt({
      title: t('nav.newShelf'),
      placeholder: t('nav.shelfNamePlaceholder'),
    })
    const trimmed = name?.trim()
    if (!trimmed) return
    try {
      await api.createShelf(trimmed)
      await catalogStore.refreshSidebarData()
      snackbarStore.show(t('collections.shelfCreated', { name: trimmed }))
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  async function renameShelf(shelf: ShelfDto): Promise<void> {
    const nextName = await dialogStore.prompt({
      title: t('collections.renameShelf'),
      message: t('collections.renameShelfPrompt'),
      defaultValue: shelf.name,
    })
    const trimmed = nextName?.trim()
    if (!trimmed || trimmed === shelf.name) return
    try {
      await api.renameShelf(shelf.id, trimmed)
      await catalogStore.refreshSidebarData()
      snackbarStore.show(t('collections.shelfRenamed'))
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  async function deleteShelf(shelf: ShelfDto): Promise<void> {
    const confirmed = await dialogStore.confirm({
      title: t('collections.deleteShelf'),
      message: t('collections.confirmDeleteShelf', { name: shelf.name }),
      danger: true,
    })
    if (!confirmed) return
    try {
      await api.deleteShelf(shelf.id)
      options?.onShelfDeleted?.(shelf.id)
      await catalogStore.refreshSidebarData()
      snackbarStore.show(t('collections.shelfDeleted'))
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  return {
    createShelf,
    renameShelf,
    deleteShelf,
  }
}
