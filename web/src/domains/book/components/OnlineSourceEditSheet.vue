<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import {
  LuBookOpen,
  LuCheckCircle2,
  LuCode2,
  LuCompass,
  LuCopy,
  LuFileText,
  LuFlaskConical,
  LuInfo,
  LuList,
  LuLoader2,
  LuSave,
  LuSearch,
  LuXCircle,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { onlineBookApi, type BookSourceSummaryDto } from '../onlineApi'
import RuleFieldsSection, { type RuleFieldDef } from './RuleFieldsSection.vue'

const props = defineProps<{
  open: boolean
  source: BookSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: BookSourceSummaryDto]
  savedAndDebug: [source: BookSourceSummaryDto]
}>()

const snackbar = useSnackbarStore()

type EditTab = 'basic' | 'search' | 'explore' | 'book_info' | 'toc' | 'content' | 'json'

const sourceTypeOptions = [
  { value: '0', label: '0 · 文本' },
  { value: '1', label: '1 · 音频' },
  { value: '2', label: '2 · 图片' },
  { value: '3', label: '3 · 文件' },
]

const searchRuleFields: RuleFieldDef[] = [
  { key: 'checkKeyWord', label: '校验关键词', hint: 'checkKeyWord', placeholder: '调试默认搜索词，例如：我的' },
  { key: 'bookList', label: '书籍列表规则', hint: 'bookList', placeholder: 'CSS / JSONPath / @js 列表选择器', fullWidth: true },
  { key: 'name', label: '书名规则', hint: 'name' },
  { key: 'author', label: '作者规则', hint: 'author' },
  { key: 'bookUrl', label: '详情页 URL', hint: 'bookUrl', fullWidth: true },
  { key: 'coverUrl', label: '封面 URL', hint: 'coverUrl', fullWidth: true },
  { key: 'kind', label: '分类 / 标签', hint: 'kind' },
  { key: 'wordCount', label: '字数规则', hint: 'wordCount' },
  { key: 'lastChapter', label: '最新章节', hint: 'lastChapter', fullWidth: true },
  { key: 'intro', label: '简介规则', hint: 'intro', multiline: true, rows: 2, fullWidth: true },
]

const exploreRuleFields: RuleFieldDef[] = [
  { key: 'bookList', label: '书籍列表规则', hint: 'bookList', fullWidth: true },
  { key: 'name', label: '书名规则', hint: 'name' },
  { key: 'author', label: '作者规则', hint: 'author' },
  { key: 'bookUrl', label: '详情页 URL', hint: 'bookUrl', fullWidth: true },
  { key: 'coverUrl', label: '封面 URL', hint: 'coverUrl', fullWidth: true },
  { key: 'kind', label: '分类 / 标签', hint: 'kind' },
  { key: 'wordCount', label: '字数规则', hint: 'wordCount' },
  { key: 'lastChapter', label: '最新章节', hint: 'lastChapter', fullWidth: true },
  { key: 'intro', label: '简介规则', hint: 'intro', multiline: true, rows: 2, fullWidth: true },
]

const bookInfoRuleFields: RuleFieldDef[] = [
  { key: 'init', label: '预处理规则', hint: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'name', label: '书名规则', hint: 'name' },
  { key: 'author', label: '作者规则', hint: 'author' },
  { key: 'tocUrl', label: '目录页 URL', hint: 'tocUrl', fullWidth: true },
  { key: 'coverUrl', label: '封面 URL', hint: 'coverUrl', fullWidth: true },
  { key: 'kind', label: '分类 / 标签', hint: 'kind' },
  { key: 'wordCount', label: '字数规则', hint: 'wordCount' },
  { key: 'lastChapter', label: '最新章节', hint: 'lastChapter' },
  { key: 'canReName', label: '允许修改书名', hint: 'canReName' },
  { key: 'intro', label: '简介规则', hint: 'intro', multiline: true, rows: 3, fullWidth: true },
]

const tocRuleFields: RuleFieldDef[] = [
  { key: 'chapterList', label: '章节列表规则', hint: 'chapterList', fullWidth: true },
  { key: 'chapterName', label: '章节名称规则', hint: 'chapterName' },
  { key: 'chapterUrl', label: '章节链接规则', hint: 'chapterUrl' },
  { key: 'nextTocUrl', label: '下一页目录 URL', hint: 'nextTocUrl', fullWidth: true },
  { key: 'updateTime', label: '章节更新时间', hint: 'updateTime' },
  { key: 'isVolume', label: '卷标判断规则', hint: 'isVolume' },
  { key: 'isVip', label: 'VIP 章节判断', hint: 'isVip' },
  { key: 'preUpdateJs', label: '目录前置 JS', hint: 'preUpdateJs', multiline: true, rows: 3, fullWidth: true },
]

