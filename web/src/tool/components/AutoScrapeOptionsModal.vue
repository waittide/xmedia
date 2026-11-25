<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCheck,
  LuKey,
  LuSearchCheck,
  LuShieldCheck,
  LuSparkles,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import {
  buildAutoScrapeOptionsFromPrefs,
  loadAutoScrapePrefs,
  saveAutoScrapePrefs,
  saveLastAutoScrapeOptions,
} from '../autoScrapePrefs'
import type {
  AutoScrapeMatchMode,
  AutoScrapeOptionsDto,
  MediaFileEntry,
  ProviderStatusDto,
} from '../types'

const props = defineProps<{
  open: boolean
  domain: MediaKind
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  start: [entries: MediaFileEntry[], options: AutoScrapeOptionsDto]
  openDryRun: [entries: MediaFileEntry[], options: AutoScrapeOptionsDto]
  openSettings: []
}>()

const { t } = useI18n()

const matchMode = ref<AutoScrapeMatchMode>('standard')
const onlyFillMissing = ref(true)
const applyCover = ref(true)
const writeSidecarLrc = ref(true)
const organizeByTemplate = ref(false)
const fieldBasic = ref(true)
const fieldSummary = ref(true)
const fieldTags = ref(true)
const fieldLyrics = ref(true)

const providers = ref<ProviderStatusDto[]>([])
const selectedProviderIds = ref<string[]>([])

const matchModes = computed(() => [
  {
    id: 'strict' as const,
    title: t('tool.modals.autoScrape.modeStrict'),
    desc: t('tool.modals.autoScrape.modeStrictDesc'),
  },
  {
    id: 'standard' as const,
    title: t('tool.modals.autoScrape.modeStandard'),
    desc: t('tool.modals.autoScrape.modeStandardDesc'),
  },
  {
    id: 'loose' as const,
    title: t('tool.modals.autoScrape.modeLoose'),
    desc: t('tool.modals.autoScrape.modeLooseDesc'),
  },
])

function loadSavedPrefs(): void {
  const parsed = loadAutoScrapePrefs()
  matchMode.value = parsed.match_mode
  onlyFillMissing.value = parsed.only_fill_missing
  applyCover.value = parsed.apply_cover
  writeSidecarLrc.value = parsed.write_sidecar_lrc
  organizeByTemplate.value = parsed.organize_by_template
  fieldBasic.value = parsed.field_basic
  fieldSummary.value = parsed.field_summary
  fieldTags.value = parsed.field_tags
  fieldLyrics.value = parsed.field_lyrics
}

function savePrefs(): void {
  saveAutoScrapePrefs({
    match_mode: matchMode.value,
    only_fill_missing: onlyFillMissing.value,
    apply_cover: applyCover.value,
    write_sidecar_lrc: writeSidecarLrc.value,
    organize_by_template: organizeByTemplate.value,
    field_basic: fieldBasic.value,
    field_summary: fieldSummary.value,
    field_tags: fieldTags.value,
    field_lyrics: fieldLyrics.value,
  })
}

let providerLoadSeq = 0

async function loadDomainProviders(): Promise<void> {
  const seq = ++providerLoadSeq
  try {
    const list = await toolApi.listProviders(undefined, props.domain)
    if (seq !== providerLoadSeq || !props.open) return
    providers.value = list
    selectedProviderIds.value = list.filter((p) => p.configured).map((p) => p.id)
  } catch {
    if (seq !== providerLoadSeq) return
    providers.value = []
    selectedProviderIds.value = []
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      loadSavedPrefs()
      void loadDomainProviders()
    }
  },
  { immediate: true },
)

function toggleProvider(p: ProviderStatusDto): void {
  if (!p.configured) {
    emit('openSettings')
    return
  }
  if (selectedProviderIds.value.includes(p.id)) {
    if (selectedProviderIds.value.length <= 1) return
    selectedProviderIds.value = selectedProviderIds.value.filter((id) => id !== p.id)
  } else {
    selectedProviderIds.value = [...selectedProviderIds.value, p.id]
  }
}

