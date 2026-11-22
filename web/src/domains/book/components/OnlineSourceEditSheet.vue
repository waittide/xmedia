<script setup lang="ts">
import { computed, reactive, ref, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuCheckCircle2,
  LuChevronDown,
  LuCode2,
  LuCompass,
  LuCopy,
  LuFileText,
  LuFlaskConical,
  LuInfo,
  LuList,
  LuSave,
  LuSearch,
  LuXCircle,
} from 'vue-icons-plus/lu'
import RuleFieldsSection, { type RuleFieldDef } from '@/components/online-sources/RuleFieldsSection.vue'
import {
  boolVal,
  hasAnyRule,
  mergeOptionalField,
  mergeRuleSection,
  numVal,
  readRuleMap,
  strVal,
  useOnlineSourceEdit,
} from '@/components/online-sources/useOnlineSourceEdit'
import {
  MdButton,
  MdCircularProgress,
  MdDialog,
  MdSelect,
  MdSwitch,
  MdTabs,
  type MdTabItem,
  type SelectOption,
} from '@/components/ui'
import { ensureBookLocale, useBookI18n } from '../i18n'
import { bookSourceAdapter, type BookSourceSummaryDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: BookSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: BookSourceSummaryDto]
  savedAndDebug: [source: BookSourceSummaryDto]
}>()

const { t } = useI18n()
const { t: bt } = useBookI18n()

void ensureBookLocale()
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void ensureBookLocale()
  },
)

type EditTab = 'basic' | 'search' | 'explore' | 'book_info' | 'toc' | 'content' | 'json'

const sourceTypeOptions = computed<SelectOption[]>(() => [
  { value: '0', label: bt('sourceEditor.typeText') },
  { value: '1', label: bt('sourceEditor.typeAudio') },
  { value: '2', label: bt('sourceEditor.typeImage') },
  { value: '3', label: bt('sourceEditor.typeFile') },
])

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
const advancedExpanded = ref(false)

function clearFormState(): void {
  initialSnapshot = null
  advancedExpanded.value = false
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

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: BookSourceSummaryDto,
): void {
  form.bookSourceName = strVal(obj.bookSourceName) || srcSummary?.name || ''
  form.bookSourceUrl = strVal(obj.bookSourceUrl) || srcSummary?.source_url || ''
  form.bookSourceGroup =
    obj.bookSourceGroup !== undefined
      ? strVal(obj.bookSourceGroup)
      : (srcSummary?.group_name ?? '')
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

const {
  activeTab,
  loading,
  saving,
  loadError,
  rawBaseObj,
  jsonText,
  jsonDirty,
  variableText,
  jsonValidation,
  switchTab,
  formatJson,
  copyJson,
  saveSource,
} = useOnlineSourceEdit<BookSourceSummaryDto, EditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: bookSourceAdapter,
  initialTab: 'basic',
  jsonTabId: 'json',
  clearFormState,
  hydrateFormFromObject,
  buildMergedObject: () => {
    const base = rawBaseObj.value
    const snap = initialSnapshot
    const out: Record<string, unknown> = { ...base }
    out.bookSourceName = form.bookSourceName.trim()
    out.bookSourceUrl = form.bookSourceUrl.trim()
    mergeOptionalField(base, out, 'bookSourceGroup', form.bookSourceGroup, snap?.bookSourceGroup, true)
    mergeOptionalField(base, out, 'bookSourceComment', form.bookSourceComment, snap?.bookSourceComment)

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

    mergeOptionalField(base, out, 'concurrentRate', form.concurrentRate, snap?.concurrentRate, true)
    mergeOptionalField(base, out, 'header', form.header, snap?.header)
    mergeOptionalField(base, out, 'bookUrlPattern', form.bookUrlPattern, snap?.bookUrlPattern, true)
    mergeOptionalField(base, out, 'loginUrl', form.loginUrl, snap?.loginUrl)
    mergeOptionalField(base, out, 'loginUi', form.loginUi, snap?.loginUi)
    mergeOptionalField(base, out, 'loginCheckJs', form.loginCheckJs, snap?.loginCheckJs)
    mergeOptionalField(base, out, 'jsLib', form.jsLib, snap?.jsLib)
    mergeOptionalField(base, out, 'searchUrl', form.searchUrl, snap?.searchUrl)
    mergeOptionalField(base, out, 'exploreUrl', form.exploreUrl, snap?.exploreUrl)

    const sections: Array<
      [string, Record<string, string>, Record<string, string> | undefined, RuleFieldDef[]]
    > = [
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
  },
  validateForm: () => {
    if (!form.bookSourceName.trim() && !form.bookSourceUrl.trim()) {
      return t('onlineSources.edit.nameAndUrlRequired')
    }
    return null
  },
  validateJsonObject: (obj) => {
    const name = strVal(obj.bookSourceName).trim()
    const url = strVal(obj.bookSourceUrl).trim()
    if (!name && !url) {
      return t('onlineSources.edit.nameAndUrlRequired')
    }
    return null
  },
  getGroupName: () => form.bookSourceGroup,
  getEnabled: () => form.enabled,
  getWeight: () => numVal(form.weight, 0),
  onSaved: (updated) => emit('saved', updated),
  onSavedAndDebug: (updated) => emit('savedAndDebug', updated),
  onClose: () => emit('close'),
})