const contentRuleFields: RuleFieldDef[] = [
  { key: 'content', label: '正文内容规则', hint: 'content', multiline: true, rows: 4, fullWidth: true },
  { key: 'title', label: '章节标题规则', hint: 'title' },
  { key: 'nextContentUrl', label: '正文下一页 URL', hint: 'nextContentUrl' },
  { key: 'replaceRegex', label: '正文净化正则', hint: 'replaceRegex', multiline: true, rows: 2, fullWidth: true },
  { key: 'sourceRegex', label: '资源 sniffing 正则', hint: 'sourceRegex', fullWidth: true },
  { key: 'webJs', label: 'WebView 注入 JS', hint: 'webJs', multiline: true, rows: 3, fullWidth: true },
]

const activeTab = ref<EditTab>('basic')
const loading = ref(false)
const saving = ref(false)
const loadError = ref<string | null>(null)

// Preserves any unknown extra keys in the original Legado JSON losslessly
const rawBaseObj = ref<Record<string, unknown>>({})
const jsonText = ref('')
const jsonDirty = ref(false)
const variableText = ref('')
const initialVariableText = ref('')
let loadSeq = 0

interface FormSnapshot {
  bookSourceGroup: string
  bookSourceComment: string
  bookSourceType: string
  enabled: boolean
  enabledExplore: boolean
  enabledCookieJar: boolean
  weight: number
  concurrentRate: string
  header: string
  bookUrlPattern: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  searchUrl: string
  exploreUrl: string
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleBookInfo: Record<string, string>
  ruleToc: Record<string, string>
  ruleContent: Record<string, string>
}

const form = reactive({
  bookSourceName: '',
  bookSourceUrl: '',
  bookSourceGroup: '',
  bookSourceComment: '',
  bookSourceType: '0',
  enabled: true,
  enabledExplore: true,
  enabledCookieJar: true,
  weight: 0 as number | string,
  concurrentRate: '',
  header: '',
  bookUrlPattern: '',
  loginUrl: '',
  loginUi: '',
  loginCheckJs: '',
  jsLib: '',
  searchUrl: '',
  exploreUrl: '',
  ruleSearch: {} as Record<string, string>,
  ruleExplore: {} as Record<string, string>,
  ruleBookInfo: {} as Record<string, string>,
  ruleToc: {} as Record<string, string>,
  ruleContent: {} as Record<string, string>,
})

let initialSnapshot: FormSnapshot | null = null

function strVal(v: unknown): string {
  if (v === null || v === undefined) return ''
  if (typeof v === 'string') return v
  if (typeof v === 'number' || typeof v === 'boolean') return String(v)
  try {
    return JSON.stringify(v)
  } catch {
    return ''
  }
}

function boolVal(v: unknown, fallback = true): boolean {
  if (typeof v === 'boolean') return v
  if (typeof v === 'number') return v !== 0
  if (typeof v === 'string') {
    const t = v.trim().toLowerCase()
    if (t === 'false' || t === '0' || t === '') return false
    return true
  }
  return fallback
}

function numVal(v: unknown, fallback = 0): number {
  if (typeof v === 'number' && Number.isFinite(v)) return Math.trunc(v)
  if (typeof v === 'string' && v.trim() !== '') {
    const n = Number(v.trim())
    if (Number.isFinite(n)) return Math.trunc(n)
  }
  return fallback
}

function readRuleMap(rawRule: unknown, fields: RuleFieldDef[]): Record<string, string> {
  const out: Record<string, string> = {}
  const srcObj =
    rawRule && typeof rawRule === 'object' && !Array.isArray(rawRule)
      ? (rawRule as Record<string, unknown>)
      : {}
  for (const f of fields) {
    out[f.key] = strVal(srcObj[f.key])
  }
  return out
}

