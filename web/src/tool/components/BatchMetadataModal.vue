<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronUp,
  LuLanguages,
  LuPencil,
  LuReplace,
  LuRotateCcw,
  LuUndo2,
  LuWand2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import {
  BATCH_TITLE_TEMPLATE_TOKENS,
  normalizeTitleTemplateForBackend,
  renderTitleTemplatePreview,
} from '../templates'
import type {
  BatchMetadataEditInputDto,
  BatchMetadataEditReportDto,
  MediaFileEntry,
} from '../types'

const props = defineProps<{
  open: boolean
  rootId: string
  domain: MediaKind
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  completed: [report: BatchMetadataEditReportDto]
  openHistory: []
}>()

const { t } = useI18n()

const titleTemplate = ref('')
const creator = ref('')
const seriesOrAlbum = ref('')
const publisherOrStudio = ref('')
const yearText = ref('')
const language = ref('')
const genre = ref('')
const addTagsText = ref('')
const removeTagsText = ref('')

const replaceFind = ref('')
const replaceWith = ref('')
const replaceRegex = ref(false)
const replaceFields = ref<string[]>(['title', 'series'])

const zhDirection = ref<'none' | 't2s' | 's2t'>('none')

const showFilesList = ref(false)
const submitting = ref(false)
const undoing = ref(false)
const errorMsg = ref<string | null>(null)
const report = ref<BatchMetadataEditReportDto | null>(null)
const undoSuccessMsg = ref<string | null>(null)

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      errorMsg.value = null
      report.value = null
      undoSuccessMsg.value = null
      showFilesList.value = false
    }
  },
)

const domainFieldLabels = computed(() => {
  switch (props.domain) {
    case 'book':
      return {
        domainName: t('tool.domains.book'),
        creator: t('tool.forms.book.authors'),
        series: t('tool.forms.book.series'),
        publisher: t('tool.forms.book.publisher'),
        genre: t('tool.forms.common.genres'),
        showGenre: false,
      }
    case 'comic':
      return {
        domainName: t('tool.domains.comic'),
        creator: t('tool.forms.comic.writers'),
        series: t('tool.forms.comic.series'),
        publisher: t('tool.forms.common.publisher'),
        genre: t('tool.forms.comic.genres'),
        showGenre: true,
      }
    case 'music':
      return {
        domainName: t('tool.domains.music'),
        creator: t('tool.forms.music.artist'),
        series: t('tool.forms.music.album'),
        publisher: t('tool.forms.music.albumArtist'),
        genre: t('tool.forms.music.genre'),
        showGenre: true,
      }
    case 'video':
      return {
        domainName: t('tool.domains.video'),
        creator: t('tool.forms.video.directors'),
        series: t('tool.forms.video.showTitle'),
        publisher: t('tool.forms.video.studio'),
        genre: t('tool.forms.video.genres'),
        showGenre: true,
      }
  }
})

const templateTokens = BATCH_TITLE_TEMPLATE_TOKENS

const replacePresets = computed(() => [
  { label: t('tool.modals.batchMeta.presetBracketCn'), find: '【[^】]+】', replace: '', regex: true },
  { label: t('tool.modals.batchMeta.presetBracketEn'), find: '\\[[^\\]]+\\]', replace: '', regex: true },
  { label: t('tool.modals.batchMeta.presetParens'), find: '\\([^)]+\\)', replace: '', regex: true },
  { label: t('tool.modals.batchMeta.presetUnderscore'), find: '_', replace: ' ', regex: false },
])

const replaceFieldChoices = computed(() => [
  { id: 'title', label: t('tool.sort.title') },
  { id: 'series', label: t('tool.forms.comic.series') },
  { id: 'creator', label: t('tool.forms.music.artist') },
  { id: 'summary', label: t('tool.forms.common.summary') },
])

function insertToken(tok: string): void {
  titleTemplate.value = titleTemplate.value ? `${titleTemplate.value}${tok}` : tok
}

function applyReplacePreset(p: { find: string; replace: string; regex: boolean }): void {
  replaceFind.value = p.find
  replaceWith.value = p.replace
  replaceRegex.value = p.regex
  if (replaceFields.value.length === 0) {
    replaceFields.value = ['title', 'series']
  }
}

function toggleReplaceField(fieldId: string): void {
  const next = new Set(replaceFields.value)
  if (next.has(fieldId)) {
    next.delete(fieldId)
  } else {
    next.add(fieldId)
  }
  replaceFields.value = [...next]
}

function splitTokens(raw: string): string[] {
  return raw
    .split(/[,，、;；\n]+/)
    .map((s) => s.trim())
    .filter(Boolean)
}

