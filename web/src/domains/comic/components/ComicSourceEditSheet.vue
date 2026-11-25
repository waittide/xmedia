<script setup lang="ts">
import { computed, reactive, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import OnlineSourceEditShell from '@/components/online-sources/OnlineSourceEditShell.vue'
import {
  hasAnyRule,
  numVal,
  pickFirstString,
  useOnlineSourceEdit,
} from '@/components/online-sources/useOnlineSourceEdit'
import { MdSelect, MdSwitch, type SelectOption } from '@/components/ui'
import SourceAdvancedSection from '@/domains/shared/source-editor/SourceAdvancedSection.vue'
import SourceBasicSection from '@/domains/shared/source-editor/SourceBasicSection.vue'
import SourceRuleStagesAccordion from '@/domains/shared/source-editor/SourceRuleStagesAccordion.vue'
import { buildSourceEditTabs, type RuleStageItem } from '@/domains/shared/source-editor/types'
import {
  buildComicRuleFields,
  buildMergedComicSourceObject,
  createDefaultComicFormState,
  createDefaultComicKeyMap,
  hydrateComicSourceForm,
  type ComicSourceEditTab,
  type ComicSourceSnapshot,
} from '../comicSourceSchema'
import { ensureComicLocale, useComicI18n } from '../i18n'
import { comicSourceAdapter, type ComicSourceSummaryDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: ComicSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: ComicSourceSummaryDto]
  savedAndDebug: [source: ComicSourceSummaryDto]
}>()

const { t } = useI18n()
const { t: ct } = useComicI18n()

void ensureComicLocale()
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void ensureComicLocale()
  },
)

const readingDirectionOptions = computed<SelectOption[]>(() => [
  { value: 'rtl', label: t('onlineSources.edit.directionRtl') },
  { value: 'ltr', label: t('onlineSources.edit.directionLtr') },
  { value: 'webtoon', label: t('onlineSources.edit.directionWebtoon') },
])

const localizedRules = computed(() => buildComicRuleFields(ct))
const keyMap = reactive(createDefaultComicKeyMap())
const form = reactive(createDefaultComicFormState())
let initialSnapshot: ComicSourceSnapshot | null = null

function clearFormState(): void {
  initialSnapshot = null
  Object.assign(keyMap, createDefaultComicKeyMap())
  Object.assign(form, createDefaultComicFormState())
}

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: ComicSourceSummaryDto,
): void {
  initialSnapshot = hydrateComicSourceForm(form, keyMap, obj, srcSummary)
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
} = useOnlineSourceEdit<ComicSourceSummaryDto, ComicSourceEditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: comicSourceAdapter,
  initialTab: 'basic',
  jsonTabId: 'json',
  clearFormState,
  hydrateFormFromObject,
  buildMergedObject: (): Record<string, unknown> =>
    buildMergedComicSourceObject(rawBaseObj.value, form, keyMap, initialSnapshot),
  validateForm: () => {
    if (!form.sourceName.trim() && !form.sourceUrl.trim()) {
      return t('onlineSources.edit.nameAndUrlRequired')
    }
    return null
  },
  validateJsonObject: (obj) => {
    const name = pickFirstString(obj, ['sourceName', 'bookSourceName']).trim()
    const url = pickFirstString(obj, ['sourceUrl', 'bookSourceUrl']).trim()
    if (!name && !url) return t('onlineSources.edit.nameAndUrlRequired')
    return null
  },
  getGroupName: () => form.sourceGroup,
  getEnabled: () => form.enabled,
  getWeight: () => numVal(form.weight, 0),
  onSaved: (updated) => emit('saved', updated),
  onSavedAndDebug: (updated) => emit('savedAndDebug', updated),
  onClose: () => emit('close'),
})

const domainLabel = computed(() => t('onlineSources.domainLabel.comic'))

const ruleStages = computed<RuleStageItem[]>(() => [
  {
    id: 'search',
    title: ct('sourceEditor.stageSearch'),
    jsonKey: 'ruleSearch',
    fields: localizedRules.value.search,
    modelValue: form.ruleSearch,
    urlValue: form.searchUrl,
    urlLabel: ct('sourceEditor.searchUrlLabel'),
    urlJsonKey: 'searchUrl',
    onUpdateUrl: (val) => {
      form.searchUrl = val
    },
    onUpdateRules: (val) => {
      form.ruleSearch = val
    },
  },
  {
    id: 'explore',
    title: ct('sourceEditor.stageExplore'),
    jsonKey: 'ruleExplore',
    fields: localizedRules.value.explore,
    modelValue: form.ruleExplore,
    urlValue: form.exploreUrl,
    urlLabel: ct('sourceEditor.exploreUrlLabel'),
    urlJsonKey: 'exploreUrl',
    urlRows: 4,
    onUpdateUrl: (val) => {
      form.exploreUrl = val
    },
    onUpdateRules: (val) => {
      form.ruleExplore = val
    },
  },
  {
    id: 'detail',
    title: ct('sourceEditor.stageDetail'),
    jsonKey: keyMap.detailRuleKey,
    fields: localizedRules.value.detail,
    modelValue: form.ruleDetail,
    onUpdateRules: (val) => {
      form.ruleDetail = val
    },
  },
  {
    id: 'toc',
    title: ct('sourceEditor.stageToc'),
    jsonKey: keyMap.tocRuleKey,
    fields: localizedRules.value.toc,
    modelValue: form.ruleToc,
    onUpdateRules: (val) => {
      form.ruleToc = val
    },
  },
  {
    id: 'pages',
    title: ct('sourceEditor.stagePages'),
    jsonKey: keyMap.pagesRuleKey,
    fields: localizedRules.value.pages,
    modelValue: form.rulePages,
    onUpdateRules: (val) => {
      form.rulePages = val
    },
  },
])