function clearFormState(): void {
  rawBaseObj.value = {}
  initialSnapshot = null
  jsonText.value = ''
  jsonDirty.value = false
  variableText.value = ''
  initialVariableText.value = ''
  form.bookSourceName = ''
  form.bookSourceUrl = ''
  form.bookSourceGroup = ''
  form.bookSourceComment = ''
  form.bookSourceType = '0'
  form.enabled = true
  form.enabledExplore = true
  form.enabledCookieJar = true
  form.weight = 0
  form.concurrentRate = ''
  form.header = ''
  form.bookUrlPattern = ''
  form.loginUrl = ''
  form.loginUi = ''
  form.loginCheckJs = ''
  form.jsLib = ''
  form.searchUrl = ''
  form.exploreUrl = ''
  form.ruleSearch = readRuleMap(undefined, searchRuleFields)
  form.ruleExplore = readRuleMap(undefined, exploreRuleFields)
  form.ruleBookInfo = readRuleMap(undefined, bookInfoRuleFields)
  form.ruleToc = readRuleMap(undefined, tocRuleFields)
  form.ruleContent = readRuleMap(undefined, contentRuleFields)
}

function captureSnapshot(): FormSnapshot {
  return {
    bookSourceGroup: form.bookSourceGroup,
    bookSourceComment: form.bookSourceComment,
    bookSourceType: form.bookSourceType,
    enabled: form.enabled,
    enabledExplore: form.enabledExplore,
    enabledCookieJar: form.enabledCookieJar,
    weight: numVal(form.weight, 0),
    concurrentRate: form.concurrentRate,
    header: form.header,
    bookUrlPattern: form.bookUrlPattern,
    loginUrl: form.loginUrl,
    loginUi: form.loginUi,
    loginCheckJs: form.loginCheckJs,
    jsLib: form.jsLib,
    searchUrl: form.searchUrl,
    exploreUrl: form.exploreUrl,
    ruleSearch: { ...form.ruleSearch },
    ruleExplore: { ...form.ruleExplore },
    ruleBookInfo: { ...form.ruleBookInfo },
    ruleToc: { ...form.ruleToc },
    ruleContent: { ...form.ruleContent },
  }
}

function hydrateFormFromObject(obj: Record<string, unknown>, srcSummary?: BookSourceSummaryDto): void {
  rawBaseObj.value = { ...obj }
  form.bookSourceName = strVal(obj.bookSourceName) || srcSummary?.name || ''
  form.bookSourceUrl = strVal(obj.bookSourceUrl) || srcSummary?.source_url || ''
  form.bookSourceGroup =
    obj.bookSourceGroup !== undefined ? strVal(obj.bookSourceGroup) : (srcSummary?.group_name ?? '')
  form.bookSourceComment = strVal(obj.bookSourceComment)
  form.bookSourceType = String(numVal(obj.bookSourceType, srcSummary?.source_type ?? 0))
  form.enabled = srcSummary ? srcSummary.enabled : boolVal(obj.enabled, true)
  form.enabledExplore = boolVal(obj.enabledExplore, true)
  form.enabledCookieJar = boolVal(obj.enabledCookieJar, true)
  form.weight = srcSummary ? srcSummary.weight : numVal(obj.weight, 0)
  form.concurrentRate = strVal(obj.concurrentRate)
  form.header = strVal(obj.header)
  form.bookUrlPattern = strVal(obj.bookUrlPattern)
  form.loginUrl = strVal(obj.loginUrl)
  form.loginUi = strVal(obj.loginUi)
  form.loginCheckJs = strVal(obj.loginCheckJs)
  form.jsLib = strVal(obj.jsLib)
  form.searchUrl = strVal(obj.searchUrl)
  form.exploreUrl = strVal(obj.exploreUrl)
  form.ruleSearch = readRuleMap(obj.ruleSearch, searchRuleFields)
  form.ruleExplore = readRuleMap(obj.ruleExplore, exploreRuleFields)
  form.ruleBookInfo = readRuleMap(obj.ruleBookInfo, bookInfoRuleFields)
  form.ruleToc = readRuleMap(obj.ruleToc, tocRuleFields)
  form.ruleContent = readRuleMap(obj.ruleContent, contentRuleFields)
  initialSnapshot = captureSnapshot()
}

