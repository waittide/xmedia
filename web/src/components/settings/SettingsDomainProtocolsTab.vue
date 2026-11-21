<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuCopy, LuRadio } from 'vue-icons-plus/lu'
import type { RemoteProtocol } from '@/api/appHost'
import { useAppClientStore } from '@/stores/appServers'
import type { MediaKind } from '@/types/api'
import { useCopyFeedback } from './clipboard'

const RemoteSourcesView = defineAsyncComponent(() => import('@/views/RemoteSourcesView.vue'))

const props = defineProps<{
  domain: MediaKind
}>()

const { t } = useI18n()
const appClientStore = useAppClientStore()
const { copiedKey, copyText } = useCopyFeedback()

const originUrl = computed(() => window.location.origin)

const protocolEndpoints = computed(() => {
  switch (props.domain) {
    case 'book':
      return [
        {
          key: 'book12',
          label: t('settings.bookOpds12Url'),
          url: `${originUrl.value}/opds/book/v1.2`,
        },
        {
          key: 'book20',
          label: t('settings.bookOpds20Url'),
          url: `${originUrl.value}/opds/book/v2`,
        },
      ]
    case 'comic':
      return [
        {
          key: 'comicPse',
          label: t('settings.comicOpdsPseUrl'),
          url: `${originUrl.value}/opds/comic/v1.2`,
        },
      ]
    case 'music':
      return [
        { key: 'subServer', label: t('settings.subsonicServerUrl'), url: originUrl.value },
        {
          key: 'subRest',
          label: t('settings.subsonicEndpointUrl'),
          url: `${originUrl.value}/rest`,
        },
      ]
    case 'video':
      return []
  }
})

const remoteProtocol = computed<RemoteProtocol | null>(() => {
  if (props.domain === 'book' || props.domain === 'comic') return 'opds'
  if (props.domain === 'music') return 'subsonic'
  return null
})
</script>

<template>
  <!-- Standalone App: Remote Protocol Client (Manage & Browse) -->
  <section
    v-if="appClientStore.isStandaloneApp && remoteProtocol"
    class="md-card-outlined p-6"
  >
    <RemoteSourcesView :embedded-protocol="remoteProtocol" />
  </section>

  <!-- Exposed Open Protocol Endpoints -->
  <section v-if="protocolEndpoints.length > 0" class="md-card-outlined space-y-4 p-6">
    <div>
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.protocolsGuideTitle') }}
      </h2>
      <p class="md-body-small mt-1 leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('settings.protocolsGuideDesc') }}
      </p>
    </div>

    <div class="space-y-3">
      <div v-for="ep in protocolEndpoints" :key="ep.key">
        <label class="md-label-medium mb-1 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ ep.label }}
        </label>
        <div class="flex items-center gap-2">
          <input
            readonly
            :value="ep.url"
            class="md-input h-9 min-w-0 flex-1 font-mono text-xs"
          />
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-filled shrink-0"
            @click="copyText(ep.key, ep.url)"
          >
            <LuCheck v-if="copiedKey === ep.key" class="h-3.5 w-3.5" />
            <LuCopy v-else class="h-3.5 w-3.5" />
            <span>{{ copiedKey === ep.key ? t('common.copied') : t('common.copy') }}</span>
          </button>
        </div>
      </div>
    </div>

    <div
      v-if="domain === 'music'"
      class="md-card-filled flex flex-wrap items-center gap-x-2 gap-y-1 px-3.5 py-2.5 text-xs"
    >
      <span class="font-semibold text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.subsonicAuthModes') }}
      </span>
      <span class="text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('settings.subsonicAuthModesVal') }}
      </span>
    </div>
  </section>

  <!-- Empty state for domains with no protocols yet (e.g. Video) -->
  <section
    v-if="protocolEndpoints.length === 0 && !(appClientStore.isStandaloneApp && remoteProtocol)"
    class="md-card-outlined px-6 py-14 text-center"
  >
    <div
      class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuRadio class="h-5 w-5" />
    </div>
    <p class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('settings.domainProtocolEmptyTitle') }}
    </p>
    <p class="md-body-small mx-auto mt-1 max-w-sm text-[var(--md-sys-color-on-surface-variant)]">
      {{ t('settings.domainProtocolEmptyHint') }}
    </p>
  </section>
</template>