const hasRulesConfigured = computed(() =>
  Boolean(
    form.searchUrl.trim() ||
      form.exploreUrl.trim() ||
      hasAnyRule(form.ruleSearch) ||
      hasAnyRule(form.ruleExplore) ||
      hasAnyRule(form.ruleDetail) ||
      hasAnyRule(form.ruleToc) ||
      hasAnyRule(form.rulePages),
  ),
)

const hasAdvancedConfigured = computed(() =>
  Boolean(
    form.concurrentRate.trim() ||
      form.header.trim() ||
      form.imageHeader.trim() ||
      form.comicUrlPattern.trim() ||
      form.loginUrl.trim() ||
      form.loginUi.trim() ||
      form.loginCheckJs.trim() ||
      form.jsLib.trim() ||
      variableText.value.trim(),
  ),
)

const tabItems = computed(() =>
  buildSourceEditTabs({
    basicLabel: t('onlineSources.edit.tabBasic'),
    rulesLabel: ct('sourceEditor.tabRules'),
    advancedLabel: ct('sourceEditor.tabAdvanced'),
    jsonLabel: t('onlineSources.edit.tabJson'),
    hasRulesConfigured: hasRulesConfigured.value,
    hasAdvancedConfigured: hasAdvancedConfigured.value,
  }),
)
</script>

<template>
  <OnlineSourceEditShell
    :open="open"
    :title="
      t('onlineSources.edit.title', {
        domain: domainLabel,
        name: form.sourceName || source?.name || '',
      })
    "
    :subtitle="form.sourceUrl || source?.source_url"
    :domain-label="domainLabel"
    :tabs="tabItems"
    :active-tab="activeTab"
    json-tab-id="json"
    :loading="loading"
    :saving="saving"
    :load-error="loadError"
    :json-text="jsonText"
    :json-validation="jsonValidation"
    @close="emit('close')"
    @switch-tab="switchTab"
    @update:json-text="jsonText = $event"
    @mark-json-dirty="jsonDirty = true"
    @format-json="formatJson"
    @copy-json="copyJson"
    @save="saveSource"
  >
    <SourceBasicSection
      v-if="activeTab === 'basic'"
      v-model:source-name="form.sourceName"
      v-model:source-group="form.sourceGroup"
      v-model:source-url="form.sourceUrl"
      v-model:weight="form.weight"
      v-model:source-comment="form.sourceComment"
      :name-json-key="keyMap.nameKey"
      :group-json-key="keyMap.groupKey"
      :url-json-key="keyMap.urlKey"
      :comment-json-key="keyMap.commentKey"
    >
      <template #extra-fields>
        <div class="space-y-1">
          <span class="text-label-medium block text-on-surface-variant" title="readingDirection">
            {{ t('onlineSources.edit.readingDirectionLabel') }}
          </span>
          <MdSelect v-model="form.readingDirection" :options="readingDirectionOptions" size="sm" />
          <p class="text-body-small px-1 text-on-surface-variant">readingDirection</p>
        </div>
      </template>

      <template #switches>
        <MdSwitch v-model="form.enabled" :label="t('onlineSources.edit.enabledSwitch')" />
        <MdSwitch
          v-model="form.enabledExplore"
          :label="t('onlineSources.edit.enabledExploreSwitch')"
        />
        <MdSwitch
          v-model="form.enabledCookieJar"
          :label="t('onlineSources.edit.enabledCookieJarSwitch')"
        />
      </template>
    </SourceBasicSection>

    <SourceRuleStagesAccordion v-else-if="activeTab === 'rules'" :stages="ruleStages" />

    <SourceAdvancedSection
      v-else-if="activeTab === 'advanced'"
      v-model:concurrent-rate="form.concurrentRate"
      v-model:header="form.header"
      v-model:secondary-header="form.imageHeader"
      v-model:url-pattern="form.comicUrlPattern"
      v-model:login-url="form.loginUrl"
      v-model:login-ui="form.loginUi"
      v-model:login-check-js="form.loginCheckJs"
      v-model:js-lib="form.jsLib"
      v-model:variable-text="variableText"
      has-secondary-header
      :secondary-header-label="t('onlineSources.edit.imageHeaderLabel')"
      secondary-header-json-key="imageHeader"
      has-url-pattern
      :url-pattern-label="t('onlineSources.edit.comicUrlPatternLabel')"
      :url-pattern-json-key="keyMap.patternKey"
    />
  </OnlineSourceEditShell>
</template>