const domainLabel = computed(() => t('onlineSources.domainLabel.book'))

const advancedFieldCount = computed(() => {
  const fields = [
    form.header,
    form.bookUrlPattern,
    form.loginUrl,
    form.loginUi,
    form.loginCheckJs,
    form.jsLib,
    variableText.value,
  ]
  return fields.filter((val) => val.trim().length > 0).length
})

const tabItems = computed<MdTabItem<EditTab>[]>(() => [
  { value: 'basic', label: t('onlineSources.edit.tabBasic'), icon: LuInfo },
  {
    value: 'search',
    label: t('onlineSources.edit.tabSearch'),
    icon: LuSearch,
    badge: form.searchUrl.trim() || hasAnyRule(form.ruleSearch) ? '•' : undefined,
  },
  {
    value: 'explore',
    label: t('onlineSources.edit.tabExplore'),
    icon: LuCompass,
    badge: form.exploreUrl.trim() || hasAnyRule(form.ruleExplore) ? '•' : undefined,
  },
  {
    value: 'book_info',
    label: t('onlineSources.edit.tabDetail'),
    icon: LuBookOpen,
    badge: hasAnyRule(form.ruleBookInfo) ? '•' : undefined,
  },
  {
    value: 'toc',
    label: t('onlineSources.edit.tabToc'),
    icon: LuList,
    badge: hasAnyRule(form.ruleToc) ? '•' : undefined,
  },
  {
    value: 'content',
    label: bt('sourceEditor.tabContent'),
    icon: LuFileText,
    badge: hasAnyRule(form.ruleContent) ? '•' : undefined,
  },
  { value: 'json', label: t('onlineSources.edit.tabJson'), icon: LuCode2 },
])

const canSave = computed(
  () => !loading.value && !saving.value && !loadError.value && jsonValidation.value.valid,
)
</script>

