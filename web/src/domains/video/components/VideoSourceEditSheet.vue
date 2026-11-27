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
import { MdSelect, MdSwitch, MdTextField, type SelectOption } from '@/components/ui'
import SourceAdvancedSection from '@/domains/shared/source-editor/SourceAdvancedSection.vue'
import SourceBasicSection from '@/domains/shared/source-editor/SourceBasicSection.vue'
import SourceRuleStagesAccordion from '@/domains/shared/source-editor/SourceRuleStagesAccordion.vue'
import { buildSourceEditTabs, type RuleStageItem } from '@/domains/shared/source-editor/types'
import { ensureVideoLocale, useVideoI18n } from '../i18n'
import { videoSourceAdapter, type VideoSourceSummaryDto } from '../onlineApi'
import {
  buildMergedVideoSourceObject,
  buildVideoRuleFields,
  createDefaultFormState,
  createDefaultKeyMap,
  hydrateVideoSourceForm,
  type VideoSourceEditTab,
  type VideoSourceSnapshot,
} from '../videoSourceSchema'

const props = defineProps<{
  open: boolean
  source: VideoSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: VideoSourceSummaryDto]
  savedAndDebug: [source: VideoSourceSummaryDto]
}>()

const { t } = useI18n()
const { t: vt } = useVideoI18n()

void ensureVideoLocale()
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void ensureVideoLocale()
  },
)

const formatOptions = computed<SelectOption[]>(() => [
  { value: 'applecms_v10', label: t('onlineSources.edit.formatApplecms') },
  { value: 'rule', label: t('onlineSources.edit.formatRule') },
])

const localizedRules = computed(() => buildVideoRuleFields(vt))
const keyMap = reactive(createDefaultKeyMap())
const form = reactive(createDefaultFormState())
let initialSnapshot: VideoSourceSnapshot | null = null

