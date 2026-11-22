<script setup lang="ts">
import { computed, reactive, ref, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuChevronDown,
  LuChevronUp,
  LuCode2,
  LuCompass,
  LuFilm,
  LuFolderTree,
  LuInfo,
  LuPlay,
  LuSearch,
} from 'vue-icons-plus/lu'
import OnlineSourceEditShell from '@/components/online-sources/OnlineSourceEditShell.vue'
import RuleFieldsSection from '@/components/online-sources/RuleFieldsSection.vue'
import {
  hasAnyRule,
  numVal,
  pickFirstString,
  useOnlineSourceEdit,
  type EditTabItem,
} from '@/components/online-sources/useOnlineSourceEdit'
import { MdSelect, MdSwitch, type SelectOption } from '@/components/ui'
import { videoSourceAdapter, type VideoSourceSummaryDto } from '../onlineApi'
import {
  buildMergedVideoSourceObject,
  CATEGORIES_RULE_FIELDS,
  createDefaultFormState,
  createDefaultKeyMap,
  DETAIL_RULE_FIELDS,
  hydrateVideoSourceForm,
  PLAY_RULE_FIELDS,
  SEARCH_RULE_FIELDS,
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

const formatOptions = computed<SelectOption[]>(() => [
  { value: 'applecms_v10', label: t('onlineSources.edit.formatApplecms') },
  { value: 'rule', label: t('onlineSources.edit.formatRule') },
])

const keyMap = reactive(createDefaultKeyMap())
const form = reactive(createDefaultFormState())
const advancedNetworkOpen = ref(false)

let initialSnapshot: VideoSourceSnapshot | null = null

function clearFormState(): void {
  initialSnapshot = null
  advancedNetworkOpen.value = false
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
    buildMergedVideoSourceObject(
      rawBaseObj.value as Record<string, unknown>,
      form,
      keyMap,
      initialSnapshot,
    ),
  validateForm: () => {
    if (!form.sourceName.trim() && !form.sourceUrl.trim()) {
      return t('onlineSources.edit.nameAndUrlRequired')
    }
    return null
  },
  validateJsonObject: (obj) => {
    const name = pickFirstString(obj, ['sourceName', 'name', 'bookSourceName']).trim()
    const url = pickFirstString(obj, ['sourceUrl', 'url', 'bookSourceUrl']).trim()
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

const hasAdvancedNetworkValues = computed(() =>
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

// Hide inapplicable custom rule tabs when source format is AppleCMS v10 (V-19)
const tabItems = computed<EditTabItem<VideoSourceEditTab>[]>(() => {
  const basicTab: EditTabItem<VideoSourceEditTab> = {
    id: 'basic',
    label: t('onlineSources.edit.tabBasic'),
    icon: LuInfo,
    activeDot: false,
  }
  const jsonTab: EditTabItem<VideoSourceEditTab> = {
    id: 'json',
    label: t('onlineSources.edit.tabJson'),
    icon: LuCode2,
    activeDot: false,
  }

  if (isApplecms.value) {
    return [basicTab, jsonTab]
  }

  return [
    basicTab,
    {
      id: 'categories',
      label: t('onlineSources.edit.tabCategories'),
      icon: LuFolderTree,
      activeDot: Boolean(form.categoriesUrl.trim() || hasAnyRule(form.ruleCategories)),
    },
    {
      id: 'search',
      label: t('onlineSources.edit.tabSearch'),
      icon: LuSearch,
      activeDot: Boolean(form.searchUrl.trim() || hasAnyRule(form.ruleSearch)),
    },
    {
      id: 'explore',
      label: t('onlineSources.edit.tabExplore'),
      icon: LuCompass,
      activeDot: Boolean(form.exploreUrl.trim() || hasAnyRule(form.ruleExplore)),
    },
    {
      id: 'detail',
      label: t('onlineSources.edit.tabDetail'),
      icon: LuFilm,
      activeDot: hasAnyRule(form.ruleDetail),
    },
    {
      id: 'play',
      label: t('onlineSources.edit.tabPlay'),
      icon: LuPlay,
      activeDot: hasAnyRule(form.rulePlay),
    },
    jsonTab,
  ]
})

watch(isApplecms, (applecms) => {
  if (applecms && activeTab.value !== 'basic' && activeTab.value !== 'json') {
    switchTab('basic')
  }
})
</script>

<template>
  <OnlineSourceEditShell
    :open="open"
    :title="t('onlineSources.edit.title', { domain: domainLabel, name: form.sourceName || source?.name || '' })"
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
    <template v-if="activeTab === 'basic'">
      <!-- Core Basic Fields (concise, no raw JSON key clutter) -->
      <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4">
        <h4 class="text-title-small text-on-surface">
          {{ t('onlineSources.edit.basicSectionTitle') }}
        </h4>

        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.nameLabel') }}
            </span>
            <input
              v-model="form.sourceName"
              type="text"
              class="text-body-medium h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.groupLabel') }}
            </span>
            <input
              v-model="form.sourceGroup"
              name="group_name"
              type="text"
              :aria-label="t('onlineSources.edit.groupLabel')"
              :placeholder="t('onlineSources.edit.groupPlaceholder')"
              class="text-body-medium h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1 sm:col-span-2">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.urlLabel') }}
            </span>
            <input
              v-model="form.sourceUrl"
              type="text"
              :placeholder="t('onlineSources.edit.urlPlaceholder')"
              class="text-body-small h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <div class="space-y-1">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('onlineSources.edit.sourceFormatLabel') }}
            </span>
            <MdSelect v-model="form.sourceFormat" :options="formatOptions" size="sm" />
          </div>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.checkKeywordLabel') }}
            </span>
            <input
              v-model="form.checkKeyword"
              type="text"
              :placeholder="t('onlineSources.edit.checkKeywordPlaceholder')"
              class="text-body-medium h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.weightLabel') }}
            </span>
            <input
              v-model.number="form.weight"
              type="number"
              class="text-body-small h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1 sm:col-span-2">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.commentLabel') }}
            </span>
            <textarea
              v-model="form.sourceComment"
              rows="2"
              :placeholder="t('onlineSources.edit.commentPlaceholder')"
              class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>
        </div>

        <div class="flex flex-wrap items-center gap-x-6 gap-y-2.5 border-t border-outline-variant/50 pt-3">
          <label class="text-body-small inline-flex items-center gap-2 text-on-surface">
            <MdSwitch v-model="form.enabled" />
            <span>{{ t('onlineSources.edit.enabledSwitch') }}</span>
          </label>
          <label class="text-body-small inline-flex items-center gap-2 text-on-surface">
            <MdSwitch v-model="form.enabledExplore" />
            <span>{{ t('onlineSources.edit.enabledExploreSwitch') }}</span>
          </label>
          <label class="text-body-small inline-flex items-center gap-2 text-on-surface">
            <MdSwitch v-model="form.searchable" />
            <span>{{ t('onlineSources.edit.searchableSwitch') }}</span>
          </label>
          <label class="text-body-small inline-flex items-center gap-2 text-on-surface">
            <MdSwitch v-model="form.enabledCookieJar" />
            <span>{{ t('onlineSources.edit.enabledCookieJarSwitch') }}</span>
          </label>
        </div>
      </section>

      <!-- AppleCMS v10 Config -->
      <section
        v-if="isApplecms"
        class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4"
      >
        <h4 class="text-title-small text-on-surface">
          {{ t('onlineSources.edit.applecmsSectionTitle') }}
        </h4>
        <div class="grid grid-cols-1 gap-3">
          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.apiUrlLabel') }}
            </span>
            <input
              v-model="form.apiUrl"
              type="text"
              :placeholder="t('onlineSources.edit.apiUrlPlaceholder')"
              class="text-body-small h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.preferredPlayFromLabel') }}
            </span>
            <input
              v-model="form.preferredPlayFrom"
              type="text"
              :placeholder="t('onlineSources.edit.preferredPlayFromPlaceholder')"
              class="text-body-small h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.playUrlFilterRegexLabel') }}
            </span>
            <input
              v-model="form.playUrlFilterRegex"
              type="text"
              spellcheck="false"
              class="text-body-small h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>
        </div>
      </section>

      <!-- Collapsible Advanced Network, Auth & Scripts Section (V-19) -->
      <section class="rounded-corner-lg bg-surface-container p-4">
        <button
          type="button"
          :aria-expanded="advancedNetworkOpen"
          class="md-state flex w-full cursor-pointer items-center justify-between gap-2 rounded-corner-sm py-1 text-left"
          @click="advancedNetworkOpen = !advancedNetworkOpen"
        >
          <span class="flex items-center gap-2">
            <span class="text-title-small text-on-surface">
              {{ t('onlineSources.edit.networkSectionTitle') }}
            </span>
            <span
              v-if="hasAdvancedNetworkValues"
              class="h-1.5 w-1.5 rounded-corner-full bg-primary"
              aria-hidden="true"
            />
          </span>
          <LuChevronUp
            v-if="advancedNetworkOpen"
            class="h-4 w-4 text-on-surface-variant"
            aria-hidden="true"
          />
          <LuChevronDown
            v-else
            class="h-4 w-4 text-on-surface-variant"
            aria-hidden="true"
          />
        </button>

        <div v-if="advancedNetworkOpen" class="mt-3 grid grid-cols-1 gap-3 border-t border-outline-variant/50 pt-3">
          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.concurrentRateLabel') }}
            </span>
            <input
              v-model="form.concurrentRate"
              type="text"
              :placeholder="t('onlineSources.edit.concurrentRatePlaceholder')"
              class="text-body-small h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.headerLabel') }}
            </span>
            <textarea
              v-model="form.header"
              rows="2"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.headerPlaceholder')"
              class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.mediaHeaderLabel') }}
            </span>
            <textarea
              v-model="form.mediaHeader"
              rows="2"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.headerPlaceholder')"
              class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.loginUrlLabel') }}
            </span>
            <textarea
              v-model="form.loginUrl"
              rows="2"
              spellcheck="false"
              class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.loginUiLabel') }}
            </span>
            <textarea
              v-model="form.loginUi"
              rows="3"
              spellcheck="false"
              class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.loginCheckJsLabel') }}
            </span>
            <textarea
              v-model="form.loginCheckJs"
              rows="2"
              spellcheck="false"
              class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.jsLibLabel') }}
            </span>
            <textarea
              v-model="form.jsLib"
              rows="3"
              spellcheck="false"
              class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('onlineSources.edit.variableLabel') }}
            </span>
            <input
              v-model="variableText"
              type="text"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.variablePlaceholder')"
              class="text-body-small h-9 w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            />
          </label>
        </div>
      </section>
    </template>

    <template v-else-if="activeTab === 'categories'">
      <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4">
        <h4 class="text-title-small text-on-surface">
          {{ t('onlineSources.edit.categoriesUrlLabel') }}
        </h4>
        <textarea
          v-model="form.categoriesUrl"
          rows="3"
          spellcheck="false"
          class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
        />
      </section>

      <RuleFieldsSection
        v-model="form.ruleCategories"
        title="ruleCategories"
        :fields="CATEGORIES_RULE_FIELDS"
      />
    </template>

    <template v-else-if="activeTab === 'search'">
      <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4">
        <h4 class="text-title-small text-on-surface">
          {{ t('onlineSources.edit.searchUrlLabel') }}
        </h4>
        <textarea
          v-model="form.searchUrl"
          rows="3"
          spellcheck="false"
          class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
        />
      </section>

      <RuleFieldsSection
        v-model="form.ruleSearch"
        title="ruleSearch"
        :fields="SEARCH_RULE_FIELDS"
      />
    </template>

    <template v-else-if="activeTab === 'explore'">
      <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4">
        <h4 class="text-title-small text-on-surface">
          {{ t('onlineSources.edit.exploreUrlLabel') }}
        </h4>
        <textarea
          v-model="form.exploreUrl"
          rows="3"
          spellcheck="false"
          class="text-body-small w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono text-on-surface outline-none focus:border-primary focus:ring-1 focus:ring-primary"
        />
      </section>

      <RuleFieldsSection
        v-model="form.ruleExplore"
        title="ruleExplore"
        :fields="SEARCH_RULE_FIELDS"
      />
    </template>

    <template v-else-if="activeTab === 'detail'">
      <RuleFieldsSection
        v-model="form.ruleDetail"
        title="ruleDetail"
        :fields="DETAIL_RULE_FIELDS"
      />
    </template>

    <template v-else-if="activeTab === 'play'">
      <RuleFieldsSection
        v-model="form.rulePlay"
        title="rulePlay"
        :fields="PLAY_RULE_FIELDS"
      />
    </template>
  </OnlineSourceEditShell>
</template>