const firstTitlePreview = computed(() => {
  const first = props.entries[0]
  if (!first) return null
  return renderTitleTemplatePreview({
    template: titleTemplate.value,
    entry: first,
    creatorOverride: creator.value,
    seriesOverride: seriesOrAlbum.value,
    yearOverride: yearText.value,
    counter: 1,
  })
})

const hasAnyRule = computed(() => {
  return Boolean(
    titleTemplate.value.trim() ||
      creator.value.trim() ||
      seriesOrAlbum.value.trim() ||
      publisherOrStudio.value.trim() ||
      yearText.value.trim() ||
      language.value.trim() ||
      genre.value.trim() ||
      addTagsText.value.trim() ||
      removeTagsText.value.trim() ||
      replaceFind.value.length > 0 ||
      zhDirection.value !== 'none',
  )
})

function resetForm(): void {
  titleTemplate.value = ''
  creator.value = ''
  seriesOrAlbum.value = ''
  publisherOrStudio.value = ''
  yearText.value = ''
  language.value = ''
  genre.value = ''
  addTagsText.value = ''
  removeTagsText.value = ''
  replaceFind.value = ''
  replaceWith.value = ''
  replaceRegex.value = false
  replaceFields.value = ['title', 'series']
  zhDirection.value = 'none'
  errorMsg.value = null
  report.value = null
  undoSuccessMsg.value = null
}

async function handleSubmit(): Promise<void> {
  if (props.entries.length === 0 || !hasAnyRule.value || submitting.value) return
  errorMsg.value = null
  undoSuccessMsg.value = null

  const parsedYear = yearText.value.trim() ? Number.parseInt(yearText.value.trim(), 10) : null
  if (yearText.value.trim() && (!Number.isFinite(parsedYear) || (parsedYear ?? 0) <= 0)) {
    errorMsg.value = t('tool.modals.batchMeta.invalidYear')
    return
  }

  const payload: BatchMetadataEditInputDto = {
    paths: props.entries.map((e) => e.rel_path),
    media_kind: props.domain,
  }
  if (titleTemplate.value.trim()) {
    payload.title_template = normalizeTitleTemplateForBackend(titleTemplate.value.trim())
  }
  if (creator.value.trim()) payload.creator = creator.value.trim()
  if (seriesOrAlbum.value.trim()) payload.series_or_album = seriesOrAlbum.value.trim()
  if (publisherOrStudio.value.trim()) payload.publisher_or_studio = publisherOrStudio.value.trim()
  if (parsedYear && parsedYear > 0) payload.year = parsedYear
  if (language.value.trim()) payload.language = language.value.trim()
  if (genre.value.trim()) payload.genre = genre.value.trim()

  const addTags = splitTokens(addTagsText.value)
  if (addTags.length > 0) payload.add_tags = addTags
  const removeTags = splitTokens(removeTagsText.value)
  if (removeTags.length > 0) payload.remove_tags = removeTags

  if (replaceFind.value.length > 0) {
    payload.replace_rule = {
      find: replaceFind.value,
      replace: replaceWith.value,
      use_regex: replaceRegex.value,
      fields: replaceFields.value.length > 0 ? replaceFields.value : ['title', 'series'],
    }
  }
  if (zhDirection.value !== 'none') {
    payload.zh_direction = zhDirection.value
  }

  submitting.value = true
  try {
    const res = await toolApi.batchEditMetadata(props.rootId, payload)
    report.value = res
    emit('completed', res)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    submitting.value = false
  }
}

