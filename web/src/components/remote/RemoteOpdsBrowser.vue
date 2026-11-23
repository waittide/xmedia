<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowLeft,
  LuBookOpen,
  LuCheck,
  LuChevronRight,
  LuDownload,
  LuFolder,
  LuHome,
  LuLoader2,
  LuPlus,
  LuRefreshCw,
  LuSearch,
  LuX,
} from 'vue-icons-plus/lu'
import {
  appHostApi,
  type OpdsDownloadLinkDto,
  type OpdsEntryDto,
  type OpdsFeedDto,
} from '@/api/appHost'
import MdSelect, { type SelectOption } from '@/components/ui/MdSelect.vue'
import MdSheet from '@/components/ui/MdSheet.vue'
import { useAppClientStore } from '@/stores/appServers'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

const props = defineProps<{
  selectedSourceId: string
  active: boolean
}>()

const emit = defineEmits<{
  'update:selectedSourceId': [id: string]
  'add-source': []
}>()

const { t } = useI18n()
const router = useRouter()
const appClientStore = useAppClientStore()
const snackbarStore = useSnackbarStore()
const uploadStore = useUploadStore()

const opdsFeed = ref<OpdsFeedDto | null>(null)
const opdsLoading = ref(false)
const opdsError = ref('')
const opdsHistory = ref<string[]>([])
const opdsCurrentUrl = ref<string | undefined>(undefined)
const opdsSearchInput = ref('')
const opdsActiveQuery = ref('')
const opdsCoverFailed = ref<Record<string, boolean>>({})
const opdsSelectedFormatIdx = ref<Record<string, number>>({})
const opdsImportingKeys = ref<Record<string, boolean>>({})
const opdsImportedKeys = ref<Record<string, boolean>>({})
const opdsDetailEntry = ref<OpdsEntryDto | null>(null)
let opdsRequestSeq = 0

const opdsSourceOptions = computed<SelectOption[]>(() =>
  appClientStore.opdsSources.map((src) => ({
    value: src.id,
    label: src.name || t('remote.unnamed'),
  })),
)

function getEntrySelectedDownload(entry: OpdsEntryDto): OpdsDownloadLinkDto | null {
  if (entry.downloads.length === 0) return null
  const idx = opdsSelectedFormatIdx.value[entry.id] ?? 0
  return entry.downloads[idx] ?? entry.downloads[0] ?? null
}

function opdsImportKey(sourceId: string, href: string): string {
  return `${sourceId}::${href}`
}

interface OpdsEntryCardView {
  entry: OpdsEntryDto
  selectedDownload: OpdsDownloadLinkDto | null
  importing: boolean
  imported: boolean
}

const opdsEntryCards = computed<OpdsEntryCardView[]>(() => {
  const entries = opdsFeed.value?.entries ?? []
  const sourceId = props.selectedSourceId
  return entries.map((entry) => {
    const selectedDownload = getEntrySelectedDownload(entry)
    const key = selectedDownload ? opdsImportKey(sourceId, selectedDownload.href) : null
    return {
      entry,
      selectedDownload,
      importing: Boolean(key && opdsImportingKeys.value[key]),
      imported: Boolean(key && opdsImportedKeys.value[key]),
    }
  })
})