function clearFormState(): void {
  initialSnapshot = null
  Object.assign(keyMap, createDefaultKeyMap())
  Object.assign(form, createDefaultFormState())
}

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: VideoSourceSummaryDto,
): void {
  initialSnapshot = hydrateVideoSourceForm(form, keyMap, obj, srcSummary)
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
} = useOnlineSourceEdit<VideoSourceSummaryDto, VideoSourceEditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: videoSourceAdapter,
  initialTab: 'basic',
  jsonTabId: 'json',
  clearFormState,
  hydrateFormFromObject,
  buildMergedObject: (): Record<string, unknown> =>
    buildMergedVideoSourceObject(rawBaseObj.value, form, keyMap, initialSnapshot),
  validateForm: () => {
    if (!form.sourceName.trim() && !form.sourceUrl.trim()) {
      return t('onlineSources.edit.nameAndUrlRequired')
    }
    return null
  },
  validateJsonObject: (obj) => {
    const name = strVal(obj.sourceName).trim()
    const url = strVal(obj.sourceUrl).trim()
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

const isApplecms = computed(() => form.sourceFormat === 'applecms_v10')
const domainLabel = computed(() => t('onlineSources.domainLabel.video'))

const ruleStages = computed<RuleStageItem[]>(() => [
  {
    id: 'categories',
    title: vt('sourceEditor.stageCategories'),
    jsonKey: 'ruleCategories',
    fields: localizedRules.value.categories,
    modelValue: form.ruleCategories,
    urlValue: form.categoriesUrl,
    urlLabel: vt('sourceEditor.categoriesUrlLabel'),
    urlJsonKey: keyMap.categoriesUrlKey,
    onUpdateUrl: (val) => {
      form.categoriesUrl = val
    },
    onUpdateRules: (val) => {
      form.ruleCategories = val
    },
  },
  {
    id: 'search',
    title: vt('sourceEditor.stageSearch'),
    jsonKey: 'ruleSearch',
    fields: localizedRules.value.search,
    modelValue: form.ruleSearch,
    urlValue: form.searchUrl,
    urlLabel: vt('sourceEditor.searchUrlLabel'),
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
    title: vt('sourceEditor.stageExplore'),
    jsonKey: 'ruleExplore',
    fields: localizedRules.value.search,
    modelValue: form.ruleExplore,
    urlValue: form.exploreUrl,
    urlLabel: vt('sourceEditor.exploreUrlLabel'),
    urlJsonKey: keyMap.exploreUrlKey,
    onUpdateUrl: (val) => {
      form.exploreUrl = val
    },
    onUpdateRules: (val) => {
      form.ruleExplore = val
    },
  },
  {
    id: 'detail',
    title: vt('sourceEditor.stageDetail'),
    jsonKey: 'ruleDetail',
    fields: localizedRules.value.detail,
    modelValue: form.ruleDetail,
    onUpdateRules: (val) => {
      form.ruleDetail = val
    },
  },
  {
    id: 'play',
    title: vt('sourceEditor.stagePlay'),
    jsonKey: 'rulePlay',
    fields: localizedRules.value.play,
    modelValue: form.rulePlay,
    onUpdateRules: (val) => {
      form.rulePlay = val
    },
  },
])

const hasRulesConfigured = computed(() =>
  Boolean(
    form.categoriesUrl.trim() ||
      form.searchUrl.trim() ||
      form.exploreUrl.trim() ||
      hasAnyRule(form.ruleCategories) ||
      hasAnyRule(form.ruleSearch) ||
      hasAnyRule(form.ruleExplore) ||
      hasAnyRule(form.ruleDetail) ||
      hasAnyRule(form.rulePlay),
  ),
)

const hasAdvancedConfigured = computed(() =>
  Boolean(
    form.concurrentRate.trim() ||
      form.header.trim() ||
      form.mediaHeader.trim() ||
      form.loginUrl.trim() ||
      form.loginUi.trim() ||
      form.loginCheckJs.trim() ||
      form.jsLib.trim() ||
      variableText.value.trim(),
  ),
)

// Hide custom rule tab when source format is AppleCMS v10 (V-19)
const tabItems = computed(() =>
  buildSourceEditTabs({
    basicLabel: t('onlineSources.edit.tabBasic'),
    rulesLabel: vt('sourceEditor.tabRules'),
    advancedLabel: vt('sourceEditor.tabAdvanced'),
    jsonLabel: t('onlineSources.edit.tabJson'),
    showRules: !isApplecms.value,
    hasRulesConfigured: hasRulesConfigured.value,
    hasAdvancedConfigured: hasAdvancedConfigured.value,
  }),
)

watch(isApplecms, (applecms) => {
  if (applecms && activeTab.value === 'rules') {
    switchTab('basic')
  }
})
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
    :js-module="jsModule"
    @close="emit('close')"
    @switch-tab="switchTab"
    @update:json-text="jsonText = $event"
    @mark-json-dirty="jsonDirty = true"
    @format-json="formatJson"
    @copy-json="copyJson"
    @save="saveSource"
  >
    <template v-if="activeTab === 'basic'">
      <SourceBasicSection
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
            <span class="text-label-medium block text-on-surface-variant" :title="keyMap.formatKey">
              {{ t('onlineSources.edit.sourceFormatLabel') }}
            </span>
            <MdSelect v-model="form.sourceFormat" :options="formatOptions" size="sm" />
            <p class="text-body-small px-1 text-on-surface-variant">{{ keyMap.formatKey }}</p>
          </div>

          <MdTextField
            v-model="form.checkKeyword"
            size="sm"
            :label="t('onlineSources.edit.checkKeywordLabel')"
            :placeholder="t('onlineSources.edit.checkKeywordPlaceholder')"
            :supporting-text="keyMap.checkKeywordKey"
            :title="keyMap.checkKeywordKey"
          />
        </template>

        <template #switches>
          <MdSwitch v-model="form.enabled" :label="t('onlineSources.edit.enabledSwitch')" />
          <MdSwitch
            v-model="form.enabledExplore"
            :label="t('onlineSources.edit.enabledExploreSwitch')"
          />
          <MdSwitch
            v-model="form.searchable"
            :label="t('onlineSources.edit.searchableSwitch')"
          />
          <MdSwitch
            v-model="form.enabledCookieJar"
            :label="t('onlineSources.edit.enabledCookieJarSwitch')"
          />
        </template>
      </SourceBasicSection>

      <section
        v-if="isApplecms"
        class="flex flex-col gap-3.5 rounded-corner-lg bg-surface-container p-4 sm:p-5"
      >
        <h4 class="text-title-small text-on-surface">
          {{ t('onlineSources.edit.applecmsSectionTitle') }}
        </h4>
        <div class="grid grid-cols-1 gap-3.5">
          <MdTextField
            v-model="form.apiUrl"
            size="sm"
            :label="t('onlineSources.edit.apiUrlLabel')"
            :placeholder="t('onlineSources.edit.apiUrlPlaceholder')"
            :supporting-text="keyMap.apiUrlKey"
            :title="keyMap.apiUrlKey"
            class="font-mono"
          />

          <MdTextField
            v-model="form.preferredPlayFrom"
            size="sm"
            :label="t('onlineSources.edit.preferredPlayFromLabel')"
            :placeholder="t('onlineSources.edit.preferredPlayFromPlaceholder')"
            supporting-text="preferredPlayFrom"
            title="preferredPlayFrom"
            class="font-mono"
          />

          <MdTextField
            v-model="form.playUrlFilterRegex"
            size="sm"
            :label="t('onlineSources.edit.playUrlFilterRegexLabel')"
            supporting-text="playUrlFilterRegex"
            title="playUrlFilterRegex"
            class="font-mono"
          />
        </div>
      </section>
    </template>

    <SourceRuleStagesAccordion v-else-if="activeTab === 'rules'" :stages="ruleStages" />

    <SourceAdvancedSection
      v-else-if="activeTab === 'advanced'"
      v-model:concurrent-rate="form.concurrentRate"
      v-model:header="form.header"
      v-model:secondary-header="form.mediaHeader"
      v-model:login-url="form.loginUrl"
      v-model:login-ui="form.loginUi"
      v-model:login-check-js="form.loginCheckJs"
      v-model:js-lib="form.jsLib"
      v-model:variable-text="variableText"
      has-secondary-header
      :secondary-header-label="t('onlineSources.edit.mediaHeaderLabel')"
      :secondary-header-json-key="keyMap.mediaHeaderKey"
    />
  </OnlineSourceEditShell>
</template>