async function handleUndoLastBatch(): Promise<void> {
  if (!report.value?.batch_id || undoing.value) return
  undoing.value = true
  errorMsg.value = null
  try {
    const res = await toolApi.undoMetadataBatch(report.value.batch_id)
    undoSuccessMsg.value = t('tool.modals.batchMeta.undoReport', {
      count: res.restored_count,
    })
    report.value = { ...report.value, batch_id: null }
    emit('completed', {
      batch_id: null,
      updated_count: res.restored_count,
      skipped_count: 0,
      failed_count: res.failed_paths.length,
      errors: res.failed_paths,
    })
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    undoing.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('tool.modals.batchMeta.title', { domain: domainFieldLabels.domainName })"
    :subtitle="t('tool.modals.batchMeta.subtitle', { count: entries.length })"
    max-width="max-w-2xl"
    footer-align="between"
    @close="emit('close')"
  >
    <template #icon>
      <LuPencil class="h-5 w-5" aria-hidden="true" />
    </template>

    <template #headerActions>
      <MdButton variant="tonal" :icon="LuUndo2" @click="emit('openHistory')">
        {{ t('tool.modals.batchMeta.historyBtn') }}
      </MdButton>
    </template>

    <div class="space-y-5 py-2">
      <!-- Selected Files Collapsible Preview -->
      <div class="rounded-corner-lg bg-surface">
        <button
          type="button"
          class="text-body-small flex w-full cursor-pointer items-center justify-between px-4 py-2.5 text-left"
          @click="showFilesList = !showFilesList"
        >
          <span class="font-medium text-on-surface">
            {{ t('tool.modals.batchMeta.targetSummary', { count: entries.length }) }}
            <span v-if="entries[0]" class="font-normal text-on-surface-variant">
              {{ t('tool.modals.batchMeta.firstItem', { name: entries[0].file_name }) }}
            </span>
          </span>
          <LuChevronUp v-if="showFilesList" class="h-4 w-4 text-on-surface-variant" aria-hidden="true" />
          <LuChevronDown v-else class="h-4 w-4 text-on-surface-variant" aria-hidden="true" />
        </button>
        <div
          v-if="showFilesList"
          class="max-h-32 divide-y divide-outline-variant/30 overflow-y-auto bg-surface-container-low px-4 py-1.5"
        >
          <div
            v-for="(item, idx) in entries"
            :key="item.rel_path"
            class="text-body-small flex items-center justify-between gap-2 py-1"
          >
            <span class="truncate text-on-surface">
              <span class="text-label-small mr-1.5 font-mono text-on-surface-variant">
                {{ String(idx + 1).padStart(2, '0') }}
              </span>
              {{ item.file_name }}
            </span>
            <span class="text-label-small shrink-0 truncate text-on-surface-variant">
              {{ item.display_title }}
            </span>
          </div>
        </div>
      </div>

      <!-- Feedback Banners -->
      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small flex items-start gap-2 rounded-corner-lg bg-error-container px-4 py-3 text-on-error-container"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ errorMsg }}</span>
      </div>

      <div
        v-if="undoSuccessMsg"
        class="text-body-small flex items-center gap-2 rounded-corner-lg bg-success-container px-4 py-3 font-medium text-on-success-container"
      >
        <LuCheckCircle2 class="h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ undoSuccessMsg }}</span>
      </div>

      <div
        v-if="report"
        class="text-body-small space-y-2 rounded-corner-lg bg-success-container px-4 py-3 text-on-success-container"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2 font-medium">
            <LuCheckCircle2 class="h-4 w-4 shrink-0" aria-hidden="true" />
            <span>
              {{
                t('tool.modals.batchMeta.batchDone', {
                  updated: report.updated_count,
                  skipped: report.skipped_count,
                })
              }}{{
                report.failed_count > 0
                  ? t('tool.modals.batchMeta.failedSuffix', { failed: report.failed_count })
                  : ''
              }}
            </span>
          </div>

          <MdButton
            v-if="report.batch_id"
            variant="tonal"
            :icon="LuRotateCcw"
            :loading="undoing"
            @click="handleUndoLastBatch"
          >
            {{ t('tool.modals.batchMeta.undoNow') }}
          </MdButton>
        </div>
        <ul
          v-if="report.errors.length > 0"
          class="text-label-small max-h-24 space-y-0.5 overflow-y-auto pl-6 font-mono text-error"
        >
          <li v-for="e in report.errors" :key="e">{{ e }}</li>
        </ul>
      </div>

      <!-- Section 1: Title Template -->
      <section class="space-y-2 rounded-corner-lg bg-surface p-4">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <span class="text-title-small text-on-surface">
            {{ t('tool.modals.batchMeta.titleTemplate') }}
          </span>
          <div class="flex flex-wrap gap-1">
            <MdChip
              v-for="tk in templateTokens"
              :key="tk.token"
              variant="suggestion"
              :label="tk.token"
              :title="t('tool.modals.batchMeta.insertToken', { label: tk.label })"
              @click="insertToken(tk.token)"
            />
          </div>
        </div>
        <MdTextField
          v-model="titleTemplate"
          size="sm"
          :aria-label="t('tool.modals.batchMeta.titleTemplate')"
          :placeholder="t('tool.modals.batchMeta.titlePlaceholder')"
        />
        <p v-if="firstTitlePreview" class="text-body-small text-on-surface-variant">
          {{ t('tool.modals.batchMeta.firstPreview') }}
          <span class="font-medium text-primary">{{ firstTitlePreview }}</span>
        </p>
      </section>

      <!-- Section 2: Common Field Overrides -->
      <section class="space-y-3 rounded-corner-lg bg-surface p-4">
        <div class="text-title-small text-on-surface">
          {{ t('tool.modals.batchMeta.overrideHeading') }}
        </div>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <MdTextField
            v-model="creator"
            size="sm"
            :label="domainFieldLabels.creator"
          />

          <MdTextField
            v-model="seriesOrAlbum"
            size="sm"
            :label="domainFieldLabels.series"
          />

          <MdTextField
            v-model="publisherOrStudio"
            size="sm"
            :label="domainFieldLabels.publisher"
          />

          <div class="grid grid-cols-2 gap-2.5">
            <MdTextField
              v-model="yearText"
              size="sm"
              inputmode="numeric"
              :label="t('tool.forms.common.year')"
              placeholder="2024"
            />
            <MdTextField
              v-model="language"
              size="sm"
              :label="t('tool.modals.batchMeta.languageCode')"
              placeholder="zh / ja / en"
            />
          </div>

          <div v-if="domainFieldLabels.showGenre" class="sm:col-span-2">
            <MdTextField
              v-model="genre"
              size="sm"
              :label="domainFieldLabels.genre"
              :placeholder="t('tool.modals.batchMeta.genrePlaceholder')"
            />
          </div>
        </div>
      </section>

      <!-- Section 3: Tag Add / Remove -->
      <section v-if="domain !== 'music'" class="space-y-2.5 rounded-corner-lg bg-surface p-4">
        <div class="text-title-small text-on-surface">
          {{ t('tool.modals.batchMeta.tagsHeading') }}
        </div>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <MdTextField
            v-model="addTagsText"
            size="sm"
            :label="t('tool.modals.batchMeta.addTags')"
            :placeholder="t('tool.modals.batchMeta.addTagsPlaceholder')"
          />
          <MdTextField
            v-model="removeTagsText"
            size="sm"
            :label="t('tool.modals.batchMeta.removeTags')"
            :placeholder="t('tool.modals.batchMeta.removeTagsPlaceholder')"
          />
        </div>
      </section>

      <!-- Section 4: Regex / Text Replace -->
      <section class="space-y-3 rounded-corner-lg bg-surface p-4">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="text-title-small flex items-center gap-1.5 text-on-surface">
            <LuReplace class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ t('tool.modals.batchMeta.replaceHeading') }}</span>
          </div>
          <div class="flex flex-wrap gap-1">
            <MdChip
              v-for="p in replacePresets"
              :key="p.label"
              variant="suggestion"
              :label="p.label"
              @click="applyReplacePreset(p)"
            />
          </div>
        </div>

        <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <MdTextField
            v-model="replaceFind"
            size="sm"
            :label="t('tool.modals.batchMeta.findLabel')"
          />
          <MdTextField
            v-model="replaceWith"
            size="sm"
            :label="t('tool.modals.batchMeta.replaceLabel')"
          />
        </div>

        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('tool.modals.batchMeta.targetFields') }}
            </span>
            <MdChip
              v-for="f in replaceFieldChoices"
              :key="f.id"
              variant="filter"
              :label="f.label"
              :selected="replaceFields.includes(f.id)"
              @click="toggleReplaceField(f.id)"
            />
          </div>

          <AppCheckbox v-model="replaceRegex" :label="t('tool.modals.batchMeta.useRegex')" />
        </div>
      </section>

      <!-- Section 5: Traditional <-> Simplified Conversion -->
      <section
        class="flex flex-wrap items-center justify-between gap-3 rounded-corner-lg bg-surface px-4 py-3"
      >
        <div class="flex items-center gap-2.5">
          <LuLanguages class="h-4 w-4 text-primary" aria-hidden="true" />
          <div>
            <div class="text-title-small text-on-surface">{{ t('tool.modals.batchMeta.zhHeading') }}</div>
            <div class="text-body-small text-on-surface-variant">
              {{ t('tool.modals.batchMeta.zhDesc') }}
            </div>
          </div>
        </div>

        <MdSegmentedButton
          v-model="zhDirection"
          :options="[
            { value: 'none', label: t('tool.modals.batchMeta.zhNone') },
            { value: 't2s', label: t('tool.modals.batchMeta.zhT2s') },
            { value: 's2t', label: t('tool.modals.batchMeta.zhS2t') },
          ]"
          size="sm"
        />
      </section>
    </div>

    <template #footer>
      <MdButton variant="text" :icon="LuRotateCcw" @click="resetForm">
        {{ t('tool.modals.batchMeta.resetForm') }}
      </MdButton>

      <div class="flex items-center gap-2">
        <MdButton variant="text" @click="emit('close')">
          {{ t('tool.common.close') }}
        </MdButton>
        <MdButton
          variant="filled"
          :icon="LuWand2"
          :loading="submitting"
          :disabled="!hasAnyRule || entries.length === 0"
          @click="handleSubmit"
        >
          {{
            submitting
              ? t('tool.actions.saving')
              : t('tool.modals.batchMeta.applyCount', { count: entries.length })
          }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
