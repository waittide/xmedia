<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuCopy, LuRadio } from 'vue-icons-plus/lu'
import { MdButton, MdCard, MdEmptyState } from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'
import { useCopyFeedback } from './clipboard'
import { DOMAIN_ICONS } from './sections'

interface ProtocolEndpointRow {
  key: string
  domain: MediaKind
  label: string
  url: string
}

const props = withDefaults(
  defineProps<{
    domain?: MediaKind
  }>(),
  {
    domain: undefined,
  },
)

const { t } = useI18n()
const authStore = useAuthStore()
const { copiedKey, copyText } = useCopyFeedback()

const originUrl = computed(() => window.location.origin)

function includeDomain(kind: MediaKind): boolean {
  if (props.domain) return props.domain === kind && authStore.hasModule(kind)
  return authStore.hasModule(kind)
}

const protocolEndpoints = computed<ProtocolEndpointRow[]>(() => {
  const rows: ProtocolEndpointRow[] = []
  if (includeDomain('book')) {
    rows.push(
      {
        key: 'book12',
        domain: 'book',
        label: t('settings.bookOpds12Url'),
        url: `${originUrl.value}/opds/book/v1.2`,
      },
      {
        key: 'book20',
        domain: 'book',
        label: t('settings.bookOpds20Url'),
        url: `${originUrl.value}/opds/book/v2`,
      },
    )
  }
  if (includeDomain('comic')) {
    rows.push({
      key: 'comicPse',
      domain: 'comic',
      label: t('settings.comicOpdsPseUrl'),
      url: `${originUrl.value}/opds/comic/v1.2`,
    })
  }
  if (includeDomain('music')) {
    rows.push(
      {
        key: 'subServer',
        domain: 'music',
        label: t('settings.subsonicServerUrl'),
        url: originUrl.value,
      },
      {
        key: 'subRest',
        domain: 'music',
        label: t('settings.subsonicEndpointUrl'),
        url: `${originUrl.value}/rest`,
      },
    )
  }
  if (includeDomain('video')) {
    rows.push({
      key: 'videoJellyfin',
      domain: 'video',
      label: t('settings.videoJellyfinUrl'),
      url: `${originUrl.value}/jellyfin`,
    })
  }
  return rows
})
</script>

<template>
  <MdCard
    v-if="protocolEndpoints.length > 0"
    id="protocols"
    as="section"
    variant="tonal"
    padding="none"
    class="scroll-mt-20 overflow-hidden"
  >
    <div class="px-4 pt-4 pb-2 sm:px-6 sm:pt-5">
      <div class="flex items-center gap-2">
        <LuRadio class="h-4 w-4 text-primary" />
        <h2 class="text-title-medium text-on-surface">
          {{ t('settings.protocolsGuideTitle') }}
        </h2>
      </div>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{ t('settings.protocolsGuideDesc') }}
      </p>
    </div>

    <ul class="divide-y divide-on-surface/6">
      <li
        v-for="ep in protocolEndpoints"
        :key="ep.key"
        class="flex flex-col justify-between gap-2.5 px-4 py-3.5 sm:flex-row sm:items-center sm:px-6"
      >
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <component
              :is="DOMAIN_ICONS[ep.domain]"
              class="h-3.5 w-3.5 shrink-0 text-on-surface-variant"
            />
            <span class="text-label-medium text-on-surface-variant">{{ ep.label }}</span>
          </div>
          <p class="text-body-small mt-0.5 truncate font-mono text-on-surface">
            {{ ep.url }}
          </p>
        </div>

        <MdButton
          variant="tonal"
          :icon="copiedKey === ep.key ? LuCheck : LuCopy"
          class="shrink-0 self-start sm:self-center"
          @click="copyText(ep.key, ep.url)"
        >
          {{ copiedKey === ep.key ? t('common.copied') : t('common.copy') }}
        </MdButton>
      </li>
    </ul>

    <div
      v-if="includeDomain('book') || includeDomain('comic')"
      class="text-body-small flex flex-wrap items-center gap-x-2 gap-y-1 bg-surface-container px-4 py-3 sm:px-6"
    >
      <span class="text-label-medium text-on-surface">
        {{ t('settings.opdsAuthModes') }}:
      </span>
      <span class="text-on-surface-variant">
        {{ t('settings.opdsAuthModesVal') }}
      </span>
    </div>

    <div
      v-if="includeDomain('music')"
      class="text-body-small flex flex-wrap items-center gap-x-2 gap-y-1 bg-surface-container px-4 py-3 sm:px-6"
    >
      <span class="text-label-medium text-on-surface">
        {{ t('settings.subsonicAuthModes') }}:
      </span>
      <span class="text-on-surface-variant">
        {{ t('settings.subsonicAuthModesVal') }}
      </span>
    </div>

    <div
      v-if="includeDomain('video')"
      class="text-body-small flex flex-wrap items-center gap-x-2 gap-y-1 bg-surface-container px-4 py-3 sm:px-6"
    >
      <span class="text-label-medium text-on-surface">
        {{ t('settings.jellyfinAuthModes') }}:
      </span>
      <span class="text-on-surface-variant">
        {{ t('settings.jellyfinAuthModesVal') }}
      </span>
    </div>
  </MdCard>

  <MdCard v-else id="protocols" as="section" variant="tonal" padding="none" class="scroll-mt-20">
    <MdEmptyState
      compact
      :icon="LuRadio"
      :title="t('settings.domainProtocolEmptyTitle')"
      :description="t('settings.domainProtocolEmptyHint')"
    />
  </MdCard>
</template>
