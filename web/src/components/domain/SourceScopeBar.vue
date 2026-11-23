<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCloud, LuDatabase, LuGlobe, LuHardDrive } from 'vue-icons-plus/lu'
import { MdSegmentedButton, type SegmentedOption } from '@/components/ui'
import { useSourceFilter, type SourceScope } from '@/sources'
import type { MediaKind } from '@/types/api'

const props = withDefaults(
  defineProps<{
    /** Controlled scope value. Omit together with `domain` to let SourceScopeBar drive the URL directly. */
    modelValue?: SourceScope
    /** Media domain used for URL synchronization (`scope=online`) and remote-source detection. */
    domain?: MediaKind
    /** Synchronize clicks to URL query (`scope=online`). Defaults to `true` when `domain` is given without `modelValue`. */
    syncUrl?: boolean
    /** Show the local server pool segment (`scope=pool`). Defaults to `true`. */
    showPool?: boolean
    /** Show the remote OPDS/Subsonic segment. Defaults to `sourceFilter.hasRemoteSources` when `domain` is given. */
    showRemote?: boolean
    size?: 'sm' | 'md'
    fullWidth?: boolean
    disabled?: boolean
    localLabel?: string
    poolLabel?: string
    onlineLabel?: string
    remoteLabel?: string
    ariaLabel?: string
  }>(),
  {
    modelValue: undefined,
    domain: undefined,
    syncUrl: undefined,
    showPool: true,
    showRemote: undefined,
    size: 'sm',
    fullWidth: false,
    disabled: false,
    localLabel: '',
    poolLabel: '',
    onlineLabel: '',
    remoteLabel: '',
    ariaLabel: '',
  },
)

const emit = defineEmits<{
  'update:modelValue': [scope: SourceScope]
  change: [scope: SourceScope]
}>()

const { t } = useI18n()

const shouldSyncUrl = computed(() => {
  if (props.syncUrl !== undefined) return props.syncUrl
  return props.domain !== undefined && props.modelValue === undefined
})

const sourceFilter = props.domain
  ? useSourceFilter(() => props.domain!, {
      autoLoad: props.showRemote === undefined,
    })
  : null

const activeScope = computed<SourceScope>(() => {
  if (props.modelValue !== undefined) return props.modelValue
  if (sourceFilter) return sourceFilter.filter.value.scope
  return 'local'
})

const includeRemote = computed(() => {
  if (props.showRemote !== undefined) return props.showRemote
  if (activeScope.value === 'remote') return true
  return Boolean(sourceFilter?.hasRemoteSources.value)
})

const options = computed<readonly SegmentedOption<SourceScope>[]>(() => {
  const list: SegmentedOption<SourceScope>[] = [
    {
      value: 'local',
      label: props.localLabel || t('domainUi.scopeLocal'),
      icon: LuHardDrive,
    },
  ]
  if (props.showPool) {
    list.push({
      value: 'pool',
      label: props.poolLabel || t('domainUi.scopePool'),
      icon: LuDatabase,
    })
  }
  list.push({
    value: 'online',
    label: props.onlineLabel || t('domainUi.scopeOnline'),
    icon: LuGlobe,
  })
  if (includeRemote.value) {
    list.push({
      value: 'remote',
      label: props.remoteLabel || t('domainUi.scopeRemote'),
      icon: LuCloud,
    })
  }
  return list
})

const resolvedAriaLabel = computed(
  () => props.ariaLabel || t('domainUi.scopeAriaLabel'),
)

async function handleSelect(nextScope: SourceScope): Promise<void> {
  if (nextScope === activeScope.value && props.modelValue !== undefined) return
  emit('update:modelValue', nextScope)
  emit('change', nextScope)
  if (shouldSyncUrl.value && sourceFilter) {
    await sourceFilter.setScope(nextScope)
  }
}
</script>

<template>
  <div data-scope-switch :data-scope="activeScope" class="inline-flex max-w-full">
    <MdSegmentedButton
      :model-value="activeScope"
      :options="options"
      :size="size"
      :show-check="false"
      :full-width="fullWidth"
      :disabled="disabled"
      :aria-label="resolvedAriaLabel"
      @update:model-value="handleSelect"
    />
  </div>
</template>