function buildOpdsFilename(entry: OpdsEntryDto, dl: OpdsDownloadLinkDto): string {
  const safeBase = (entry.title || 'book')
    .replace(/[\\/:*?"<>|]+/g, '_')
    .trim()
    .slice(0, 120)
  const ext = dl.format.replace(/^\.+/, '').toLowerCase()
  return ext ? `${safeBase}.${ext}` : safeBase
}

async function fetchOpdsFeed(
  opts: { url?: string; q?: string; pushHistory?: boolean; sourceId?: string } = {},
): Promise<void> {
  const sourceId = opts.sourceId ?? props.selectedSourceId
  if (!sourceId) {
    opdsFeed.value = null
    return
  }

  if (opts.pushHistory && opdsFeed.value?.url) {
    opdsHistory.value = [...opdsHistory.value, opdsFeed.value.url]
  }

  const seq = ++opdsRequestSeq
  opdsLoading.value = true
  opdsError.value = ''
  try {
    const feed = await appHostApi.browseOpds(sourceId, {
      url: opts.url,
      q: opts.q,
    })
    if (seq !== opdsRequestSeq) return
    opdsFeed.value = feed
    opdsCurrentUrl.value = opts.url ?? feed.url
    opdsCoverFailed.value = {}
  } catch (err) {
    if (seq !== opdsRequestSeq) return
    opdsError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === opdsRequestSeq) {
      opdsLoading.value = false
    }
  }
}

function selectSource(nextId: string): void {
  emit('update:selectedSourceId', nextId)
  opdsHistory.value = []
  opdsCurrentUrl.value = undefined
  opdsSearchInput.value = ''
  opdsActiveQuery.value = ''
  opdsFeed.value = null
  if (nextId) {
    void fetchOpdsFeed({ sourceId: nextId })
  }
}

function resetState(refetch = false, sourceId?: string): void {
  opdsHistory.value = []
  opdsCurrentUrl.value = undefined
  opdsSearchInput.value = ''
  opdsActiveQuery.value = ''
  opdsFeed.value = null
  opdsDetailEntry.value = null
  if (refetch && (sourceId ?? props.selectedSourceId)) {
    void fetchOpdsFeed({ sourceId: sourceId ?? props.selectedSourceId })
  }
}

function closeDetail(): void {
  opdsDetailEntry.value = null
}

function handleOpdsNavClick(href: string): void {
  opdsActiveQuery.value = ''
  void fetchOpdsFeed({ url: href, pushHistory: true })
}

function handleOpdsBack(): void {
  if (opdsHistory.value.length === 0) return
  const nextHistory = opdsHistory.value.slice(0, -1)
  const prevUrl = opdsHistory.value[opdsHistory.value.length - 1]
  opdsHistory.value = nextHistory
  opdsActiveQuery.value = ''
  void fetchOpdsFeed({ url: prevUrl })
}

function handleOpdsRoot(): void {
  opdsHistory.value = []
  opdsCurrentUrl.value = undefined
  opdsSearchInput.value = ''
  opdsActiveQuery.value = ''
  void fetchOpdsFeed()
}

function handleOpdsSearchSubmit(): void {
  const q = opdsSearchInput.value.trim()
  if (!q) {
    if (opdsActiveQuery.value) {
      handleOpdsRoot()
    }
    return
  }
  opdsActiveQuery.value = q
  void fetchOpdsFeed({ q, pushHistory: true })
}

function handleOpdsClearSearch(): void {
  opdsSearchInput.value = ''
  if (opdsActiveQuery.value) {
    handleOpdsRoot()
  }
}

function handleOpdsNextPage(): void {
  const nextUrl = opdsFeed.value?.next_url
  if (!nextUrl) return
  void fetchOpdsFeed({ url: nextUrl, pushHistory: true })
}

async function handleImportOpds(entry: OpdsEntryDto, dl: OpdsDownloadLinkDto): Promise<void> {
  const sourceId = props.selectedSourceId
  if (!sourceId) return
  const key = opdsImportKey(sourceId, dl.href)
  if (opdsImportingKeys.value[key]) return

  opdsImportingKeys.value = { ...opdsImportingKeys.value, [key]: true }
  try {
    const res = await appHostApi.importOpdsEntry(sourceId, {
      url: dl.href,
      filename: buildOpdsFilename(entry, dl),
    })
    opdsImportedKeys.value = { ...opdsImportedKeys.value, [key]: true }
    uploadStore.notifyCatalogChanged(res.media_kind)
    const targetRoute =
      res.media_kind === 'comic'
        ? `/comic/item/${encodeURIComponent(res.imported_id)}`
        : `/book/item/${encodeURIComponent(res.imported_id)}`
    snackbarStore.show({
      message: t('remote.importSuccess', { title: res.title || entry.title }),
      actionLabel: t('remote.viewInLibrary'),
      onAction: () => {
        void router.push(targetRoute)
      },
    })
  } catch (err) {
    snackbarStore.show({
      message: t('remote.importFailed', {
        message: err instanceof Error ? err.message : String(err),
      }),
      tone: 'error',
    })
  } finally {
    const next = { ...opdsImportingKeys.value }
    delete next[key]
    opdsImportingKeys.value = next
  }
}

watch(
  () => props.active,
  (isActive) => {
    if (isActive && props.selectedSourceId && !opdsFeed.value && !opdsLoading.value) {
      void fetchOpdsFeed()
    }
  },
)

onMounted(() => {
  if (props.active && props.selectedSourceId && !opdsFeed.value && !opdsLoading.value) {
    void fetchOpdsFeed()
  }
})

defineExpose({
  selectSource,
  resetState,
  closeDetail,
  hasFeed: computed(() => opdsFeed.value !== null),
})
</script>

<template>
  <div>
    <div
      v-if="appClientStore.opdsSources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] px-6 py-16 text-center"
    >
      <div
        class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuBookOpen class="h-6 w-6" />
      </div>
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
        {{ t('remote.opdsEmptySourcesTitle') }}
      </p>
      <p class="mx-auto mt-1 max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('remote.opdsEmptySourcesHint') }}
      </p>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-filled md-state mt-5"
        @click="emit('add-source')"
      >
        <LuPlus class="h-4 w-4" />
        <span>{{ t('remote.opdsAddBtn') }}</span>
      </button>
    </div>

    <div v-else class="space-y-4">
      <!-- OPDS Toolbar -->
      <div class="flex flex-wrap items-center gap-2.5 lg:flex-nowrap">
        <div class="w-44 shrink-0 sm:w-52">
          <MdSelect
            :model-value="selectedSourceId"
            :options="opdsSourceOptions"
            :placeholder="t('remote.opdsSelectSource')"
            size="sm"
            @update:model-value="selectSource"
          />
        </div>

        <div class="flex shrink-0 items-center gap-1">
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :disabled="opdsHistory.length === 0 || opdsLoading"
            :title="t('remote.opdsBack')"
            :aria-label="t('remote.opdsBack')"
            @click="handleOpdsBack"
          >
            <LuArrowLeft class="h-4 w-4" />
          </button>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :disabled="(opdsHistory.length === 0 && !opdsActiveQuery) || opdsLoading"
            :title="t('remote.opdsRoot')"
            :aria-label="t('remote.opdsRoot')"
            @click="handleOpdsRoot"
          >
            <LuHome class="h-4 w-4" />
          </button>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :disabled="opdsLoading"
            :title="t('common.refresh')"
            :aria-label="t('common.refresh')"
            @click="fetchOpdsFeed({ url: opdsCurrentUrl, q: opdsActiveQuery || undefined })"
          >
            <LuRefreshCw class="h-4 w-4" :class="opdsLoading ? 'animate-spin' : ''" />
          </button>
        </div>

        <div v-if="opdsFeed" class="flex min-w-0 flex-1 items-center gap-2">
          <span class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ opdsFeed.title }}
          </span>
          <span
            v-if="opdsFeed.opds_version"
            class="shrink-0 rounded-full bg-[var(--md-sys-color-secondary-container)] px-2 py-0.5 font-mono text-[10px] font-medium text-[var(--md-sys-color-on-secondary-container)]"
          >
            {{ opdsFeed.opds_version }}
          </span>
        </div>

        <form
          v-if="opdsFeed?.search_url || opdsActiveQuery"
          class="relative ml-auto w-full shrink-0 sm:w-60 lg:w-64"
          @submit.prevent="handleOpdsSearchSubmit"
        >
          <LuSearch
            class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="opdsSearchInput"
            type="text"
            :placeholder="t('remote.opdsSearchPlaceholder')"
            class="md-input h-9 w-full !pr-8 !pl-8 text-xs"
          />
          <button
            v-if="opdsSearchInput || opdsActiveQuery"
            type="button"
            class="md-icon-btn md-icon-btn-sm absolute top-1/2 right-1 -translate-y-1/2 !h-7 !w-7"
            :aria-label="t('common.cancel')"
            @click="handleOpdsClearSearch"
          >
            <LuX class="h-3.5 w-3.5" />
          </button>
        </form>
      </div>

      <!-- Error Banner -->
      <div
        v-if="opdsError"
        class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
      >
        <div class="flex min-w-0 items-center gap-2">
          <LuAlertCircle class="h-4 w-4 shrink-0" />
          <span class="break-all">{{ opdsError }}</span>
        </div>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text shrink-0 !text-[var(--md-sys-color-on-error-container)]"
          @click="fetchOpdsFeed({ url: opdsCurrentUrl, q: opdsActiveQuery || undefined })"
        >
          {{ t('remote.retryBtn') }}
        </button>
      </div>

      <!-- Loading -->
      <div
        v-else-if="opdsLoading && !opdsFeed"
        class="flex items-center justify-center py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
        <span>{{ t('common.loading') }}</span>
      </div>

      <!-- Feed Content -->
      <template v-else-if="opdsFeed">
        <!-- Navigation Chips -->
        <section v-if="opdsFeed.nav_links.length > 0" class="space-y-2.5">
          <h2 class="text-label-medium text-on-surface-variant">
            {{ t('remote.opdsCategories') }}
          </h2>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="link in opdsFeed.nav_links"
              :key="`${link.href}-${link.title}`"
              type="button"
              class="md-state inline-flex cursor-pointer items-center gap-2 rounded-full border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-3.5 py-2 text-xs font-medium text-[var(--md-sys-color-on-surface)] transition hover:border-[var(--md-sys-color-primary)]/50 hover:bg-[var(--md-sys-color-surface-container-high)]"
              @click="handleOpdsNavClick(link.href)"
            >
              <LuFolder class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-primary)]" />
              <span class="max-w-[16rem] truncate">{{ link.title }}</span>
              <span
                v-if="link.count !== null && link.count !== undefined"
                class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-1.5 py-0.5 text-[10px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ link.count }}
              </span>
              <LuChevronRight class="h-3.5 w-3.5 shrink-0 opacity-60" />
            </button>
          </div>
        </section>

        <!-- Entries 2:3 Portrait Cover Grid -->
        <section v-if="opdsFeed.entries.length > 0" class="space-y-3">
          <h2
            v-if="opdsFeed.nav_links.length > 0"
            class="text-label-medium text-on-surface-variant"
          >
            {{ t('remote.opdsBooks') }}
          </h2>

          <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-4">
            <article
              v-for="card in opdsEntryCards"
              :key="card.entry.id"
              class="group flex flex-col"
            >
              <!-- Pure 2:3 Portrait Cover (zero text/button overlays) -->
              <button
                type="button"
                :aria-label="card.entry.title"
                class="book-shadow relative block aspect-[2/3] w-full cursor-pointer overflow-hidden rounded-corner-md bg-surface-container-low text-left outline-none transition duration-200 group-hover:-translate-y-1 focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
                @click="opdsDetailEntry = card.entry"
              >
                <img
                  v-if="card.entry.cover_url && !opdsCoverFailed[card.entry.id]"
                  :src="card.entry.cover_url"
                  :alt="card.entry.title"
                  loading="lazy"
                  decoding="async"
                  class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
                  @error="opdsCoverFailed = { ...opdsCoverFailed, [card.entry.id]: true }"
                />
                <div
                  v-else
                  class="flex h-full w-full flex-col justify-end bg-gradient-to-br from-surface-container-high via-surface-container to-primary-container/35 p-3.5 text-left"
                >
                  <p class="line-clamp-3 text-title-small text-on-surface">
                    {{ card.entry.title }}
                  </p>
                  <p class="mt-1 truncate text-body-small text-on-surface-variant">
                    {{ card.entry.authors.join(', ') || t('remote.opdsUnknownAuthor') }}
                  </p>
                </div>
              </button>

              <!-- Title & Author -->
              <div class="mt-2.5 min-w-0 px-0.5">
                <button
                  type="button"
                  class="block w-full cursor-pointer truncate text-left text-title-small text-on-surface transition-colors group-hover:text-primary"
                  :title="card.entry.title"
                  @click="opdsDetailEntry = card.entry"
                >
                  {{ card.entry.title }}
                </button>
                <p class="mt-0.5 truncate text-body-small text-on-surface-variant">
                  {{ card.entry.authors.join(', ') || t('remote.opdsUnknownAuthor') }}
                </p>
              </div>

              <!-- Import Action & Format Pill (metadata kept below cover) -->
              <div class="mt-2 flex items-center gap-2">
                <template v-if="card.selectedDownload">
                  <button
                    v-if="card.entry.downloads.length > 1"
                    type="button"
                    class="md-state md-touch-target h-9 shrink-0 cursor-pointer rounded-corner-full border border-outline-variant bg-surface-container-low px-2.5 font-mono text-label-small uppercase text-on-surface-variant hover:border-primary hover:text-on-surface"
                    :title="t('remote.opdsFormats')"
                    @click="
                      opdsSelectedFormatIdx = {
                        ...opdsSelectedFormatIdx,
                        [card.entry.id]:
                          ((opdsSelectedFormatIdx[card.entry.id] ?? 0) + 1) %
                          card.entry.downloads.length,
                      }
                    "
                  >
                    {{ card.selectedDownload.format || 'FILE' }}
                  </button>
                  <span
                    v-else
                    class="shrink-0 rounded-corner-sm bg-surface-container-high px-2 py-1 font-mono text-label-small uppercase text-on-surface-variant"
                  >
                    {{ card.selectedDownload.format || 'FILE' }}
                  </span>

                  <button
                    type="button"
                    class="md-btn md-btn-sm md-state h-9 flex-1 px-3 text-label-medium"
                    :class="card.imported ? 'md-btn-outlined' : 'md-btn-tonal'"
                    :disabled="card.importing"
                    @click="handleImportOpds(card.entry, card.selectedDownload)"
                  >
                    <LuLoader2 v-if="card.importing" class="h-3.5 w-3.5 animate-spin" />
                    <LuCheck v-else-if="card.imported" class="h-3.5 w-3.5" />
                    <LuDownload v-else class="h-3.5 w-3.5" />
                    <span>
                      {{
                        card.importing
                          ? t('remote.importingBtn')
                          : card.imported
                            ? t('remote.importedBtn')
                            : t('remote.importBtn')
                      }}
                    </span>
                  </button>
                </template>
                <span
                  v-else
                  class="w-full truncate text-center text-body-small text-on-surface-variant/70"
                >
                  {{ t('remote.opdsNoDownload') }}
                </span>
              </div>
            </article>
          </div>
        </section>

        <!-- Empty Feed -->
        <div
          v-if="opdsFeed.nav_links.length === 0 && opdsFeed.entries.length === 0"
          class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('remote.opdsEmptyFeed') }}
        </div>

        <!-- Next Page -->
        <div v-if="opdsFeed.next_url" class="flex justify-center pt-2">
          <button
            type="button"
            :disabled="opdsLoading"
            class="md-btn md-btn-tonal md-btn-sm"
            @click="handleOpdsNextPage"
          >
            <LuLoader2 v-if="opdsLoading" class="h-3.5 w-3.5 animate-spin" />
            <span>{{ t('remote.opdsNextPage') }}</span>
            <LuChevronRight v-if="!opdsLoading" class="h-4 w-4" />
          </button>
        </div>
      </template>
    </div>

    <!-- OPDS Entry Detail Side Sheet -->
    <MdSheet
      :open="opdsDetailEntry !== null"
      :title="opdsDetailEntry?.title || t('remote.opdsDetailTitle')"
      :subtitle="opdsDetailEntry?.authors.join(', ') || t('remote.opdsUnknownAuthor')"
      size="md"
      @close="opdsDetailEntry = null"
    >
      <template v-if="opdsDetailEntry">
        <div class="space-y-5">
          <div class="flex gap-4">
            <div
              class="book-shadow relative aspect-[2/3] w-28 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container)]"
            >
              <img
                v-if="opdsDetailEntry.cover_url && !opdsCoverFailed[opdsDetailEntry.id]"
                :src="opdsDetailEntry.cover_url"
                :alt="opdsDetailEntry.title"
                class="h-full w-full object-cover"
                @error="opdsCoverFailed = { ...opdsCoverFailed, [opdsDetailEntry.id]: true }"
              />
              <div
                v-else
                class="flex h-full w-full items-center justify-center text-[var(--md-sys-color-on-surface-variant)]/50"
              >
                <LuBookOpen class="h-8 w-8" />
              </div>
            </div>

            <div class="min-w-0 flex-1 space-y-2">
              <p
                v-if="opdsDetailEntry.updated"
                class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ opdsDetailEntry.updated }}
              </p>
              <p
                v-if="opdsDetailEntry.summary"
                class="whitespace-pre-line text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ opdsDetailEntry.summary }}
              </p>
            </div>
          </div>

          <div class="space-y-2">
            <h4 class="text-label-medium text-on-surface-variant">
              {{ t('remote.opdsFormats') }}
            </h4>
            <div
              v-if="opdsDetailEntry.downloads.length === 0"
              class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('remote.opdsNoDownload') }}
            </div>
            <div v-else class="space-y-2">
              <div
                v-for="dl in opdsDetailEntry.downloads"
                :key="dl.href"
                class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-3.5 py-2.5"
              >
                <div class="min-w-0">
                  <div class="flex items-center gap-2">
                    <span
                      class="rounded-md bg-[var(--md-sys-color-primary-container)] px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-primary-container)]"
                    >
                      {{ dl.format || 'FILE' }}
                    </span>
                    <span class="truncate text-xs font-medium text-[var(--md-sys-color-on-surface)]">
                      {{ dl.title || dl.format }}
                    </span>
                  </div>
                  <p class="mt-0.5 truncate font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ dl.media_type }}
                  </p>
                </div>

                <button
                  type="button"
                  class="md-btn md-btn-sm md-btn-filled shrink-0"
                  :disabled="Boolean(opdsImportingKeys[opdsImportKey(selectedSourceId, dl.href)])"
                  @click="handleImportOpds(opdsDetailEntry, dl)"
                >
                  <LuLoader2
                    v-if="opdsImportingKeys[opdsImportKey(selectedSourceId, dl.href)]"
                    class="h-3.5 w-3.5 animate-spin"
                  />
                  <LuCheck
                    v-else-if="opdsImportedKeys[opdsImportKey(selectedSourceId, dl.href)]"
                    class="h-3.5 w-3.5"
                  />
                  <LuDownload v-else class="h-3.5 w-3.5" />
                  <span>
                    {{
                      opdsImportedKeys[opdsImportKey(selectedSourceId, dl.href)]
                        ? t('remote.importedBtn')
                        : t('remote.importBtn')
                    }}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </template>
    </MdSheet>
  </div>
</template>
