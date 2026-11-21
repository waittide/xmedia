<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuAlertCircle, LuCheckCircle2, LuHardDrive } from 'vue-icons-plus/lu'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'

const { t } = useI18n()
const authStore = useAuthStore()
const appClientStore = useAppClientStore()
</script>

<template>
  <!-- FFmpeg Engine Status -->
  <section class="md-card-outlined p-6">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
          {{ t('settings.ffmpegStatusTitle') }}
        </h2>
        <p
          v-if="authStore.capabilities?.ffmpeg.ffmpeg_version"
          class="md-body-small mt-1 font-mono text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ authStore.capabilities.ffmpeg.ffmpeg_version }}
        </p>
      </div>
      <span
        v-if="authStore.capabilities?.ffmpeg.ffmpeg_available"
        class="md-label-medium inline-flex items-center gap-1.5 rounded-full bg-[var(--md-sys-color-success-container)] px-3 py-1 text-[var(--md-sys-color-on-success-container)]"
      >
        <LuCheckCircle2 class="h-4 w-4" />
        <span>{{ t('settings.ffmpegAvailable') }}</span>
      </span>
      <span
        v-else
        class="md-label-medium inline-flex items-center gap-1.5 rounded-full bg-[var(--md-sys-color-warning-container)] px-3 py-1 text-[var(--md-sys-color-on-warning-container)]"
      >
        <LuAlertCircle class="h-4 w-4" />
        <span>{{ t('settings.ffmpegMissing') }}</span>
      </span>
    </div>
  </section>

  <!-- Standalone Client Engine Info -->
  <section v-if="appClientStore.isStandaloneApp" class="md-card-outlined p-6">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-3">
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]"
        >
          <LuHardDrive class="h-5 w-5" />
        </div>
        <div>
          <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
            {{ t('settings.standaloneModeTitle') }}
          </h2>
          <p class="md-body-small mt-0.5 font-mono text-[var(--md-sys-color-on-surface-variant)]">
            XMedia v{{ appClientStore.appVersion || '0.1.0' }}
          </p>
        </div>
      </div>
      <span
        class="md-label-medium rounded-full bg-[var(--md-sys-color-primary-container)] px-3 py-1 text-[var(--md-sys-color-on-primary-container)]"
      >
        {{ t('settings.standaloneModeBadge') }}
      </span>
    </div>
  </section>
</template>
