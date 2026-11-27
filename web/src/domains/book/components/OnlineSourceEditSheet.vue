<script setup lang="ts">
import { computed, reactive, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import OnlineSourceEditShell from '@/components/online-sources/OnlineSourceEditShell.vue'
import {
  hasAnyRule,
  numVal,
  strVal,
  useOnlineSourceEdit,
} from '@/components/online-sources/useOnlineSourceEdit'
import { MdSelect, MdSwitch, type SelectOption } from '@/components/ui'
import SourceAdvancedSection from '@/domains/shared/source-editor/SourceAdvancedSection.vue'
import SourceBasicSection from '@/domains/shared/source-editor/SourceBasicSection.vue'
import SourceRuleStagesAccordion from '@/domains/shared/source-editor/SourceRuleStagesAccordion.vue'
import { buildSourceEditTabs, type RuleStageItem } from '@/domains/shared/source-editor/types'
import {
  buildBookRuleFields,
  buildMergedBookSourceObject,
  createDefaultBookFormState,
  hydrateBookSourceForm,
  type BookSourceEditTab,
  type BookSourceSnapshot,
} from '../bookSourceSchema'
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

const sourceTypeOptions = computed<SelectOption[]>(() => [
  { value: '0', label: bt('sourceEditor.typeText') },
  { value: '1', label: bt('sourceEditor.typeAudio') },
  { value: '2', label: bt('sourceEditor.typeImage') },
  { value: '3', label: bt('sourceEditor.typeFile') },
])

const localizedRules = computed(() => buildBookRuleFields(bt))
const form = reactive(createDefaultBookFormState())
let initialSnapshot: BookSourceSnapshot | null = null

function clearFormState(): void {
  initialSnapshot = null
  Object.assign(form, createDefaultBookFormState())
}

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: BookSourceSummaryDto,
): void {
  initialSnapshot = hydrateBookSourceForm(form, obj, srcSummary)
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
  jsModule,
  jsonValidation,
  switchTab,
  formatJson,
  copyJson,
  saveSource,
} = useOnlineSourceEdit<BookSourceSummaryDto, BookSourceEditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: bookSourceAdapter,
  initialTab: 'basic',
  jsonTabId: 'json',
  clearFormState,
  hydrateFormFromObject,
  buildMergedObject: (): Record<string, unknown> =>
    buildMergedBookSourceObject(rawBaseObj.value, form, initialSnapshot),
  validateForm: () => {
    if (!form.bookSourceName.trim() && !form.bookSourceUrl.trim()) {
      return t('onlineSources.edit.nameAndUrlRequired')
    }
    return null
  },
  validateJsonObject: (obj) => {
    const name = strVal(obj.bookSourceName).trim()
    const url = strVal(obj.bookSourceUrl).trim()
    if (!name && !url) return t('onlineSources.edit.nameAndUrlRequired')
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

const ruleStages = computed<RuleStageItem[]>(() => [
  {
    id: 'search',
    title: bt('sourceEditor.stageSearch'),
    jsonKey: 'ruleSearch',
    fields: localizedRules.value.search,
    modelValue: form.ruleSearch,
    urlValue: form.searchUrl,
    urlLabel: bt('sourceEditor.searchUrlLabel'),
    urlJsonKey: 'searchUrl',
    urlPlaceholder: bt('sourceEditor.searchUrlPlaceholder'),
    onUpdateUrl: (val) => {
      form.searchUrl = val
    },
    onUpdateRules: (val) => {
      form.ruleSearch = val
    },
  },
  {
    id: 'explore',
    title: bt('sourceEditor.stageExplore'),
    jsonKey: 'ruleExplore',
    fields: localizedRules.value.explore,
    modelValue: form.ruleExplore,
    urlValue: form.exploreUrl,
    urlLabel: bt('sourceEditor.exploreUrlLabel'),
    urlJsonKey: 'exploreUrl',
    urlPlaceholder: bt('sourceEditor.exploreUrlPlaceholder'),
    urlRows: 4,
    onUpdateUrl: (val) => {
      form.exploreUrl = val
    },
    onUpdateRules: (val) => {
      form.ruleExplore = val
    },
  },
  {
    id: 'book_info',
    title: bt('sourceEditor.stageBookInfo'),
    jsonKey: 'ruleBookInfo',
    fields: localizedRules.value.bookInfo,
    modelValue: form.ruleBookInfo,
    onUpdateRules: (val) => {
      form.ruleBookInfo = val
    },
  },
  {
    id: 'toc',
    title: bt('sourceEditor.stageToc'),
    jsonKey: 'ruleToc',
    fields: localizedRules.value.toc,
    modelValue: form.ruleToc,
    onUpdateRules: (val) => {
      form.ruleToc = val
    },
  },
  {
    id: 'content',
    title: bt('sourceEditor.stageContent'),
    jsonKey: 'ruleContent',
    fields: localizedRules.value.content,
    modelValue: form.ruleContent,
    onUpdateRules: (val) => {
      form.ruleContent = val
    },
  },
])

const hasRulesConfigured = computed(() =>
  Boolean(
    form.searchUrl.trim() ||
      form.exploreUrl.trim() ||
      hasAnyRule(form.ruleSearch) ||
      hasAnyRule(form.ruleExplore) ||
      hasAnyRule(form.ruleBookInfo) ||
      hasAnyRule(form.ruleToc) ||
      hasAnyRule(form.ruleContent),
  ),
)

const hasAdvancedConfigured = computed(() =>
  Boolean(
    form.concurrentRate.trim() ||
      form.header.trim() ||
      form.bookUrlPattern.trim() ||
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
    rulesLabel: bt('sourceEditor.tabRules'),
    advancedLabel: bt('sourceEditor.tabAdvanced'),
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
        name: form.bookSourceName || source?.name || '',
      })
    "
    :subtitle="form.bookSourceUrl || source?.source_url"
    :domain-label="domainLabel"
    :tabs="tabItems"
    :active-tab="activeTab"
    json-tab-id="json"
    :loading="loading"
    :saving="saving"
    :load-error="loadError"
    :json-text="jsonText"
    :json-validation="jsonValidation"
    :js-module="jsModule"
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
      v-model:source-name="form.bookSourceName"
      v-model:source-group="form.bookSourceGroup"
      v-model:source-url="form.bookSourceUrl"
      v-model:weight="form.weight"
      v-model:source-comment="form.bookSourceComment"
      name-json-key="bookSourceName"
      group-json-key="bookSourceGroup"
      url-json-key="bookSourceUrl"
      comment-json-key="bookSourceComment"
    >
      <template #extra-fields>
        <div class="space-y-1">
          <span class="text-label-medium block text-on-surface-variant" title="bookSourceType">
            {{ t('onlineSources.edit.sourceTypeLabel') }}
          </span>
          <MdSelect v-model="form.bookSourceType" :options="sourceTypeOptions" size="sm" />
          <p class="text-body-small px-1 text-on-surface-variant">bookSourceType</p>
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
      v-model:url-pattern="form.bookUrlPattern"
      v-model:login-url="form.loginUrl"
      v-model:login-ui="form.loginUi"
      v-model:login-check-js="form.loginCheckJs"
      v-model:js-lib="form.jsLib"
      v-model:variable-text="variableText"
      has-url-pattern
      :url-pattern-label="bt('sourceEditor.bookUrlPatternLabel')"
      url-pattern-json-key="bookUrlPattern"
    />
  </OnlineSourceEditShell>
</template>
