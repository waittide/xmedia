<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuCheck,
  LuKey,
  LuSearchCheck,
  LuShieldCheck,
  LuSparkles,
} from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
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

const domainNameMap: Record<MediaKind, string> = {
  book: '图书',
  comic: '漫画',
  music: '音乐',
  video: '影视',
}

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
    :title="'批量自动刮削策略 · ' + domainNameMap[domain] + '（' + entries.length + ' 项）'"
    subtitle="未命中或低于置信度阈值的项目将自动归入任务清单，可一键聚焦并在右侧对照在线候选精修"
    max-width="max-w-xl"
    @close="emit('close')"
  >
    <template #icon>
      <LuSparkles class="h-5 w-5" />
    </template>

    <div class="space-y-5 py-2">
      <!-- 1. Match Strictness Mode -->
      <div class="space-y-2">
        <div class="flex items-center gap-1.5 text-xs font-bold text-[var(--md-sys-color-on-surface)]">
          <LuShieldCheck class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>匹配严格度</span>
        </div>
        <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
          <button
            v-for="m in [
              {
                id: 'strict' as const,
                title: '严格模式',
                desc: '置信度 ≥ 70，标题+作者/年份强校验，宁缺毋滥',
              },
              {
                id: 'standard' as const,
                title: '标准模式（推荐）',
                desc: '置信度 ≥ 45，兼顾命中率与准确性',
              },
              {
                id: 'loose' as const,
                title: '宽松模式',
                desc: '只要命中即写入，适合杂乱文件名快速补全',
              },
            ]"
            :key="m.id"
            type="button"
            class="md-state rounded-2xl border p-3 text-left transition"
            :class="
              matchMode === m.id
                ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/50'
                : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)] hover:border-[var(--md-sys-color-outline)]'
            "
            @click="matchMode = m.id"
          >
            <div class="flex items-center justify-between text-xs font-bold text-[var(--md-sys-color-on-surface)]">
              <span>{{ m.title }}</span>
              <LuCheck v-if="matchMode === m.id" class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
            </div>
            <p class="mt-1 text-[10px] leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
              {{ m.desc }}
            </p>
          </button>
        </div>
      </div>

      <!-- 2. Write Strategy -->
      <div class="space-y-2">
        <div class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">字段写入策略</div>
        <div class="grid grid-cols-2 gap-2">
          <button
            type="button"
            class="md-state rounded-xl border px-3 py-2.5 text-left transition"
            :class="
              onlyFillMissing
                ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/45'
                : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)]'
            "
            @click="onlyFillMissing = true"
          >
            <div class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">仅补全缺失字段</div>
            <div class="mt-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
              保护已有元数据，只填补空白项
            </div>
          </button>
          <button
            type="button"
            class="md-state rounded-xl border px-3 py-2.5 text-left transition"
            :class="
              !onlyFillMissing
                ? 'border-[var(--md-sys-color-warning)]/70 bg-[var(--md-sys-color-warning-container)]/60'
                : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)]'
            "
            @click="onlyFillMissing = false"
          >
            <div class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">强制覆盖已有字段</div>
            <div class="mt-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
              用在线高优先级源重写选中字段
            </div>
          </button>
        </div>
      </div>

      <!-- 3. Field Groups & Extra Actions -->
      <div class="space-y-2">
        <div class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">要刮削的字段与附加动作</div>
        <div
          class="grid grid-cols-2 gap-2.5 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-low)] p-3.5"
        >
          <AppCheckbox v-model="fieldBasic" label="基础信息（标题/作者/系列/年份）" />
          <AppCheckbox v-model="applyCover" label="下载并写入封面 / 海报" />
          <AppCheckbox v-model="fieldSummary" label="简介 / 剧情概要 / 注释" />
          <AppCheckbox v-model="fieldTags" label="流派与标签" />
          <AppCheckbox
            v-if="domain === 'music'"
            v-model="fieldLyrics"
            label="内嵌同步 / 纯文本歌词"
          />
          <AppCheckbox
            v-if="domain === 'music'"
            v-model="writeSidecarLrc"
            label="同步导出同名 .lrc 文件"
          />
          <div class="col-span-2 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-2">
            <AppCheckbox
              v-model="organizeByTemplate"
              label="刮削后同步按当前领域命名模板重命名并归档目录"
            />
          </div>
        </div>
      </div>

      <!-- 4. Provider Selection -->
      <div v-if="providers.length > 0" class="space-y-2">
        <div class="flex items-center justify-between text-xs font-bold text-[var(--md-sys-color-on-surface)]">
          <span>本次启用的数据源（按设置页优先级瀑布合并）</span>
          <span class="text-[10px] font-normal text-[var(--md-sys-color-on-surface-variant)]">
            已选 {{ selectedProviderIds.length }}/{{ providers.filter((p) => p.configured).length }}
          </span>
        </div>
        <div class="flex flex-wrap gap-1.5">
          <button
            v-for="p in providers"
            :key="p.id"
            type="button"
            class="md-state inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11px] font-medium transition"
            :class="
              !p.configured
                ? 'border-dashed border-[var(--md-sys-color-warning)]/55 bg-[var(--md-sys-color-warning-container)]/50 text-[var(--md-sys-color-on-warning-container)]'
                : selectedProviderIds.includes(p.id)
                  ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                  : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface-variant)]'
            "
            @click="toggleProvider(p)"
          >
            <LuKey v-if="!p.configured && p.requires_api_key" class="h-3 w-3" />
            <span>{{ p.name }}</span>
            <span v-if="!p.configured" class="text-[10px]">
              {{ p.requires_api_key ? '(去配置)' : '(去启用)' }}
            </span>
          </button>
        </div>
      </div>
    </div>

    <template #footer>
      <button
        type="button"
        :disabled="entries.length === 0 || !hasAnyFieldSelected"
        class="md-btn md-btn-sm md-btn-outlined md-state"
        @click="handleOpenDryRun"
      >
        <LuSearchCheck class="h-3.5 w-3.5" />
        <span>先预演置信度</span>
      </button>

      <div class="flex items-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="entries.length === 0 || !hasAnyFieldSelected"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleStart"
        >
          <LuSparkles class="h-3.5 w-3.5" />
          <span>开始自动刮削 ({{ entries.length }})</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
