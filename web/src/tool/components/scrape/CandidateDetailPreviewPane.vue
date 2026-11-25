<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuImage,
  LuImageOff,
  LuLayers,
  LuLoader2,
  LuRotateCcw,
  LuSave,
  LuSkipForward,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import type { MediaKind } from '@/types/api'
import { resolveScraperCoverUrl } from '../../api'
import type { FieldSourceBadge, ScrapedMetadata } from '../../types'
import InspectorBookForm from '../inspector/InspectorBookForm.vue'
import InspectorComicForm from '../inspector/InspectorComicForm.vue'
import InspectorMusicForm from '../inspector/InspectorMusicForm.vue'
import InspectorVideoForm from '../inspector/InspectorVideoForm.vue'
import { useMetadataForm, type ApplyFillMode } from '../inspector/useMetadataForm'

export interface CandidatePreviewState {
  key: string
  isMerged: boolean
  sourceLabel: string
  meta: ScrapedMetadata | null
  loading: boolean
  fieldSources: Record<string, FieldSourceBadge> | null
  fillMode: ApplyFillMode
}

export interface CandidateApplyOptions {
  organizeAfterSave?: boolean
  writeAlbumCover?: boolean
}

const props = withDefaults(
  defineProps<{
    mediaKind: MediaKind
    preview: CandidatePreviewState
    hasNext?: boolean
    closable?: boolean
    saving?: boolean
  }>(),
  {
    hasNext: false,
    closable: true,
    saving: false,
  },
)

const emit = defineEmits<{
  directApply: [meta: ScrapedMetadata, mode: ApplyFillMode, opts?: CandidateApplyOptions]
  directApplyNext: [meta: ScrapedMetadata, mode: ApplyFillMode, opts?: CandidateApplyOptions]
  close: []
}>()

const { t } = useI18n()
const candidateForm = useMetadataForm()
const brokenCoverUrls = ref<Set<string>>(new Set())
const organizeAfterSave = ref(false)
const writeAlbumCover = ref(false)
const useCover = ref(true)
const baselineUseCover = ref(true)

function hydrateDraft(meta: ScrapedMetadata | null): void {
  candidateForm.hydrateFromScrapedMetadata(props.mediaKind, meta)
  const hasCover = Boolean(meta?.cover_url)
  useCover.value = hasCover
  baselineUseCover.value = hasCover
}

watch(
  () => [props.preview.key, props.preview.meta, props.preview.loading, props.mediaKind] as const,
  ([, meta]) => {
    hydrateDraft(meta)
  },
  { immediate: true },
)

const isDraftEdited = computed(
  () => candidateForm.isDirty.value || useCover.value !== baselineUseCover.value,
)

function resetDraft(): void {
  hydrateDraft(props.preview.meta)
}

function isCoverUsable(url: string | null | undefined): boolean {
  const trimmed = url?.trim()
  return Boolean(trimmed && !brokenCoverUrls.value.has(trimmed))
}

function markCoverBroken(url: string | null | undefined): void {
  const trimmed = url?.trim()
  if (!trimmed || brokenCoverUrls.value.has(trimmed)) return
  const next = new Set(brokenCoverUrls.value)
  next.add(trimmed)
  brokenCoverUrls.value = next
}

function sourceBadgeFor(...keys: string[]): string | null {
  const map = props.preview.fieldSources
  if (!map) return null
  for (const k of keys) {
    const hit = map[k]
    if (hit?.provider_name) return hit.provider_name
  }
  return null
}

const sourceFieldLabels = computed<Record<string, string>>(() => ({
  title: t('tool.sort.title'),
  original_title: t('tool.workbench.local.originalTitle'),
  series_or_album: t('tool.forms.comic.series'),
  creators: t('tool.forms.music.artist'),
  illustrators_or_directors: t('tool.forms.video.directors'),
  cast: t('tool.workbench.local.cast'),
  publisher_or_studio: t('tool.forms.common.publisher'),
  year: t('tool.forms.common.year'),
  published_date: t('tool.forms.book.publishedAt'),
  isbn: t('tool.forms.book.isbn'),
  rating: t('tool.forms.video.rating'),
  language: t('tool.forms.common.language'),
  disc_or_season_number: t('tool.forms.video.season'),
  track_or_episode_number: t('tool.forms.video.episode'),
  genres: t('tool.forms.common.genres'),
  tags: t('tool.forms.common.tags'),
  summary: t('tool.forms.common.summary'),
  lyrics: t('tool.workbench.candidates.targetLyrics'),
  cover_url: t('tool.modals.comicPages.coverBadge'),
}))

const sourceBadgesList = computed(() => {
  const map = props.preview.fieldSources
  if (!map) return []
  return Object.entries(map)
    .filter(([, badge]) => Boolean(badge?.provider_name))
    .map(([key, badge]) => ({
      key,
      label: sourceFieldLabels.value[key] ?? key,
      provider: badge.provider_name,
    }))
})