const hasAnyFieldSelected = computed(
  () =>
    applyCover.value ||
    fieldBasic.value ||
    fieldSummary.value ||
    fieldTags.value ||
    (props.domain === 'music' && fieldLyrics.value),
)

function buildOptions(): AutoScrapeOptionsDto {
  const allConfigured = providers.value.filter((p) => p.configured).map((p) => p.id)
  const customProviders =
    selectedProviderIds.value.length > 0 && selectedProviderIds.value.length < allConfigured.length
      ? [...selectedProviderIds.value]
      : []

  return buildAutoScrapeOptionsFromPrefs(
    {
      match_mode: matchMode.value,
      only_fill_missing: onlyFillMissing.value,
      apply_cover: applyCover.value,
      write_sidecar_lrc: writeSidecarLrc.value,
      organize_by_template: organizeByTemplate.value,
      field_basic: fieldBasic.value,
      field_summary: fieldSummary.value,
      field_tags: fieldTags.value,
      field_lyrics: fieldLyrics.value,
    },
    props.domain,
    customProviders,
  )
}

function handleStart(): void {
  if (props.entries.length === 0 || !hasAnyFieldSelected.value) return
  savePrefs()
  const opts = buildOptions()
  saveLastAutoScrapeOptions(props.domain, opts)
  emit('start', props.entries, opts)
}

