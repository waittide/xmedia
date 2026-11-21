<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuBookOpen,
  LuCheckCircle2,
  LuChevronRight,
  LuDisc3,
  LuLoader2,
  LuPencil,
  LuPlus,
  LuShieldAlert,
  LuTrash2,
  LuWifi,
} from 'vue-icons-plus/lu'
import type {
  RemoteProtocol,
  RemoteSourceSummaryDto,
  TestRemoteSourceResponse,
} from '@/api/appHost'

defineProps<{
  protocol: RemoteProtocol
  sources: RemoteSourceSummaryDto[]
  testingSourceId: string | null
  deletingSourceId: string | null
  cardTestResults: Record<string, TestRemoteSourceResponse>
}>()

const emit = defineEmits<{
  add: [protocol: RemoteProtocol]
  browse: [source: RemoteSourceSummaryDto]
  test: [source: RemoteSourceSummaryDto]
  edit: [source: RemoteSourceSummaryDto]
  delete: [source: RemoteSourceSummaryDto]
}>()

const { t } = useI18n()
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between gap-3">
      <div class="flex items-center gap-2">
        <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
          {{ t('remote.savedSourcesTitle') }}
        </h2>
        <span
          class="md-label-small rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ sources.length }}
        </span>
      </div>

      <button
        type="button"
        class="md-btn md-btn-filled md-btn-sm"
        @click="emit('add', protocol)"
      >
        <LuPlus class="h-4 w-4" />
        <span>{{ t('remote.addSource') }}</span>
      </button>
    </div>

    <div
      v-if="sources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] px-6 py-16 text-center"
    >
      <div
        class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuBookOpen v-if="protocol === 'opds'" class="h-6 w-6" />
        <LuDisc3 v-else class="h-6 w-6" />
      </div>
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
        {{
          protocol === 'opds'
            ? t('remote.opdsEmptySourcesTitle')
            : t('remote.subsonicEmptySourcesTitle')
        }}
      </p>
      <p class="mx-auto mt-1 max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{
          protocol === 'opds'
            ? t('remote.opdsEmptySourcesHint')
            : t('remote.subsonicEmptySourcesHint')
        }}
      </p>
      <div class="mt-5 flex flex-wrap justify-center gap-2">
        <button
          v-if="protocol === 'opds'"
          type="button"
          class="md-btn md-btn-sm md-btn-filled"
          @click="emit('add', 'opds')"
        >
          <LuBookOpen class="h-3.5 w-3.5" />
          <span>{{ t('remote.opdsAddBtn') }}</span>
        </button>
        <button
          v-else
          type="button"
          class="md-btn md-btn-sm md-btn-filled"
          @click="emit('add', 'subsonic')"
        >
          <LuDisc3 class="h-3.5 w-3.5" />
          <span>{{ t('remote.subsonicAddBtn') }}</span>
        </button>
      </div>
    </div>

    <div v-else class="grid grid-cols-1 gap-3 md:grid-cols-2">
      <article
        v-for="source in sources"
        :key="source.id"
        class="flex flex-col justify-between gap-3 rounded-[22px] border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-low)] p-4 transition-colors hover:bg-[var(--md-sys-color-surface-container)]"
      >
        <div class="min-w-0 space-y-1.5">
          <div class="flex items-center gap-2">
            <span
              class="inline-flex shrink-0 items-center gap-1 rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
              :class="
                source.protocol === 'opds'
                  ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                  : 'bg-[var(--md-sys-color-tertiary-container)] text-[var(--md-sys-color-on-tertiary-container)]'
              "
            >
              <LuBookOpen v-if="source.protocol === 'opds'" class="h-3 w-3" />
              <LuDisc3 v-else class="h-3 w-3" />
              <span>{{ source.protocol === 'opds' ? 'OPDS' : 'OpenSubsonic' }}</span>
            </span>

            <h3 class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ source.name || t('remote.unnamed') }}
            </h3>

            <span
              v-if="source.allow_insecure_tls"
              class="inline-flex shrink-0 items-center text-[var(--md-sys-color-on-surface-variant)]"
              :title="t('remote.insecureBadge')"
            >
              <LuShieldAlert class="h-3.5 w-3.5" />
            </span>
          </div>

          <p class="truncate font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ source.url }}
          </p>

          <div
            v-if="source.username || source.has_password"
            class="flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
          >
            <span
              v-if="source.username"
              class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono"
            >
              @{{ source.username }}
            </span>
            <span
              v-if="source.has_password"
              class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5"
            >
              {{ t('remote.passwordSavedBadge') }}
            </span>
          </div>

          <!-- Inline Test Result -->
          <div
            v-if="cardTestResults[source.id]"
            class="mt-1 flex items-center gap-1.5 rounded-xl px-2.5 py-1.5 text-[11px] font-medium"
            :class="
              cardTestResults[source.id]!.ok
                ? 'bg-[color-mix(in_srgb,var(--md-sys-color-success,#16a34a)_14%,transparent)] text-[var(--md-sys-color-success,#15803d)]'
                : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
            "
          >
            <LuCheckCircle2 v-if="cardTestResults[source.id]!.ok" class="h-3.5 w-3.5 shrink-0" />
            <LuAlertCircle v-else class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">
              <template v-if="cardTestResults[source.id]!.ok">
                {{ t('remote.testOk', { latency: cardTestResults[source.id]!.latency_ms }) }}
                <span v-if="cardTestResults[source.id]!.detail" class="opacity-85">
                  · {{ cardTestResults[source.id]!.detail }}
                </span>
              </template>
              <template v-else>
                {{ cardTestResults[source.id]!.error || t('remote.testFailed') }}
              </template>
            </span>
          </div>
        </div>

        <div class="flex items-center justify-between gap-2 pt-1">
          <button
            type="button"
            class="md-btn md-btn-tonal md-btn-sm"
            @click="emit('browse', source)"
          >
            <span>{{ t('remote.browseBtn') }}</span>
            <LuChevronRight class="h-3.5 w-3.5" />
          </button>

          <div class="flex items-center gap-1">
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="testingSourceId === source.id"
              :title="t('remote.testBtn')"
              :aria-label="t('remote.testBtn')"
              @click="emit('test', source)"
            >
              <LuLoader2 v-if="testingSourceId === source.id" class="h-4 w-4 animate-spin" />
              <LuWifi v-else class="h-4 w-4" />
            </button>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :title="t('remote.editBtn')"
              :aria-label="t('remote.editBtn')"
              @click="emit('edit', source)"
            >
              <LuPencil class="h-4 w-4" />
            </button>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state hover:text-[var(--md-sys-color-error)]"
              :disabled="deletingSourceId === source.id"
              :title="t('common.delete')"
              :aria-label="t('common.delete')"
              @click="emit('delete', source)"
            >
              <LuLoader2 v-if="deletingSourceId === source.id" class="h-4 w-4 animate-spin" />
              <LuTrash2 v-else class="h-4 w-4" />
            </button>
          </div>
        </div>
      </article>
    </div>
  </div>
</template>
