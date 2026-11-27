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
import { MdSwitch, MdTextField } from '@/components/ui'
import SourceAdvancedSection from '@/domains/shared/source-editor/SourceAdvancedSection.vue'
import SourceBasicSection from '@/domains/shared/source-editor/SourceBasicSection.vue'
import SourceRuleStagesAccordion from '@/domains/shared/source-editor/SourceRuleStagesAccordion.vue'
import { buildSourceEditTabs, type RuleStageItem } from '@/domains/shared/source-editor/types'
import { ensureMusicLocale, useMusicI18n } from '../i18n'
import {
  buildMergedMusicSourceObject,
  buildMusicRuleFields,
  createDefaultMusicFormState,
  createDefaultMusicKeyMap,
  hydrateMusicSourceForm,
  type MusicSourceEditTab,
  type MusicSourceSnapshot,
} from '../musicSourceSchema'
import { musicSourceAdapter, type MusicSourceSummaryDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: MusicSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: MusicSourceSummaryDto]
  savedAndDebug: [source: MusicSourceSummaryDto]
}>()

const { t } = useI18n()
const { t: tm } = useMusicI18n()

void ensureMusicLocale()
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void ensureMusicLocale()
  },
)

const localizedRules = computed(() => buildMusicRuleFields(tm))
const keyMap = reactive(createDefaultMusicKeyMap())
const form = reactive(createDefaultMusicFormState())
let initialSnapshot: MusicSourceSnapshot | null = null

function clearFormState(): void {
  initialSnapshot = null
  Object.assign(keyMap, createDefaultMusicKeyMap())
  Object.assign(form, createDefaultMusicFormState())
}

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: MusicSourceSummaryDto,
): void {
  initialSnapshot = hydrateMusicSourceForm(form, keyMap, obj, srcSummary)
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
} = useOnlineSourceEdit<MusicSourceSummaryDto, MusicSourceEditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: musicSourceAdapter,
  initialTab: 'basic',
  jsonTabId: 'json',
  clearFormState,
  hydrateFormFromObject,
  buildMergedObject: (): Record<string, unknown> =>
    buildMergedMusicSourceObject(rawBaseObj.value, form, keyMap, initialSnapshot),
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

const domainLabel = computed(() => t('onlineSources.domainLabel.music'))

const ruleStages = computed<RuleStageItem[]>(() => [
  {
    id: 'search',
    title: tm('sourceEditor.sectionSearch'),
    jsonKey: 'ruleSearch',
    fields: localizedRules.value.search,
    modelValue: form.ruleSearch,
    urlValue: form.searchUrl,
    urlLabel: tm('sourceEditor.searchUrlLabel'),
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
    title: tm('sourceEditor.sectionExplore'),
    jsonKey: 'ruleExplore',
    fields: localizedRules.value.explore,
    modelValue: form.ruleExplore,
    urlValue: form.exploreUrl,
    urlLabel: tm('sourceEditor.exploreUrlLabel'),
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
    id: 'track_detail',
    title: tm('sourceEditor.sectionTrackDetail'),
    jsonKey: 'ruleTrackDetail',
    fields: localizedRules.value.trackDetail,
    modelValue: form.ruleTrackDetail,
    onUpdateRules: (val) => {
      form.ruleTrackDetail = val
    },
  },
  {
    id: 'stream',
    title: tm('sourceEditor.sectionStream'),
    jsonKey: 'ruleStream',
    fields: localizedRules.value.stream,
    modelValue: form.ruleStream,
    onUpdateRules: (val) => {
      form.ruleStream = val
    },
  },
  {
    id: 'cover',
    title: tm('sourceEditor.sectionCover'),
    jsonKey: 'ruleCover',
    fields: localizedRules.value.cover,
    modelValue: form.ruleCover,
    onUpdateRules: (val) => {
      form.ruleCover = val
    },
  },
  {
    id: 'lyric',
    title: tm('sourceEditor.sectionLyric'),
    jsonKey: 'ruleLyric',
    fields: localizedRules.value.lyric,
    modelValue: form.ruleLyric,
    onUpdateRules: (val) => {
      form.ruleLyric = val
    },
  },
])

const hasRulesConfigured = computed(() =>
  Boolean(
    form.searchUrl.trim() ||
      form.exploreUrl.trim() ||
      hasAnyRule(form.ruleSearch) ||
      hasAnyRule(form.ruleExplore) ||
      hasAnyRule(form.ruleTrackDetail) ||
      hasAnyRule(form.ruleStream) ||
      hasAnyRule(form.ruleCover) ||
      hasAnyRule(form.ruleLyric),
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

const tabItems = computed(() =>
  buildSourceEditTabs({
    basicLabel: t('onlineSources.edit.tabBasic'),
    rulesLabel: tm('sourceEditor.tabRules'),
    advancedLabel: tm('sourceEditor.tabAdvanced'),
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
      v-model:source-name="form.sourceName"
      v-model:source-group="form.sourceGroup"
      v-model:source-url="form.sourceUrl"
      v-model:weight="form.weight"
      v-model:source-comment="form.sourceComment"
      :name-json-key="keyMap.nameKey"
      :url-json-key="keyMap.urlKey"
    >
      <template #extra-fields>
        <MdTextField
          v-model="form.supportedQualities"
          size="sm"
          :label="t('onlineSources.edit.supportedQualitiesLabel')"
          :placeholder="t('onlineSources.edit.supportedQualitiesPlaceholder')"
          supporting-text="supportedQualities"
          title="supportedQualities"
          class="font-mono"
        />
      </template>

      <template #switches>
        <MdSwitch v-model="form.enabled" :label="t('onlineSources.edit.enabledSwitch')" />
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
      v-model:secondary-header="form.mediaHeader"
      v-model:login-url="form.loginUrl"
      v-model:login-ui="form.loginUi"
      v-model:login-check-js="form.loginCheckJs"
      v-model:js-lib="form.jsLib"
      v-model:variable-text="variableText"
      :header-json-key="keyMap.headerKey"
      has-secondary-header
      :secondary-header-label="t('onlineSources.edit.mediaHeaderLabel')"
      :secondary-header-json-key="keyMap.mediaHeaderKey"
    />
  </OnlineSourceEditShell>
</template>