function handleOpenDryRun(): void {
  if (props.entries.length === 0 || !hasAnyFieldSelected.value) return
  savePrefs()
  const opts = buildOptions()
  saveLastAutoScrapeOptions(props.domain, opts)
  emit('openDryRun', props.entries, opts)
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="
      t('tool.modals.autoScrape.title', {
        domain: t(`tool.domains.${domain}`),
        count: entries.length,
      })
    "
    :subtitle="t('tool.modals.autoScrape.subtitle')"
    max-width="max-w-xl"
    footer-align="between"
    @close="emit('close')"
  >
    <template #icon>
      <LuSparkles class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="space-y-5 py-2">
      <!-- 1. Match Strictness Mode -->
      <div class="space-y-2">
        <div class="text-title-small flex items-center gap-1.5 text-on-surface">
          <LuShieldCheck class="h-4 w-4 text-primary" aria-hidden="true" />
          <span>{{ t('tool.modals.autoScrape.strictness') }}</span>
        </div>
        <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
          <button
            v-for="m in matchModes"
            :key="m.id"
            type="button"
            class="md-state cursor-pointer rounded-corner-lg p-3 text-left transition"
            :class="
              matchMode === m.id
                ? 'bg-secondary-container text-on-secondary-container'
                : 'bg-surface text-on-surface hover:bg-surface-container-low'
            "
            @click="matchMode = m.id"
          >
            <div class="text-label-large flex items-center justify-between">
              <span>{{ m.title }}</span>
              <LuCheck v-if="matchMode === m.id" class="h-4 w-4 text-primary" aria-hidden="true" />
            </div>
            <p class="text-body-small mt-1 leading-relaxed opacity-85">
              {{ m.desc }}
            </p>
          </button>
        </div>
      </div>

      <!-- 2. Write Strategy -->
      <div class="space-y-2">
        <div class="text-title-small text-on-surface">
          {{ t('tool.modals.autoScrape.writeStrategy') }}
        </div>
        <div class="grid grid-cols-2 gap-2">
          <button
            type="button"
            class="md-state cursor-pointer rounded-corner-lg px-3.5 py-2.5 text-left transition"
            :class="
              onlyFillMissing
                ? 'bg-secondary-container text-on-secondary-container'
                : 'bg-surface text-on-surface hover:bg-surface-container-low'
            "
            @click="onlyFillMissing = true"
          >
            <div class="text-label-large">{{ t('tool.modals.autoScrape.fillMissingTitle') }}</div>
            <div class="text-body-small mt-0.5 opacity-85">
              {{ t('tool.modals.autoScrape.fillMissingDesc') }}
            </div>
          </button>
          <button
            type="button"
            class="md-state cursor-pointer rounded-corner-lg px-3.5 py-2.5 text-left transition"
            :class="
              !onlyFillMissing
                ? 'bg-warning-container text-on-warning-container'
                : 'bg-surface text-on-surface hover:bg-surface-container-low'
            "
            @click="onlyFillMissing = false"
          >
            <div class="text-label-large">{{ t('tool.modals.autoScrape.overwriteTitle') }}</div>
            <div class="text-body-small mt-0.5 opacity-85">
              {{ t('tool.modals.autoScrape.overwriteDesc') }}
            </div>
          </button>
        </div>
      </div>

      <!-- 3. Field Groups & Extra Actions -->
      <div class="space-y-2">
        <div class="text-title-small text-on-surface">
          {{ t('tool.modals.autoScrape.fieldsHeading') }}
        </div>
        <div class="grid grid-cols-1 gap-2.5 rounded-corner-lg bg-surface p-3.5 sm:grid-cols-2">
          <AppCheckbox v-model="fieldBasic" :label="t('tool.modals.autoScrape.fieldBasic')" />
          <AppCheckbox v-model="applyCover" :label="t('tool.modals.autoScrape.fieldCover')" />
          <AppCheckbox v-model="fieldSummary" :label="t('tool.modals.autoScrape.fieldSummary')" />
          <AppCheckbox v-model="fieldTags" :label="t('tool.modals.autoScrape.fieldTags')" />
          <AppCheckbox
            v-if="domain === 'music'"
            v-model="fieldLyrics"
            :label="t('tool.modals.autoScrape.fieldLyrics')"
          />
          <AppCheckbox
            v-if="domain === 'music'"
            v-model="writeSidecarLrc"
            :label="t('tool.modals.autoScrape.fieldSidecarLrc')"
          />
          <div class="pt-2 sm:col-span-2">
            <AppCheckbox
              v-model="organizeByTemplate"
              :label="t('tool.modals.autoScrape.organizeAfter')"
            />
          </div>
        </div>
      </div>

      <!-- 4. Provider Selection -->
      <div v-if="providers.length > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <span class="text-title-small text-on-surface">
            {{ t('tool.modals.autoScrape.providersHeading') }}
          </span>
          <span class="text-label-small text-on-surface-variant">
            {{
              t('tool.modals.autoScrape.selectedRatio', {
                selected: selectedProviderIds.length,
                total: providers.filter((p) => p.configured).length,
              })
            }}
          </span>
        </div>
        <div class="flex flex-wrap gap-1.5">
          <MdChip
            v-for="p in providers"
            :key="p.id"
            variant="filter"
            :selected="p.configured && selectedProviderIds.includes(p.id)"
            @click="toggleProvider(p)"
          >
            <template #icon>
              <LuKey
                v-if="!p.configured && p.requires_api_key"
                class="h-3.5 w-3.5 shrink-0 text-warning"
                aria-hidden="true"
              />
            </template>
            <span>{{ p.name }}</span>
            <span v-if="!p.configured" class="text-label-small ml-1 opacity-80">
              {{
                p.requires_api_key
                  ? t('tool.modals.autoScrape.goConfigure')
                  : t('tool.modals.autoScrape.goEnable')
              }}
            </span>
          </MdChip>
        </div>
      </div>
    </div>

    <template #footer>
      <MdButton
        variant="outlined"
        :icon="LuSearchCheck"
        :disabled="entries.length === 0 || !hasAnyFieldSelected"
        @click="handleOpenDryRun"
      >
        {{ t('tool.modals.autoScrape.dryRunFirst') }}
      </MdButton>

      <div class="flex items-center gap-2">
        <MdButton variant="text" @click="emit('close')">
          {{ t('tool.common.cancel') }}
        </MdButton>
        <MdButton
          variant="filled"
          :icon="LuSparkles"
          :disabled="entries.length === 0 || !hasAnyFieldSelected"
          @click="handleStart"
        >
          {{ t('tool.modals.autoScrape.startScrape', { count: entries.length }) }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
