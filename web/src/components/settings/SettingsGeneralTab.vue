<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuAlertCircle, LuCheckCircle2, LuHardDrive, LuLayers } from 'vue-icons-plus/lu'
import { MdCard } from '@/components/ui'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { DOMAIN_ICONS } from './sections'

const { t } = useI18n()
const authStore = useAuthStore()
const appClientStore = useAppClientStore()

const displayVersion = computed(
  () => authStore.capabilities?.version || appClientStore.appVersion || '0.1.0',
)
</script>

<template>
  <div class="flex flex-col gap-5">
    <!-- Application Version & Runtime Mode -->
    <MdCard as="section" variant="outlined" padding="lg" class="space-y-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="flex items-center gap-3.5">
          <div
            class="flex h-11 w-11 shrink-0 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
          >
            <LuHardDrive v-if="appClientStore.isStandaloneApp" class="h-5 w-5" />
            <LuLayers v-else class="h-5 w-5" />
          </div>
          <div>
            <h2 class="text-title-medium text-on-surface">
              {{
                appClientStore.isStandaloneApp
                  ? t('settings.standaloneModeTitle')
                  : t('app.name')
              }}
            </h2>
            <p class="text-body-small mt-0.5 font-mono text-on-surface-variant">
              XMedia v{{ displayVersion }}
            </p>
          </div>
        </div>

        <span
          v-if="appClientStore.isStandaloneApp"
          class="text-label-medium rounded-corner-full bg-primary-container px-3 py-1 text-on-primary-container"
        >
          {{ t('settings.standaloneModeBadge') }}
        </span>
        <span
          v-else
          class="text-label-medium rounded-corner-full bg-secondary-container px-3 py-1 text-on-secondary-container"
        >
          {{ t('settings.serverModeBadge') }}
        </span>
      </div>

      <div class="flex flex-wrap items-center gap-2 border-t border-outline-variant/50 pt-3.5">
        <span class="text-label-medium mr-1 text-on-surface-variant">
          {{ t('settings.enabledDomainsLabel') }}:
        </span>
        <span
          v-for="kind in authStore.enabledModules"
          :key="kind"
          class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-surface-container-high px-2.5 py-0.5 text-on-surface-variant"
        >
          <component :is="DOMAIN_ICONS[kind]" class="h-3 w-3" />
          <span>{{ t(`app.domains.${kind}`) }}</span>
        </span>
      </div>
    </MdCard>

    <!-- FFmpeg Engine Status -->
    <MdCard as="section" variant="outlined" padding="lg">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="min-w-0">
          <h2 class="text-title-medium text-on-surface">
            {{ t('settings.ffmpegStatusTitle') }}
          </h2>
          <p
            v-if="authStore.capabilities?.ffmpeg.ffmpeg_version"
            class="text-body-small mt-1 truncate font-mono text-on-surface-variant"
          >
            {{ authStore.capabilities.ffmpeg.ffmpeg_version }}
          </p>
        </div>

        <span
          v-if="authStore.capabilities?.ffmpeg.ffmpeg_available"
          class="text-label-medium inline-flex shrink-0 items-center gap-1.5 rounded-corner-full bg-success-container px-3 py-1 text-on-success-container"
        >
          <LuCheckCircle2 class="h-4 w-4" />
          <span>{{ t('settings.ffmpegAvailable') }}</span>
        </span>
        <span
          v-else
          class="text-label-medium inline-flex shrink-0 items-center gap-1.5 rounded-corner-full bg-warning-container px-3 py-1 text-on-warning-container"
        >
          <LuAlertCircle class="h-4 w-4" />
          <span>{{ t('settings.ffmpegMissing') }}</span>
        </span>
      </div>
    </MdCard>
  </div>
</template>