const displayDraftTitle = computed(() => {
  if (props.mediaKind === 'book') return candidateForm.bookForm.title
  if (props.mediaKind === 'comic') return candidateForm.comicForm.title
  if (props.mediaKind === 'music') return candidateForm.musicForm.title
  return candidateForm.videoForm.title
})

const displayDraftCreators = computed(() => {
  if (props.mediaKind === 'book') return candidateForm.bookForm.authors
  if (props.mediaKind === 'comic') return candidateForm.comicForm.writers
  if (props.mediaKind === 'music') return candidateForm.musicForm.artist
  return candidateForm.videoForm.directors || candidateForm.videoForm.actors
})

function buildEditedMetadata(): ScrapedMetadata | null {
  const base = props.preview.meta
  if (!base) return null
  const includeCover = useCover.value && isCoverUsable(base.cover_url)
  return candidateForm.toScrapedMetadata(props.mediaKind, base, includeCover)
}

function emitDirectApply(advanceNext = false): void {
  const edited = buildEditedMetadata()
  if (!edited) return
  const opts: CandidateApplyOptions = {
    organizeAfterSave: organizeAfterSave.value,
    writeAlbumCover: writeAlbumCover.value,
  }
  if (advanceNext) {
    emit('directApplyNext', edited, props.preview.fillMode, opts)
  } else {
    emit('directApply', edited, props.preview.fillMode, opts)
  }
}

