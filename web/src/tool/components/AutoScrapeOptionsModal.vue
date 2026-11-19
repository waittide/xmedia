<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuCheck,
  LuKey,
  LuSearchCheck,
  LuShieldCheck,
  LuSparkles,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import type {
  AutoScrapeMatchMode,
  AutoScrapeOptionsDto,
  MediaFileEntry,
  ProviderStatusDto,
} from '../types'

const STORAGE_KEY = 'xmedia_tool.auto_scrape_prefs.v1'

const props = defineProps<{
  open: boolean
  domain: MediaKind
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  start: [entries: MediaFileEntry[], options: AutoScrapeOptionsDto]
  openDryRun: [entries: MediaFileEntry[]]
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
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return
    const parsed = JSON.parse(raw) as Partial<{
      match_mode: AutoScrapeMatchMode
      only_fill_missing: boolean
      apply_cover: boolean
      write_sidecar_lrc: boolean
      organize_by_template: boolean
      field_basic: boolean
      field_summary: boolean
      field_tags: boolean
      field_lyrics: boolean
    }>
    if (parsed.match_mode === 'strict' || parsed.match_mode === 'standard' || parsed.match_mode === 'loose') {
      matchMode.value = parsed.match_mode
    }
    if (typeof parsed.only_fill_missing === 'boolean') onlyFillMissing.value = parsed.only_fill_missing
    if (typeof parsed.apply_cover === 'boolean') applyCover.value = parsed.apply_cover
    if (typeof parsed.write_sidecar_lrc === 'boolean') writeSidecarLrc.value = parsed.write_sidecar_lrc
    if (typeof parsed.organize_by_template === 'boolean') {
      organizeByTemplate.value = parsed.organize_by_template
    }
    if (typeof parsed.field_basic === 'boolean') fieldBasic.value = parsed.field_basic
    if (typeof parsed.field_summary === 'boolean') fieldSummary.value = parsed.field_summary
    if (typeof parsed.field_tags === 'boolean') fieldTags.value = parsed.field_tags
    if (typeof parsed.field_lyrics === 'boolean') fieldLyrics.value = parsed.field_lyrics
  } catch {
    // ignore malformed local prefs
  }
}

function savePrefs(): void {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        match_mode: matchMode.value,
        only_fill_missing: onlyFillMissing.value,
        apply_cover: applyCover.value,
        write_sidecar_lrc: writeSidecarLrc.value,
        organize_by_template: organizeByTemplate.value,
        field_basic: fieldBasic.value,
        field_summary: fieldSummary.value,
        field_tags: fieldTags.value,
        field_lyrics: fieldLyrics.value,
      }),
    )
  } catch {
    // ignore storage quota
  }
}