<template>
  <MdDialog
    :open="open"
    :title="
      t('onlineSources.edit.title', {
        domain: domainLabel,
        name: form.bookSourceName || source?.name || '',
      })
    "
    :subtitle="form.bookSourceUrl || source?.source_url"
    fullscreen
    @close="emit('close')"
  >
    <div class="mx-auto flex w-full max-w-5xl flex-col gap-4">
      <MdTabs
        :model-value="activeTab"
        :tabs="tabItems"
        variant="primary"
        @update:model-value="switchTab"
      />

      <!-- Loading -->
      <div
        v-if="loading"
        class="flex items-center justify-center gap-3 py-20 text-body-medium text-on-surface-variant"
      >
        <MdCircularProgress indeterminate :size="24" :stroke-width="3" />
        <span>{{ t('onlineSources.edit.loading', { domain: domainLabel }) }}</span>
      </div>

      <!-- Load Error -->
      <div
        v-else-if="loadError"
        role="alert"
        class="rounded-corner-md bg-error-container px-4 py-3 text-body-small text-on-error-container"
      >
        {{ loadError }}
      </div>

      <!-- Main Tab Panels -->
      <div v-else class="flex flex-col gap-4">
        <!-- TAB 1: Basic Info & Collapsible Advanced Network/Auth -->
        <template v-if="activeTab === 'basic'">
          <section class="flex flex-col gap-4 rounded-corner-lg bg-surface-container p-4 sm:p-5">
            <h4 class="text-title-small text-on-surface">
              {{ t('onlineSources.edit.basicSectionTitle') }}
            </h4>

            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.nameLabel') }}
                  <span class="font-mono opacity-65">bookSourceName</span>
                </span>
                <input
                  v-model="form.bookSourceName"
                  type="text"
                  class="md-input w-full py-2 text-body-medium"
                />
              </label>

              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.groupLabel') }}
                  <span class="font-mono opacity-65">bookSourceGroup</span>
                </span>
                <input
                  v-model="form.bookSourceGroup"
                  type="text"
                  :placeholder="t('onlineSources.edit.groupPlaceholder')"
                  class="md-input w-full py-2 text-body-medium"
                />
              </label>

              <label class="flex flex-col gap-1 sm:col-span-2">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.urlLabel') }}
                  <span class="font-mono opacity-65">bookSourceUrl</span>
                </span>
                <input
                  v-model="form.bookSourceUrl"
                  type="text"
                  :placeholder="t('onlineSources.edit.urlPlaceholder')"
                  class="md-input w-full py-2 font-mono text-body-small"
                />
              </label>

              <div class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.sourceTypeLabel') }}
                  <span class="font-mono opacity-65">bookSourceType</span>
                </span>
                <MdSelect
                  v-model="form.bookSourceType"
                  :options="sourceTypeOptions"
                  size="sm"
                />
              </div>

              <div class="grid grid-cols-2 gap-2.5">
                <label class="flex flex-col gap-1">
                  <span class="text-label-medium text-on-surface-variant">
                    {{ t('onlineSources.edit.concurrentRateLabel') }}
                  </span>
                  <input
                    v-model="form.concurrentRate"
                    type="text"
                    :placeholder="t('onlineSources.edit.concurrentRatePlaceholder')"
                    class="md-input w-full py-2 font-mono text-body-small"
                  />
                </label>

                <label class="flex flex-col gap-1">
                  <span class="text-label-medium text-on-surface-variant">
                    {{ t('onlineSources.edit.weightLabel') }}
                  </span>
                  <input
                    v-model.number="form.weight"
                    type="number"
                    class="md-input w-full py-2 font-mono text-body-small"
                  />
                </label>
              </div>

              <label class="flex flex-col gap-1 sm:col-span-2">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.commentLabel') }}
                  <span class="font-mono opacity-65">bookSourceComment</span>
                </span>
                <textarea
                  v-model="form.bookSourceComment"
                  rows="2"
                  :placeholder="t('onlineSources.edit.commentPlaceholder')"
                  class="md-input w-full py-2 text-body-small leading-5"
                />
              </label>
            </div>

            <!-- Feature Switches -->
            <div class="flex flex-wrap items-center gap-x-6 gap-y-2 border-t border-outline-variant/50 pt-3">
              <MdSwitch
                v-model="form.enabled"
                :label="t('onlineSources.edit.enabledSwitch')"
              />
              <MdSwitch
                v-model="form.enabledExplore"
                :label="t('onlineSources.edit.enabledExploreSwitch')"
              />
              <MdSwitch
                v-model="form.enabledCookieJar"
                :label="t('onlineSources.edit.enabledCookieJarSwitch')"
              />
            </div>
          </section>

          <!-- Collapsible Advanced: Network, Auth & Scripts -->
          <section class="overflow-hidden rounded-corner-lg bg-surface-container">
            <button
              type="button"
              class="md-state flex w-full cursor-pointer items-center justify-between gap-3 px-4 py-3.5 text-left sm:px-5"
              :aria-expanded="advancedExpanded"
              @click="advancedExpanded = !advancedExpanded"
            >
              <div class="flex min-w-0 items-center gap-2">
                <span class="text-title-small text-on-surface">
                  {{ bt('sourceEditor.advancedNetworkTitle') }}
                </span>
                <span
                  v-if="advancedFieldCount > 0"
                  class="text-label-small rounded-corner-full bg-secondary-container px-2 py-0.5 tabular-nums text-on-secondary-container"
                >
                  {{ advancedFieldCount }}
                </span>
              </div>
              <LuChevronDown
                class="h-4 w-4 shrink-0 text-on-surface-variant transition-transform duration-200 ease-standard"
                :class="advancedExpanded ? 'rotate-180' : ''"
                aria-hidden="true"
              />
            </button>

            <div
              v-if="advancedExpanded"
              class="flex flex-col gap-3 border-t border-outline-variant/50 px-4 pt-4 pb-5 sm:px-5"
            >
              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.headerLabel') }}
                  <span class="font-mono opacity-65">header</span>
                </span>
                <textarea
                  v-model="form.header"
                  rows="2"
                  spellcheck="false"
                  :placeholder="t('onlineSources.edit.headerPlaceholder')"
                  class="md-input w-full py-2 font-mono text-body-small leading-5"
                />
              </label>

              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ bt('sourceEditor.bookUrlPatternLabel') }}
                  <span class="font-mono opacity-65">bookUrlPattern</span>
                </span>
                <input
                  v-model="form.bookUrlPattern"
                  type="text"
                  spellcheck="false"
                  class="md-input w-full py-2 font-mono text-body-small"
                />
              </label>

              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.loginUrlLabel') }}
                  <span class="font-mono opacity-65">loginUrl</span>
                </span>
                <textarea
                  v-model="form.loginUrl"
                  rows="2"
                  spellcheck="false"
                  class="md-input w-full py-2 font-mono text-body-small leading-5"
                />
              </label>

              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.loginUiLabel') }}
                  <span class="font-mono opacity-65">loginUi</span>
                </span>
                <textarea
                  v-model="form.loginUi"
                  rows="3"
                  spellcheck="false"
                  class="md-input w-full py-2 font-mono text-body-small leading-5"
                />
              </label>

              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.loginCheckJsLabel') }}
                  <span class="font-mono opacity-65">loginCheckJs</span>
                </span>
                <textarea
                  v-model="form.loginCheckJs"
                  rows="2"
                  spellcheck="false"
                  class="md-input w-full py-2 font-mono text-body-small leading-5"
                />
              </label>

              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.jsLibLabel') }}
                  <span class="font-mono opacity-65">jsLib</span>
                </span>
                <textarea
                  v-model="form.jsLib"
                  rows="3"
                  spellcheck="false"
                  class="md-input w-full py-2 font-mono text-body-small leading-5"
                />
              </label>

              <label class="flex flex-col gap-1">
                <span class="text-label-medium text-on-surface-variant">
                  {{ t('onlineSources.edit.variableLabel') }}
                  <span class="font-mono opacity-65">source.variable</span>
                </span>
                <input
                  v-model="variableText"
                  type="text"
                  spellcheck="false"
                  :placeholder="t('onlineSources.edit.variablePlaceholder')"
                  class="md-input w-full py-2 font-mono text-body-small"
                />
              </label>
            </div>
          </section>
        </template>

        <!-- TAB 2: Search Rule -->
        <template v-else-if="activeTab === 'search'">
          <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4 sm:p-5">
            <h4 class="text-title-small text-on-surface">
              {{ t('onlineSources.edit.searchUrlLabel') }}
            </h4>
            <textarea
              v-model="form.searchUrl"
              rows="3"
              spellcheck="false"
              :placeholder="bt('sourceEditor.searchUrlPlaceholder')"
              class="md-input w-full py-2 font-mono text-body-small leading-5"
            />
          </section>

          <RuleFieldsSection
            v-model="form.ruleSearch"
            :title="bt('sourceEditor.ruleSearchTitle')"
            :fields="searchRuleFields"
          />
        </template>

        <!-- TAB 3: Explore Rule -->
        <template v-else-if="activeTab === 'explore'">
          <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4 sm:p-5">
            <h4 class="text-title-small text-on-surface">
              {{ t('onlineSources.edit.exploreUrlLabel') }}
            </h4>
            <textarea
              v-model="form.exploreUrl"
              rows="4"
              spellcheck="false"
              :placeholder="bt('sourceEditor.exploreUrlPlaceholder')"
              class="md-input w-full py-2 font-mono text-body-small leading-5"
            />
          </section>

          <RuleFieldsSection
            v-model="form.ruleExplore"
            :title="bt('sourceEditor.ruleExploreTitle')"
            :fields="exploreRuleFields"
          />
        </template>

        <!-- TAB 4: Book Info Rule -->
        <template v-else-if="activeTab === 'book_info'">
          <RuleFieldsSection
            v-model="form.ruleBookInfo"
            :title="bt('sourceEditor.ruleBookInfoTitle')"
            :fields="bookInfoRuleFields"
          />
        </template>

        <!-- TAB 5: TOC Rule -->
        <template v-else-if="activeTab === 'toc'">
          <RuleFieldsSection
            v-model="form.ruleToc"
            :title="bt('sourceEditor.ruleTocTitle')"
            :fields="tocRuleFields"
          />
        </template>

        <!-- TAB 6: Content Rule -->
        <template v-else-if="activeTab === 'content'">
          <RuleFieldsSection
            v-model="form.ruleContent"
            :title="bt('sourceEditor.ruleContentTitle')"
            :fields="contentRuleFields"
          />
        </template>

        <!-- TAB 7: Raw JSON Source Mode -->
        <template v-else-if="activeTab === 'json'">
          <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4 sm:p-5">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="flex flex-wrap items-center gap-2">
                <span class="text-title-small text-on-surface">
                  {{ t('onlineSources.edit.jsonTitle') }}
                </span>
                <span
                  v-if="jsonValidation.valid"
                  class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-success-container px-2.5 py-0.5 text-on-success-container"
                >
                  <LuCheckCircle2 class="h-3.5 w-3.5" aria-hidden="true" />
                  <span>{{ t('onlineSources.edit.jsonValid') }}</span>
                </span>
                <span
                  v-else
                  class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-error-container px-2.5 py-0.5 text-on-error-container"
                >
                  <LuXCircle class="h-3.5 w-3.5" aria-hidden="true" />
                  <span>{{ t('onlineSources.edit.jsonInvalid') }}</span>
                </span>
              </div>

              <div class="flex items-center gap-2">
                <MdButton
                  variant="tonal"
                  size="sm"
                  :icon="LuCode2"
                  @click="formatJson"
                >
                  {{ t('onlineSources.edit.formatJsonBtn') }}
                </MdButton>
                <MdButton
                  variant="outlined"
                  size="sm"
                  :icon="LuCopy"
                  @click="copyJson"
                >
                  {{ t('onlineSources.edit.copyJsonBtn') }}
                </MdButton>
              </div>
            </div>

            <p
              v-if="!jsonValidation.valid && jsonValidation.error"
              role="alert"
              class="text-body-small text-error"
            >
              {{ jsonValidation.error }}
            </p>

            <textarea
              v-model="jsonText"
              rows="22"
              spellcheck="false"
              class="md-input w-full py-2.5 font-mono text-body-small leading-5"
              @input="jsonDirty = true"
            />
          </section>
        </template>
      </div>
    </div>

    <template #footer>
      <MdButton
        variant="text"
        :disabled="saving"
        @click="emit('close')"
      >
        {{ t('common.cancel') }}
      </MdButton>
      <MdButton
        variant="tonal"
        :icon="LuFlaskConical"
        :disabled="!canSave"
        @click="saveSource(true)"
      >
        {{ t('onlineSources.edit.saveAndDebugBtn') }}
      </MdButton>
      <MdButton
        variant="filled"
        :icon="LuSave"
        :loading="saving"
        :disabled="!canSave"
        @click="saveSource(false)"
      >
        {{ saving ? t('onlineSources.edit.savingBtn') : t('onlineSources.edit.saveBtn') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