defineExpose({
  triggerSave: emitDirectApply,
  isEdited: isDraftEdited,
})
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-surface-container-low">
    <!-- Preview / Editable Draft Header -->
    <div
      class="flex shrink-0 items-center justify-between gap-2 border-b border-outline-variant/45 px-3.5 py-2.5"
    >
      <div class="flex min-w-0 items-center gap-1.5">
        <span
          class="text-label-small truncate rounded-corner-full px-2 py-0.5"
          :class="
            preview.isMerged
              ? 'bg-primary-container text-on-primary-container'
              : 'bg-secondary-container text-on-secondary-container'
          "
        >
          {{ preview.sourceLabel }}
        </span>
        <span class="text-label-medium truncate text-on-surface-variant">
          {{ t('tool.workbench.preview.editableHint') }}
        </span>
        <span
          v-if="isDraftEdited"
          class="text-label-small shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-on-warning-container"
        >
          {{ t('tool.workbench.preview.editedBadge') }}
        </span>
      </div>

      <div class="flex shrink-0 items-center gap-0.5">
        <MdIconButton
          v-if="isDraftEdited"
          size="sm"
          :icon="LuRotateCcw"
          :tooltip="t('tool.workbench.preview.resetDraft')"
          :aria-label="t('tool.workbench.preview.resetDraft')"
          @click="resetDraft"
        />

        <MdIconButton
          v-if="closable"
          size="sm"
          :icon="LuX"
          :tooltip="t('tool.workbench.preview.closePreview')"
          :aria-label="t('tool.workbench.preview.closePreview')"
          @click="emit('close')"
        />
      </div>
    </div>

    <!-- Loading State -->
    <div
      v-if="preview.loading && !preview.meta"
      class="text-body-small flex flex-1 flex-col items-center justify-center gap-2 px-4 text-on-surface-variant"
    >
      <LuLoader2 class="h-5 w-5 animate-spin text-primary" aria-hidden="true" />
      <span>{{ t('tool.workbench.preview.loadingDetail') }}</span>
    </div>

    <!-- Shared Inspector Domain Form Body -->
    <template v-else-if="preview.meta">
      <div class="flex-1 space-y-3.5 overflow-y-auto px-3.5 pt-3 pb-4">
        <!-- Cover Card -->
        <div class="flex gap-3 rounded-corner-lg bg-surface p-3">
          <div class="flex shrink-0 flex-col items-center gap-1.5">
            <div
              class="relative overflow-hidden rounded-corner-md bg-surface-container-high"
              :class="[
                mediaKind === 'music' ? 'h-24 w-24 album-shadow' : 'h-32 w-24 book-shadow',
                !useCover ? 'opacity-45' : '',
              ]"
            >
              <img
                v-if="useCover && isCoverUsable(preview.meta.cover_url)"
                :src="resolveScraperCoverUrl(preview.meta.cover_url)"
                alt="scraped cover"
                referrerpolicy="no-referrer"
                class="h-full w-full object-cover"
                @error="markCoverBroken(preview.meta.cover_url)"
              />
              <div
                v-else
                class="flex h-full w-full flex-col items-center justify-center gap-1 text-on-surface-variant"
              >
                <LuBookOpen v-if="mediaKind === 'book'" class="h-6 w-6 opacity-50" aria-hidden="true" />
                <LuLayers v-else-if="mediaKind === 'comic'" class="h-6 w-6 opacity-50" aria-hidden="true" />
                <LuDisc3 v-else-if="mediaKind === 'music'" class="h-6 w-6 opacity-50" aria-hidden="true" />
                <LuClapperboard v-else class="h-6 w-6 opacity-50" aria-hidden="true" />
                <span class="text-label-small">
                  {{
                    !useCover
                      ? t('tool.workbench.preview.coverRemoved')
                      : preview.meta.cover_url
                        ? t('tool.workbench.preview.coverFailed')
                        : t('tool.inspector.noCover')
                  }}
                </span>
              </div>
            </div>
            <span
              v-if="useCover && isCoverUsable(preview.meta.cover_url) && sourceBadgeFor('cover_url')"
              class="text-label-small rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
            >
              {{ t('tool.workbench.preview.coverFrom', { provider: sourceBadgeFor('cover_url') }) }}
            </span>
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
            <div>
              <div class="text-title-small line-clamp-2 text-on-surface">
                {{ displayDraftTitle || preview.meta.title || '—' }}
              </div>
              <div
                v-if="displayDraftCreators"
                class="text-body-small mt-1 truncate text-on-surface-variant"
              >
                {{ displayDraftCreators }}
              </div>
              <div
                v-if="preview.loading"
                class="text-label-small mt-1.5 inline-flex items-center gap-1 text-primary"
              >
                <LuLoader2 class="h-3 w-3 animate-spin" aria-hidden="true" />
                <span>{{ t('tool.workbench.preview.fetchingMore') }}</span>
              </div>
            </div>

            <div class="flex flex-wrap gap-1.5">
              <MdChip
                v-if="isCoverUsable(preview.meta.cover_url)"
                variant="filter"
                hide-check
                :selected="!useCover"
                :icon="useCover ? LuImageOff : LuImage"
                :label="
                  useCover
                    ? t('tool.workbench.preview.skipCover')
                    : t('tool.workbench.preview.restoreCover')
                "
                @click="useCover = !useCover"
              />
            </div>
          </div>
        </div>

        <!-- Waterfall Field Provenance Summary -->
        <div
          v-if="sourceBadgesList.length > 0"
          class="flex flex-wrap items-center gap-1 rounded-corner-md bg-surface px-2.5 py-2"
        >
          <span class="text-label-small text-on-surface-variant">
            {{ t('tool.workbench.preview.fieldProvenance') }}
          </span>
          <span
            v-for="item in sourceBadgesList"
            :key="item.key"
            class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface"
          >
            {{ item.label }} · <strong class="text-primary">{{ item.provider }}</strong>
          </span>
        </div>

        <!-- Shared Domain Metadata Form -->
        <InspectorMusicForm
          v-if="mediaKind === 'music'"
          :form="candidateForm.musicForm"
          sidecar-lrc-filename=".lrc"
          :file-key="preview.key"
        />
        <InspectorVideoForm
          v-else-if="mediaKind === 'video'"
          :form="candidateForm.videoForm"
          hide-locks
        />
        <InspectorComicForm
          v-else-if="mediaKind === 'comic'"
          :form="candidateForm.comicForm"
        />
        <InspectorBookForm
          v-else
          :form="candidateForm.bookForm"
          book-format="epub"
          :writable="true"
          :is-epub="true"
          :has-cover="Boolean(preview.meta.cover_url)"
          :polishing-book="false"
          hide-locks
        />
      </div>

      <!-- Bottom Action Bar -->
      <div
        class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-outline-variant/45 bg-surface-container/75 px-3.5 py-2.5"
      >
        <div class="text-label-small flex flex-wrap items-center gap-3 text-on-surface-variant">
          <span>
            {{
              t('tool.workbench.preview.writeModeLabel', {
                mode:
                  preview.fillMode === 'overwrite'
                    ? t('tool.workbench.search.modeOverwrite')
                    : t('tool.workbench.search.modeFillMissing'),
              })
            }}
          </span>
          <span :title="t('tool.inspector.saveAndArchiveTitle')">
            <AppCheckbox v-model="organizeAfterSave" :label="t('tool.inspector.saveAndArchive')" />
          </span>
          <span
            v-if="mediaKind === 'music' && useCover && isCoverUsable(preview.meta.cover_url)"
            :title="t('tool.workbench.preview.syncAlbumCoverHint')"
          >
            <AppCheckbox v-model="writeAlbumCover" :label="t('tool.workbench.preview.syncAlbumCover')" />
          </span>
        </div>

        <div class="ml-auto flex items-center gap-1.5">
          <MdIconButton
            v-if="hasNext"
            variant="tonal"
            size="sm"
            :disabled="saving"
            :icon="LuSkipForward"
            :tooltip="t('tool.workbench.preview.applyAndNextHint')"
            :aria-label="t('tool.workbench.preview.applyAndNext')"
            @click="emitDirectApply(true)"
          />

          <MdButton
            variant="filled"
            size="sm"
            :icon="LuSave"
            :loading="saving"
            :title="t('tool.workbench.preview.applyAndSaveHint')"
            @click="emitDirectApply(false)"
          >
            {{ saving ? t('tool.actions.saving') : t('tool.workbench.preview.applyAndSave') }}
          </MdButton>
        </div>
      </div>
    </template>
  </div>
</template>