async function loadDomainProviders(): Promise<void> {
  try {
    const list = await toolApi.listProviders(undefined, props.domain)
    providers.value = list
    selectedProviderIds.value = list.filter((p) => p.configured).map((p) => p.id)
  } catch {
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

function handleStart(): void {
  if (props.entries.length === 0 || !hasAnyFieldSelected.value) return
  savePrefs()
  const groups: string[] = []
  if (fieldBasic.value) groups.push('basic')
  if (fieldSummary.value) groups.push('summary')
  if (fieldTags.value) groups.push('tags')
  if (props.domain === 'music' && fieldLyrics.value) groups.push('lyrics')

  const allConfigured = providers.value.filter((p) => p.configured).map((p) => p.id)
  const customProviders =
    selectedProviderIds.value.length > 0 && selectedProviderIds.value.length < allConfigured.length
      ? [...selectedProviderIds.value]
      : []

  const options: AutoScrapeOptionsDto = {
    match_mode: matchMode.value,
    only_fill_missing: onlyFillMissing.value,
    apply_cover: applyCover.value,
    write_sidecar_lrc: props.domain === 'music' ? writeSidecarLrc.value : false,
    organize_by_template: organizeByTemplate.value,
    providers: customProviders,
    field_groups: groups.length === (props.domain === 'music' ? 4 : 3) ? [] : groups,
  }
  emit('start', props.entries, options)
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-xs"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[90vh] w-full max-w-xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/60 px-5 py-4"
        >
          <div class="flex items-center gap-2.5">
            <div
              class="flex h-9 w-9 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuSparkles class="h-4 w-4" />
            </div>
            <div>
              <h2 class="text-sm font-bold text-[var(--ctp-text)]">
                批量自动刮削策略 · {{ domainNameMap[domain] }}（{{ entries.length }} 项）
              </h2>
              <p class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
                未命中或低于置信度阈值的项目将自动归入任务清单，可一键聚焦并在右侧对照在线候选精修
              </p>
            </div>
          </div>
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Body -->
        <div class="min-h-0 flex-1 space-y-5 overflow-y-auto p-5">
          <!-- 1. Match Strictness Mode -->
          <div class="space-y-2">
            <div class="flex items-center gap-1.5 text-xs font-bold text-[var(--ctp-text)]">
              <LuShieldCheck class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
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
                class="rounded-2xl border p-3 text-left transition"
                :class="
                  matchMode === m.id
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/50'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/40 hover:border-[var(--ctp-surface1)]'
                "
                @click="matchMode = m.id"
              >
                <div class="flex items-center justify-between text-xs font-bold text-[var(--ctp-text)]">
                  <span>{{ m.title }}</span>
                  <LuCheck v-if="matchMode === m.id" class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                </div>
                <p class="mt-1 text-[10px] leading-relaxed text-[var(--ctp-subtext0)]">
                  {{ m.desc }}
                </p>
              </button>
            </div>
          </div>

          <!-- 2. Write Strategy -->
          <div class="space-y-2">
            <div class="text-xs font-bold text-[var(--ctp-text)]">字段写入策略</div>
            <div class="grid grid-cols-2 gap-2">
              <button
                type="button"
                class="rounded-xl border px-3 py-2.5 text-left transition"
                :class="
                  onlyFillMissing
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/45'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/40'
                "
                @click="onlyFillMissing = true"
              >
                <div class="text-xs font-bold text-[var(--ctp-text)]">仅补全缺失字段</div>
                <div class="mt-0.5 text-[10px] text-[var(--ctp-subtext0)]">
                  保护已有元数据，只填补空白项
                </div>
              </button>
              <button
                type="button"
                class="rounded-xl border px-3 py-2.5 text-left transition"
                :class="
                  !onlyFillMissing
                    ? 'border-amber-500/70 bg-amber-500/10'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/40'
                "
                @click="onlyFillMissing = false"
              >
                <div class="text-xs font-bold text-[var(--ctp-text)]">强制覆盖已有字段</div>
                <div class="mt-0.5 text-[10px] text-[var(--ctp-subtext0)]">
                  用在线高优先级源重写选中字段
                </div>
              </button>
            </div>
          </div>

          <!-- 3. Field Groups & Extra Actions -->
          <div class="space-y-2">
            <div class="text-xs font-bold text-[var(--ctp-text)]">要刮削的字段与附加动作</div>
            <div
              class="grid grid-cols-2 gap-2.5 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/35 p-3.5"
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
              <div class="col-span-2 border-t border-[var(--ctp-surface0)]/70 pt-2">
                <AppCheckbox
                  v-model="organizeByTemplate"
                  label="刮削后同步按当前领域命名模板重命名并归档目录"
                />
              </div>
            </div>
          </div>

          <!-- 4. Provider Selection -->
          <div v-if="providers.length > 0" class="space-y-2">
            <div class="flex items-center justify-between text-xs font-bold text-[var(--ctp-text)]">
              <span>本次启用的数据源（按设置页优先级瀑布合并）</span>
              <span class="text-[10px] font-normal text-[var(--ctp-subtext0)]">
                已选 {{ selectedProviderIds.length }}/{{ providers.filter((p) => p.configured).length }}
              </span>
            </div>
            <div class="flex flex-wrap gap-1.5">
              <button
                v-for="p in providers"
                :key="p.id"
                type="button"
                class="inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11px] font-medium transition"
                :class="
                  !p.configured
                    ? 'border-dashed border-amber-500/45 bg-amber-500/10 text-amber-400'
                    : selectedProviderIds.includes(p.id)
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                      : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext0)]'
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

        <!-- Footer -->
        <div
          class="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/50 px-5 py-3.5"
        >
          <button
            type="button"
            class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
            @click="emit('openDryRun', entries)"
          >
            <LuSearchCheck class="h-3.5 w-3.5" />
            <span>先预演置信度</span>
          </button>

          <div class="flex items-center gap-2">
            <button
              type="button"
              class="h-9 rounded-xl border border-[var(--ctp-surface0)] px-4 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/50"
              @click="emit('close')"
            >
              取消
            </button>
            <button
              type="button"
              :disabled="entries.length === 0 || !hasAnyFieldSelected"
              class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-45"
              @click="handleStart"
            >
              <LuSparkles class="h-3.5 w-3.5" />
              <span>开始自动刮削 ({{ entries.length }})</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
