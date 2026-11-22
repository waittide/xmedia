<script setup lang="ts">
import { computed } from 'vue'
import { LuCheck, LuCopy, LuRadio } from 'vue-icons-plus/lu'
import { MdButton, MdCard, MdEmptyState } from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'
import { useCopyFeedback } from './clipboard'
import { useSettingsI18n } from './i18nHelper'
import { DOMAIN_ICONS } from './sections'

interface ProtocolEndpointRow {
  key: string
  domain: MediaKind
  label: string
  url: string
}

const { t } = useSettingsI18n()
const authStore = useAuthStore()
const { copiedKey, copyText } = useCopyFeedback()

const originUrl = computed(() => window.location.origin)

const protocolEndpoints = computed<ProtocolEndpointRow[]>(() => {
  const rows: ProtocolEndpointRow[] = []
  if (authStore.hasModule('book')) {
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
  if (authStore.hasModule('comic')) {
    rows.push({
      key: 'comicPse',
      domain: 'comic',
      label: t('settings.comicOpdsPseUrl'),
      url: `${originUrl.value}/opds/comic/v1.2`,
    })
  }
  if (authStore.hasModule('music')) {
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
  return rows
})
</script>

<template>
  <MdCard
    v-if="protocolEndpoints.length > 0"
    id="protocols"
    as="section"
    variant="outlined"
    padding="none"
    class="scroll-mt-20 overflow-hidden"
  >
    <div class="border-b border-outline-variant/60 px-6 py-4">
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

    <ul class="divide-y divide-outline-variant/50">
      <li
        v-for="ep in protocolEndpoints"
        :key="ep.key"
        class="flex flex-col justify-between gap-2.5 px-6 py-3.5 sm:flex-row sm:items-center"
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
          size="sm"
          :icon="copiedKey === ep.key ? LuCheck : LuCopy"
          class="shrink-0 self-start sm:self-center"
          @click="copyText(ep.key, ep.url)"
        >
          {{ copiedKey === ep.key ? t('common.copied') : t('common.copy') }}
        </MdButton>
      </li>
    </ul>

    <div
      v-if="authStore.hasModule('music')"
      class="text-body-small flex flex-wrap items-center gap-x-2 gap-y-1 border-t border-outline-variant/50 bg-surface-container-low px-6 py-3"
    >
      <span class="text-label-medium text-on-surface">
        {{ t('settings.subsonicAuthModes') }}:
      </span>
      <span class="text-on-surface-variant">
        {{ t('settings.subsonicAuthModesVal') }}
      </span>
    </div>
  </MdCard>

  <MdCard v-else id="protocols" as="section" variant="outlined" padding="none" class="scroll-mt-20">
    <MdEmptyState
      compact
      :icon="LuRadio"
      :title="t('settings.domainProtocolEmptyTitle')"
      :description="t('settings.domainProtocolEmptyHint')"
    />
  </MdCard>
</template>