function coerceEditedValue(original: unknown, edited: string): unknown {
  if (typeof original === 'boolean') {
    const lower = edited.trim().toLowerCase()
    if (lower === 'true') return true
    if (lower === 'false') return false
  } else if (typeof original === 'number') {
    const n = Number(edited.trim())
    if (edited.trim() !== '' && Number.isFinite(n)) return n
  } else if (original !== null && typeof original === 'object') {
    try {
      const parsed = JSON.parse(edited)
      if (parsed !== null && typeof parsed === 'object') {
        return parsed
      }
    } catch {
      // fall back to string
    }
  }
  return edited
}

function mergeRuleSection(
  baseRule: unknown,
  editedMap: Record<string, string>,
  initialMap: Record<string, string> | undefined,
  fields: RuleFieldDef[],
): Record<string, unknown> | undefined {
  const baseObj =
    baseRule && typeof baseRule === 'object' && !Array.isArray(baseRule)
      ? (baseRule as Record<string, unknown>)
      : null
  const merged: Record<string, unknown> = baseObj ? { ...baseObj } : {}

  for (const f of fields) {
    const val = editedMap[f.key] ?? ''
    const initVal = initialMap?.[f.key] ?? ''
    if (val === initVal) {
      // Unchanged from snapshot: preserve original value & type (or absence) losslessly
      continue
    }
    if (val.trim().length > 0) {
      merged[f.key] = coerceEditedValue(baseObj?.[f.key], val)
    } else {
      delete merged[f.key]
    }
  }
  return Object.keys(merged).length > 0 ? merged : undefined
}

function mergeOptionalField(
  target: Record<string, unknown>,
  key: string,
  edited: string,
  initial: string | undefined,
  trim = false,
): void {
  if (initial !== undefined && edited === initial) {
    return
  }
  const next = trim ? edited.trim() : edited
  if (next.trim().length > 0) {
    target[key] = coerceEditedValue(rawBaseObj.value[key], next)
  } else {
    delete target[key]
  }
}

function buildMergedObject(): Record<string, unknown> {
  const base = rawBaseObj.value
  const snap = initialSnapshot
  const out: Record<string, unknown> = { ...base }
  out.bookSourceName = form.bookSourceName.trim()
  out.bookSourceUrl = form.bookSourceUrl.trim()
  mergeOptionalField(out, 'bookSourceGroup', form.bookSourceGroup, snap?.bookSourceGroup, true)
  mergeOptionalField(out, 'bookSourceComment', form.bookSourceComment, snap?.bookSourceComment)

  if ('bookSourceType' in base || !snap || form.bookSourceType !== snap.bookSourceType) {
    out.bookSourceType = numVal(form.bookSourceType, 0)
  }
  if ('enabled' in base || !snap || form.enabled !== snap.enabled) {
    out.enabled = form.enabled
  }
  if ('enabledExplore' in base || !snap || form.enabledExplore !== snap.enabledExplore) {
    out.enabledExplore = form.enabledExplore
  }
  if ('enabledCookieJar' in base || !snap || form.enabledCookieJar !== snap.enabledCookieJar) {
    out.enabledCookieJar = form.enabledCookieJar
  }
  const normalizedWeight = numVal(form.weight, 0)
  if ('weight' in base || !snap || normalizedWeight !== snap.weight) {
    out.weight = normalizedWeight
  }

  mergeOptionalField(out, 'concurrentRate', form.concurrentRate, snap?.concurrentRate, true)
  mergeOptionalField(out, 'header', form.header, snap?.header)
  mergeOptionalField(out, 'bookUrlPattern', form.bookUrlPattern, snap?.bookUrlPattern, true)
  mergeOptionalField(out, 'loginUrl', form.loginUrl, snap?.loginUrl)
  mergeOptionalField(out, 'loginUi', form.loginUi, snap?.loginUi)
  mergeOptionalField(out, 'loginCheckJs', form.loginCheckJs, snap?.loginCheckJs)
  mergeOptionalField(out, 'jsLib', form.jsLib, snap?.jsLib)
  mergeOptionalField(out, 'searchUrl', form.searchUrl, snap?.searchUrl)
  mergeOptionalField(out, 'exploreUrl', form.exploreUrl, snap?.exploreUrl)

  const sections: Array<[string, Record<string, string>, Record<string, string> | undefined, RuleFieldDef[]]> = [
    ['ruleSearch', form.ruleSearch, snap?.ruleSearch, searchRuleFields],
    ['ruleExplore', form.ruleExplore, snap?.ruleExplore, exploreRuleFields],
    ['ruleBookInfo', form.ruleBookInfo, snap?.ruleBookInfo, bookInfoRuleFields],
    ['ruleToc', form.ruleToc, snap?.ruleToc, tocRuleFields],
    ['ruleContent', form.ruleContent, snap?.ruleContent, contentRuleFields],
  ]
  for (const [key, map, initMap, fields] of sections) {
    const merged = mergeRuleSection(base[key], map, initMap, fields)
    if (merged) {
      out[key] = merged
    } else {
      delete out[key]
    }
  }
  return out
}

const jsonValidation = computed<{ valid: boolean; error: string | null }>(() => {
  if (activeTab.value !== 'json' && !jsonDirty.value) {
    if (!form.bookSourceName.trim() && !form.bookSourceUrl.trim()) {
      return { valid: false, error: '书源名称与书源地址不能同时为空' }
    }
    return { valid: true, error: null }
  }
  const txt = jsonText.value.trim()
  if (!txt) return { valid: false, error: '书源 JSON 不能为空' }
  try {
    const parsed = JSON.parse(txt)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return { valid: false, error: '书源 JSON 必须是单个对象 {...}' }
    }
    const obj = parsed as Record<string, unknown>
    const name = strVal(obj.bookSourceName).trim()
    const url = strVal(obj.bookSourceUrl).trim()
    if (!name && !url) {
      return { valid: false, error: 'bookSourceName 与 bookSourceUrl 不能同时为空' }
    }
    return { valid: true, error: null }
  } catch (err) {
    return { valid: false, error: err instanceof Error ? err.message : 'JSON 语法错误' }
  }
})

function hasAnyRule(map: Record<string, string>): boolean {
  return Object.values(map).some((v) => v.trim().length > 0)
}

const tabItems = computed(() => [
  { id: 'basic' as EditTab, label: '基本', icon: LuInfo, activeDot: false },
  {
    id: 'search' as EditTab,
    label: '搜索',
    icon: LuSearch,
    activeDot: Boolean(form.searchUrl.trim() || hasAnyRule(form.ruleSearch)),
  },
  {
    id: 'explore' as EditTab,
    label: '发现',
    icon: LuCompass,
    activeDot: Boolean(form.exploreUrl.trim() || hasAnyRule(form.ruleExplore)),
  },
  {
    id: 'book_info' as EditTab,
    label: '详情',
    icon: LuBookOpen,
    activeDot: hasAnyRule(form.ruleBookInfo),
  },
  {
    id: 'toc' as EditTab,
    label: '目录',
    icon: LuList,
    activeDot: hasAnyRule(form.ruleToc),
  },
  {
    id: 'content' as EditTab,
    label: '正文',
    icon: LuFileText,
    activeDot: hasAnyRule(form.ruleContent),
  },
  { id: 'json' as EditTab, label: 'JSON', icon: LuCode2, activeDot: false },
])

function switchTab(next: EditTab): void {
  if (next === activeTab.value) return
  if (activeTab.value === 'json' && jsonDirty.value) {
    try {
      const parsed = JSON.parse(jsonText.value)
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        snackbar.show({ message: 'JSON 必须是单个对象，请先修正语法', tone: 'error' })
        return
      }
      hydrateFormFromObject(parsed as Record<string, unknown>)
      jsonDirty.value = false
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? `JSON 语法有误，无法切换：${err.message}` : 'JSON 语法有误',
        tone: 'error',
      })
      return
    }
  }
  if (next === 'json') {
    jsonText.value = JSON.stringify(buildMergedObject(), null, 2)
    jsonDirty.value = false
  }
  activeTab.value = next
}

async function loadSourceRaw(src: BookSourceSummaryDto): Promise<void> {
  const seq = ++loadSeq
  const targetId = src.id
  loading.value = true
  loadError.value = null
  activeTab.value = 'basic'
  clearFormState()
  try {
    const [obj, varRes] = await Promise.all([
      onlineBookApi.fetchSourceJson(targetId),
      onlineBookApi.getSourceVariable(targetId),
    ])
    if (seq !== loadSeq || !props.open || props.source?.id !== targetId) return
    hydrateFormFromObject(obj, src)
    const fullVar = varRes.variable ?? ''
    variableText.value = fullVar
    initialVariableText.value = fullVar
    jsonText.value = JSON.stringify(buildMergedObject(), null, 2)
  } catch (err) {
    if (seq !== loadSeq || !props.open || props.source?.id !== targetId) return
    clearFormState()
    loadError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === loadSeq) {
      loading.value = false
    }
  }
}

watch(
  () => [props.open, props.source?.id] as const,
  ([isOpen]) => {
    if (isOpen && props.source) {
      void loadSourceRaw(props.source)
    } else {
      loadSeq += 1
      loading.value = false
      loadError.value = null
      clearFormState()
    }
  },
  { immediate: true },
)

function formatJson(): void {
  try {
    const parsed = JSON.parse(jsonText.value)
    jsonText.value = JSON.stringify(parsed, null, 2)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? `格式化失败：${err.message}` : 'JSON 语法无效',
      tone: 'error',
    })
  }
}

async function copyJson(): Promise<void> {
  const txt =
    activeTab.value === 'json' ? jsonText.value : JSON.stringify(buildMergedObject(), null, 2)
  try {
    await navigator.clipboard.writeText(txt)
    snackbar.show({ message: '已复制书源 JSON' })
  } catch {
    snackbar.show({ message: '复制失败', tone: 'error' })
  }
}

async function saveSource(openDebugAfter = false): Promise<void> {
  if (!props.source || loading.value || saving.value || Boolean(loadError.value) || !jsonValidation.value.valid) return

  const targetId = props.source.id
  let finalObj: Record<string, unknown>
  if (activeTab.value === 'json' && jsonDirty.value) {
    try {
      finalObj = JSON.parse(jsonText.value) as Record<string, unknown>
      hydrateFormFromObject(finalObj)
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : 'JSON 语法错误',
        tone: 'error',
      })
      return
    }
  } else {
    finalObj = buildMergedObject()
  }

  saving.value = true
  try {
    const patch: {
      enabled?: boolean
      group_name?: string
      weight?: number
      variable?: string
      raw_json?: string
    } = {
      enabled: form.enabled,
      group_name: form.bookSourceGroup.trim(),
      weight: numVal(form.weight, 0),
      raw_json: JSON.stringify(finalObj),
    }
    if (variableText.value !== initialVariableText.value) {
      patch.variable = variableText.value
    }
    const updated = await onlineBookApi.updateSource(targetId, patch)
    snackbar.show({ message: `已保存书源「${updated.name}」` })
    if (openDebugAfter) {
      emit('savedAndDebug', updated)
    } else {
      emit('saved', updated)
      emit('close')
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MdSideSheet
    :open="open"
    :title="`编辑书源 · ${form.bookSourceName || source?.name || ''}`"
    :subtitle="form.bookSourceUrl || source?.source_url"
    width-class="w-[min(54rem,100vw)]"
    @close="emit('close')"
  >
    <template #subheader>
      <!-- MD3 Pill Tab Strip (modeled after legado-with-MD3 BookSourceEditActivity) -->
      <div class="flex flex-wrap items-center gap-1.5 border-b border-[var(--md-sys-color-outline-variant)]/55 pb-3">
        <button
          v-for="tab in tabItems"
          :key="tab.id"
          type="button"
          class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
          :class="
            activeTab === tab.id
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
          "
          @click="switchTab(tab.id)"
        >
          <component :is="tab.icon" class="h-3.5 w-3.5" />
          <span>{{ tab.label }}</span>
          <span
            v-if="tab.activeDot"
            class="h-1.5 w-1.5 rounded-full bg-[var(--md-sys-color-primary)]"
          />
        </button>
      </div>
    </template>

    <div
      v-if="loading"
      class="flex items-center justify-center gap-2 py-20 text-sm text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="h-4 w-4 animate-spin" />
      <span>正在加载书源规则…</span>
    </div>

    <div
      v-else-if="loadError"
      class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
    >
      {{ loadError }}
    </div>

    <div v-else class="space-y-4 pt-1">
      <!-- TAB 1: Basic Info & Request / Auth -->
      <template v-if="activeTab === 'basic'">
        <!-- Core Identity -->
        <section class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
          <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">基础信息</h4>
          <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                书源名称 <span class="font-mono opacity-65">bookSourceName</span>
              </span>
              <input
                v-model="form.bookSourceName"
                type="text"
                placeholder="必填"
                class="md-input w-full py-2 text-sm"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                书源分组 <span class="font-mono opacity-65">bookSourceGroup</span>
              </span>
              <input
                v-model="form.bookSourceGroup"
                type="text"
                placeholder="例如：精品、轻小说"
                class="md-input w-full py-2 text-sm"
              />
            </label>

            <label class="block space-y-1 sm:col-span-2">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                书源地址 <span class="font-mono opacity-65">bookSourceUrl</span>
              </span>
              <input
                v-model="form.bookSourceUrl"
                type="text"
                placeholder="https://example.com"
                class="md-input w-full py-2 font-mono text-xs"
              />
            </label>

            <div class="space-y-1">
              <span class="block text-xs text-[var(--md-sys-color-on-surface-variant)]">
                书源类型 <span class="font-mono opacity-65">bookSourceType</span>
              </span>
              <AppSelect
                v-model="form.bookSourceType"
                :options="sourceTypeOptions"
              />
            </div>

            <div class="grid grid-cols-2 gap-2.5">
              <label class="block space-y-1">
                <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">并发率</span>
                <input
                  v-model="form.concurrentRate"
                  type="text"
                  placeholder="如 10/1000 或 200"
                  class="md-input w-full py-2 font-mono text-xs"
                />
              </label>

              <label class="block space-y-1">
                <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">排序权重</span>
                <input
                  v-model.number="form.weight"
                  type="number"
                  class="md-input w-full py-2 font-mono text-xs"
                />
              </label>
            </div>

            <label class="block space-y-1 sm:col-span-2">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                书源注释 <span class="font-mono opacity-65">bookSourceComment</span>
              </span>
              <textarea
                v-model="form.bookSourceComment"
                rows="2"
                placeholder="作者说明、使用注意或更新记录…"
                class="md-input w-full py-2 text-xs leading-5"
              />
            </label>
          </div>

          <!-- Feature Switches -->
          <div class="flex flex-wrap items-center gap-x-6 gap-y-2.5 border-t border-[var(--md-sys-color-outline-variant)]/40 pt-3">
            <label class="inline-flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
              <AppSwitch v-model="form.enabled" />
              <span>启用书源</span>
            </label>
            <label class="inline-flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
              <AppSwitch v-model="form.enabledExplore" />
              <span>启用发现</span>
            </label>
            <label class="inline-flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
              <AppSwitch v-model="form.enabledCookieJar" />
              <span>启用 CookieJar</span>
            </label>
          </div>
        </section>

        <!-- Network & Auth -->
        <section class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
          <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            请求头、登录与脚本扩展
          </h4>
          <div class="grid grid-cols-1 gap-3">
            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                请求头 <span class="font-mono opacity-65">header</span>
              </span>
              <textarea
                v-model="form.header"
                rows="2"
                spellcheck="false"
                placeholder='例如：{"User-Agent":"Mozilla/5.0 ..."}'
                class="md-input w-full py-2 font-mono text-xs leading-5"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                书籍 URL 正则 <span class="font-mono opacity-65">bookUrlPattern</span>
              </span>
              <input
                v-model="form.bookUrlPattern"
                type="text"
                spellcheck="false"
                class="md-input w-full py-2 font-mono text-xs"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                登录地址 / 脚本 <span class="font-mono opacity-65">loginUrl</span>
              </span>
              <textarea
                v-model="form.loginUrl"
                rows="2"
                spellcheck="false"
                class="md-input w-full py-2 font-mono text-xs leading-5"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                登录界面定义 <span class="font-mono opacity-65">loginUi</span>
              </span>
              <textarea
                v-model="form.loginUi"
                rows="3"
                spellcheck="false"
                class="md-input w-full py-2 font-mono text-xs leading-5"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                登录检测 JS <span class="font-mono opacity-65">loginCheckJs</span>
              </span>
              <textarea
                v-model="form.loginCheckJs"
                rows="2"
                spellcheck="false"
                class="md-input w-full py-2 font-mono text-xs leading-5"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                JS 扩展库 <span class="font-mono opacity-65">jsLib</span>
              </span>
              <textarea
                v-model="form.jsLib"
                rows="3"
                spellcheck="false"
                class="md-input w-full py-2 font-mono text-xs leading-5"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                运行时变量 <span class="font-mono opacity-65">source.variable</span>
              </span>
              <input
                v-model="variableText"
                type="text"
                spellcheck="false"
                placeholder="可选运行时变量字符串"
                class="md-input w-full py-2 font-mono text-xs"
              />
            </label>
          </div>
        </section>
      </template>

      <!-- TAB 2: Search Rule -->
      <template v-else-if="activeTab === 'search'">
        <section class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
          <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            搜索地址 (searchUrl)
          </h4>
          <textarea
            v-model="form.searchUrl"
            rows="3"
            spellcheck="false"
            placeholder="例如：/search?q={{key}}&page={{page}} 或 @js:..."
            class="md-input w-full py-2 font-mono text-xs leading-5"
          />
        </section>

        <RuleFieldsSection
          v-model="form.ruleSearch"
          title="搜索结果解析规则 (ruleSearch)"
          :fields="searchRuleFields"
        />
      </template>

      <!-- TAB 3: Explore Rule -->
      <template v-else-if="activeTab === 'explore'">
        <section class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
          <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            发现地址 (exploreUrl)
          </h4>
          <textarea
            v-model="form.exploreUrl"
            rows="4"
            spellcheck="false"
            placeholder="分类名::/category/1/{{page}} 或 JSON 数组格式"
            class="md-input w-full py-2 font-mono text-xs leading-5"
          />
        </section>

        <RuleFieldsSection
          v-model="form.ruleExplore"
          title="发现结果解析规则 (ruleExplore)"
          :fields="exploreRuleFields"
        />
      </template>

      <!-- TAB 4: Book Info Rule -->
      <template v-else-if="activeTab === 'book_info'">
        <RuleFieldsSection
          v-model="form.ruleBookInfo"
          title="书籍详情解析规则 (ruleBookInfo)"
          :fields="bookInfoRuleFields"
        />
      </template>

      <!-- TAB 5: TOC Rule -->
      <template v-else-if="activeTab === 'toc'">
        <RuleFieldsSection
          v-model="form.ruleToc"
          title="目录列表解析规则 (ruleToc)"
          :fields="tocRuleFields"
        />
      </template>

      <!-- TAB 6: Content Rule -->
      <template v-else-if="activeTab === 'content'">
        <RuleFieldsSection
          v-model="form.ruleContent"
          title="正文内容解析规则 (ruleContent)"
          :fields="contentRuleFields"
        />
      </template>

      <!-- TAB 7: Raw JSON Source Mode -->
      <template v-else-if="activeTab === 'json'">
        <section class="space-y-2.5 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex items-center gap-2">
              <span class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
                完整 Legado JSON（与表单双向同步）
              </span>
              <span
                v-if="jsonValidation.valid"
                class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
              >
                <LuCheckCircle2 class="h-3 w-3" />
                <span>JSON 合法</span>
              </span>
              <span
                v-else
                class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-error-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-error-container)]"
              >
                <LuXCircle class="h-3 w-3" />
                <span>语法有误</span>
              </span>
            </div>

            <div class="flex items-center gap-1.5">
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="formatJson"
              >
                <LuCode2 class="h-3.5 w-3.5" />
                <span>格式化</span>
              </button>
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-outlined md-state"
                @click="copyJson"
              >
                <LuCopy class="h-3.5 w-3.5" />
                <span>复制</span>
              </button>
            </div>
          </div>

          <p
            v-if="!jsonValidation.valid && jsonValidation.error"
            class="text-[11px] text-[var(--md-sys-color-error)]"
          >
            {{ jsonValidation.error }}
          </p>

          <textarea
            v-model="jsonText"
            rows="22"
            spellcheck="false"
            class="md-input w-full py-2.5 font-mono text-xs leading-5"
            @input="jsonDirty = true"
          />
        </section>
      </template>
    </div>

    <template #footer>
      <button
        type="button"
        class="md-btn md-btn-text md-state"
        @click="emit('close')"
      >
        取消
      </button>
      <button
        type="button"
        :disabled="loading || saving || Boolean(loadError) || !jsonValidation.valid"
        class="md-btn md-btn-tonal md-state"
        @click="saveSource(true)"
      >
        <LuFlaskConical class="h-4 w-4" />
        <span>保存并调试</span>
      </button>
      <button
        type="button"
        :disabled="loading || saving || Boolean(loadError) || !jsonValidation.valid"
        class="md-btn md-btn-filled md-state"
        @click="saveSource(false)"
      >
        <LuLoader2 v-if="saving" class="h-4 w-4 animate-spin" />
        <LuSave v-else class="h-4 w-4" />
        <span>{{ saving ? '保存中…' : '保存' }}</span>
      </button>
    </template>
  </MdSideSheet>
</template>
