<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import {
  LuAlertCircle,
  LuArrowRight,
  LuCheck,
  LuCheckCircle2,
  LuEye,
  LuEyeOff,
  LuFolderOpen,
  LuLayers,
  LuLibrary,
  LuRefreshCw,
  LuSkipForward,
  LuSparkles,
  LuUserPlus,
} from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import ThemePopover from '@/components/ThemePopover.vue'
import { DOMAIN_ICONS, resolveLibraryRoute } from '@/components/settings/sections'
import {
  MdButton,
  MdCard,
  MdChip,
  MdIconButton,
  MdLinearProgress,
  MdTextField,
} from '@/components/ui'
import { localizeApiErrorMessage } from '@/i18n/apiErrors'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import type { CoreLibraryDto, MediaKind } from '@/types/api'

type OnboardingStep = 1 | 2 | 3

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()

const step = ref<OnboardingStep>(1)
const errorMsg = ref('')

// Step 1: Create Admin
const username = ref('admin')
const displayName = ref('')
const password = ref('')
const confirmPassword = ref('')
const showPassword = ref(false)
const creatingAdmin = ref(false)

// Step 2: Add First Media Folder
const enabledDomains = computed<MediaKind[]>(() => authStore.enabledModules)
const folderPath = ref('')
const libraryName = ref('')
const selectedDomain = ref<MediaKind>(enabledDomains.value[0] ?? 'book')
const domainManuallyPicked = ref(false)
const inferredDomain = ref<MediaKind | null>(null)
const mountingLibrary = ref(false)

// Step 3: Trigger Scan & Jump to Library
const createdLibrary = ref<CoreLibraryDto | null>(null)
const triggeringScan = ref(false)
const scanError = ref('')
let pollTimer: ReturnType<typeof setInterval> | null = null

const stepsMeta = computed(() => [
  {
    num: 1 as const,
    icon: LuUserPlus,
    label: t('setup.stepAdmin'),
  },
  {
    num: 2 as const,
    icon: LuFolderOpen,
    label: t('setup.stepFolder'),
  },
  {
    num: 3 as const,
    icon: LuLibrary,
    label: t('setup.stepScan'),
  },
])

function inferDomainFromPath(rawPath: string): MediaKind | null {
  const lower = rawPath.trim().toLowerCase()
  if (!lower) return null

  const rules: Array<{ domain: MediaKind; pattern: RegExp }> = [
    {
      domain: 'comic',
      pattern: /(?:^|[\\/_\-\s.])(comic|comics|manga|manhua|manhwa|cbz|cbr|漫画|本子)(?:$|[\\/_\-\s.])|漫画|连环画/,
    },
    {
      domain: 'music',
      pattern: /(?:^|[\\/_\-\s.])(music|audio|songs|albums|flac|mp3|lossless|音乐|歌曲|专辑|无损)(?:$|[\\/_\-\s.])|音乐|歌单/,
    },
    {
      domain: 'video',
      pattern: /(?:^|[\\/_\-\s.])(video|videos|movie|movies|film|films|tv|shows|series|anime|drama|bangumi|视频|影视|电影|剧集|番剧|动画)(?:$|[\\/_\-\s.])|影视|电影|剧集|番剧/,
    },
    {
      domain: 'book',
      pattern: /(?:^|[\\/_\-\s.])(book|books|ebook|ebooks|novel|novels|epub|pdf|kindle|read|书籍|图书|电子书|小说)(?:$|[\\/_\-\s.])|书籍|图书|电子书|小说/,
    },
  ]

  for (const rule of rules) {
    if (authStore.hasModule(rule.domain) && rule.pattern.test(lower)) {
      return rule.domain
    }
  }
  return null
}

function inferFolderBasename(rawPath: string): string {
  const clean = rawPath.trim().replace(/[\\/]+$/, '')
  if (!clean) return ''
  const parts = clean.split(/[\\/]/).filter(Boolean)
  return parts[parts.length - 1] ?? ''
}

watch(folderPath, (nextPath) => {
  const detected = inferDomainFromPath(nextPath)
  inferredDomain.value = detected
  if (detected && !domainManuallyPicked.value) {
    selectedDomain.value = detected
  }
})

function pickDomain(kind: MediaKind): void {
  selectedDomain.value = kind
  domainManuallyPicked.value = true
}

function stopPolling(): void {
  if (pollTimer !== null) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

onBeforeUnmount(() => {
  stopPolling()
})

async function onCreateAdmin(): Promise<void> {
  errorMsg.value = ''
  const trimmedUsername = username.value.trim()
  if (trimmedUsername.length < 2) {
    errorMsg.value = t('auth.usernamePlaceholder')
    return
  }
  if (password.value.length < 6) {
    errorMsg.value = t('auth.passwordPlaceholder')
    return
  }
  if (password.value !== confirmPassword.value) {
    errorMsg.value = t('auth.passwordMismatch')
    return
  }
  creatingAdmin.value = true
  try {
    await authStore.setupAdmin({
      username: trimmedUsername,
      display_name: displayName.value.trim() || undefined,
      password: password.value,
    })
    if (!authStore.hasModule(selectedDomain.value) && enabledDomains.value[0]) {
      selectedDomain.value = enabledDomains.value[0]
    }
    step.value = 2
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    creatingAdmin.value = false
  }
}

async function onMountFirstLibrary(): Promise<void> {
  errorMsg.value = ''
  const trimmedPath = folderPath.value.trim()
  if (!trimmedPath) {
    errorMsg.value = t('admin.libraryPathPlaceholder')
    return
  }
  const resolvedName =
    libraryName.value.trim() ||
    inferFolderBasename(trimmedPath) ||
    t(`app.domains.${selectedDomain.value}`)

  mountingLibrary.value = true
  try {
    const created = await api.createLibrary({
      name: resolvedName,
      media_kind: selectedDomain.value,
      root_path: trimmedPath,
    })
    createdLibrary.value = created
    domainStore.setActiveDomain(created.media_kind)
    eventsStore.bumpDomain(created.media_kind)
    step.value = 3
    await triggerInitialScan(created.id)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    mountingLibrary.value = false
  }
}

async function pollLibraryStatus(libraryId: string, mediaKind: MediaKind): Promise<void> {
  try {
    const libs = await api.listLibraries(mediaKind)
    const found = libs.find((l) => l.id === libraryId)
    if (!found) return
    createdLibrary.value = found
    eventsStore.bumpDomain(mediaKind)
    if (found.scan_status.state === 'completed' || found.scan_status.state === 'failed') {
      stopPolling()
    }
  } catch {
    // keep current state on transient poll error
  }
}

async function triggerInitialScan(libraryId: string): Promise<void> {
  scanError.value = ''
  triggeringScan.value = true
  stopPolling()
  try {
    const updated = await api.scanLibrary(libraryId)
    createdLibrary.value = updated
    eventsStore.bumpDomain(updated.media_kind)
    if (updated.scan_status.state === 'scanning' || updated.scan_status.state === 'idle') {
      pollTimer = setInterval(() => {
        void pollLibraryStatus(updated.id, updated.media_kind)
      }, 1200)
    }
  } catch (err) {
    scanError.value = err instanceof Error ? err.message : String(err)
  } finally {
    triggeringScan.value = false
  }
}

function skipForNow(): void {
  stopPolling()
  void router.replace('/')
}

function enterDomainLibrary(): void {
  stopPolling()
  const lib = createdLibrary.value
  const targetDomain = lib?.media_kind ?? selectedDomain.value
  domainStore.setActiveDomain(targetDomain)
  void router.replace(
    resolveLibraryRoute(router, targetDomain, lib ? { library: lib.id } : undefined),
  )
}

const scanProgressPercent = computed<number | null>(() => {
  const status = createdLibrary.value?.scan_status
  if (!status) return null
  if (status.state === 'completed') return 100
  if (status.state === 'scanning' && status.total && status.total > 0) {
    return Math.min(100, Math.round((status.scanned / status.total) * 100))
  }
  return null
})

const isScanCompleted = computed(() => createdLibrary.value?.scan_status.state === 'completed')
const isScanFailed = computed(
  () => Boolean(scanError.value) || createdLibrary.value?.scan_status.state === 'failed',
)

const scanStatusSummary = computed(() => {
  if (scanError.value) return scanError.value
  const status = createdLibrary.value?.scan_status
  if (!status || triggeringScan.value) {
    return t('setup.scanStarting')
  }
  if (status.state === 'scanning') {
    if (status.total && status.total > 0) {
      const pct = Math.min(100, Math.round((status.scanned / status.total) * 100))
      return t('admin.scanningWithTotal', { scanned: status.scanned, total: status.total, pct })
    }
    return t('admin.scanning', { scanned: status.scanned })
  }
  if (status.state === 'completed') {
    return t('admin.scanCompleted', {
      scanned: status.scanned,
      added: status.added,
      updated: status.updated,
      missing: status.missing,
    })
  }
  if (status.state === 'failed') {
    return t('admin.scanFailed', { message: localizeApiErrorMessage(status.message) })
  }
  return t('setup.scanQueued')
})
</script>

<template>
  <div class="safe-pb flex min-h-screen flex-col bg-surface text-on-surface">
    <!-- Top Brand Bar -->
    <header class="safe-pt safe-px">
      <div class="flex items-center justify-between px-6 py-4">
        <div class="flex items-center gap-2.5">
          <div
            class="flex h-10 w-10 items-center justify-center rounded-corner-lg bg-primary text-on-primary shadow-elevation-1"
          >
            <LuLayers class="h-5 w-5" />
          </div>
          <span class="text-title-medium tracking-tight">{{ t('app.name') }}</span>
        </div>
        <ThemePopover show-language />
      </div>
    </header>

    <main class="safe-px flex flex-1 items-center justify-center p-4">
      <MdCard
        variant="elevated"
        padding="lg"
        class="w-full max-w-lg rounded-corner-xl"
      >
        <!-- 3-Step Onboarding Stepper -->
        <ol
          class="mb-6 grid grid-cols-3 gap-2"
          :aria-label="t('setup.stepperLabel')"
        >
          <li
            v-for="item in stepsMeta"
            :key="item.num"
            class="flex flex-col items-center gap-1.5 text-center"
          >
            <div
              class="text-label-medium flex h-8 w-8 items-center justify-center rounded-corner-full transition-colors"
              :class="
                step > item.num
                  ? 'bg-primary text-on-primary'
                  : step === item.num
                    ? 'bg-primary-container text-on-primary-container ring-2 ring-primary/40'
                    : 'bg-surface-container-high text-on-surface-variant'
              "
            >
              <LuCheck v-if="step > item.num" class="h-4 w-4" />
              <span v-else>{{ item.num }}</span>
            </div>
            <span
              class="text-label-small truncate max-w-full"
              :class="step >= item.num ? 'text-on-surface' : 'text-on-surface-variant'"
            >
              {{ item.label }}
            </span>
          </li>
        </ol>

        <!-- Error Banner -->
        <div
          v-if="errorMsg"
          role="alert"
          class="text-body-small mb-5 flex items-start gap-2 rounded-corner-md bg-error-container px-4 py-3 text-on-error-container"
        >
          <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
          <span>{{ errorMsg }}</span>
        </div>

        <!-- STEP 1: Create Administrator -->
        <template v-if="step === 1">
          <div class="mb-6 flex items-center gap-3">
            <div
              class="flex h-11 w-11 shrink-0 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
            >
              <LuSparkles class="h-5 w-5" />
            </div>
            <div>
              <h1 class="text-title-large text-on-surface">
                {{ t('auth.setupTitle') }}
              </h1>
              <p class="text-body-small mt-0.5 text-on-surface-variant">
                {{ t('auth.setupSubtitle') }}
              </p>
            </div>
          </div>

          <form class="space-y-4" @submit.prevent="onCreateAdmin">
            <MdTextField
              v-model="username"
              type="text"
              required
              autocomplete="username"
              :label="t('auth.username')"
              :placeholder="t('auth.usernamePlaceholder')"
              :disabled="creatingAdmin"
            />

            <MdTextField
              v-model="displayName"
              type="text"
              :label="t('auth.displayName')"
              :placeholder="t('auth.displayNamePlaceholder')"
              :disabled="creatingAdmin"
            />

            <MdTextField
              v-model="password"
              :type="showPassword ? 'text' : 'password'"
              required
              autocomplete="new-password"
              :label="t('auth.password')"
              :placeholder="t('auth.passwordPlaceholder')"
              :disabled="creatingAdmin"
            >
              <template #trailing>
                <MdIconButton
                  size="sm"
                  :icon="showPassword ? LuEyeOff : LuEye"
                  :aria-label="showPassword ? t('auth.hidePassword') : t('auth.showPassword')"
                  @click="showPassword = !showPassword"
                />
              </template>
            </MdTextField>

            <MdTextField
              v-model="confirmPassword"
              :type="showPassword ? 'text' : 'password'"
              required
              autocomplete="new-password"
              :label="t('auth.confirmPassword')"
              :placeholder="t('auth.confirmPasswordPlaceholder')"
              :disabled="creatingAdmin"
            />

            <MdButton
              type="submit"
              variant="filled"
              block
              :loading="creatingAdmin"
              class="mt-2"
            >
              <span>{{
                creatingAdmin
                  ? t('common.loading')
                  : t('setup.createAdminNextBtn')
              }}</span>
              <template #trailing>
                <LuArrowRight v-if="!creatingAdmin" class="h-4 w-4" />
              </template>
            </MdButton>
          </form>
        </template>

        <!-- STEP 2: Add First Media Folder (or Skip) -->
        <template v-else-if="step === 2">
          <div class="mb-6 flex items-center gap-3">
            <div
              class="flex h-11 w-11 shrink-0 items-center justify-center rounded-corner-lg bg-secondary-container text-on-secondary-container"
            >
              <LuFolderOpen class="h-5 w-5" />
            </div>
            <div>
              <h1 class="text-title-large text-on-surface">
                {{ t('setup.addFolderTitle') }}
              </h1>
              <p class="text-body-small mt-0.5 text-on-surface-variant">
                {{ t('setup.addFolderSubtitle') }}
              </p>
            </div>
          </div>

          <form class="space-y-4" @submit.prevent="onMountFirstLibrary">
            <MdTextField
              v-model="folderPath"
              type="text"
              required
              autofocus
              :label="t('admin.libraryPath')"
              :placeholder="t('admin.libraryPathPlaceholder')"
              :supporting-text="
                inferredDomain
                  ? t('setup.inferredDomainHint', { domain: t(`app.domains.${inferredDomain}`) })
                  : t('admin.mountLibraryDesc')
              "
              :disabled="mountingLibrary"
            />

            <div>
              <span class="text-label-medium mb-2 block text-on-surface-variant">
                {{ t('admin.libraryKind') }}
              </span>
              <div class="flex flex-wrap items-center gap-2">
                <MdChip
                  v-for="kind in enabledDomains"
                  :key="kind"
                  variant="filter"
                  hide-check
                  :icon="DOMAIN_ICONS[kind]"
                  :label="t(`app.domains.${kind}`)"
                  :selected="selectedDomain === kind"
                  :disabled="mountingLibrary"
                  @click="pickDomain(kind)"
                />
              </div>
            </div>

            <MdTextField
              v-model="libraryName"
              type="text"
              :label="t('admin.libraryName')"
              :placeholder="inferFolderBasename(folderPath) || t('admin.libraryNamePlaceholder')"
              :supporting-text="t('setup.libraryNameOptional')"
              :disabled="mountingLibrary"
            />

            <div class="flex flex-col gap-2.5 pt-2">
              <MdButton
                type="submit"
                variant="filled"
                block
                :icon="LuLibrary"
                :loading="mountingLibrary"
                :disabled="!folderPath.trim()"
              >
                {{ t('setup.mountAndScanNextBtn') }}
              </MdButton>

              <MdButton
                type="button"
                variant="text"
                block
                :icon="LuSkipForward"
                :disabled="mountingLibrary"
                @click="skipForNow"
              >
                {{ t('setup.skipForNowBtn') }}
              </MdButton>
            </div>
          </form>
        </template>

        <!-- STEP 3: Trigger Scan & Jump Straight to Domain Library -->
        <template v-else>
          <div class="mb-5 flex items-center gap-3">
            <div
              class="flex h-11 w-11 shrink-0 items-center justify-center rounded-corner-lg"
              :class="
                isScanFailed
                  ? 'bg-error-container text-on-error-container'
                  : isScanCompleted
                    ? 'bg-success-container text-on-success-container'
                    : 'bg-primary-container text-on-primary-container'
              "
            >
              <LuAlertCircle v-if="isScanFailed" class="h-5 w-5" />
              <LuCheckCircle2 v-else-if="isScanCompleted" class="h-5 w-5" />
              <LuRefreshCw v-else class="h-5 w-5 animate-spin" />
            </div>
            <div>
              <h1 class="text-title-large text-on-surface">
                {{
                  isScanCompleted
                    ? t('setup.scanReadyTitle')
                    : isScanFailed
                      ? t('setup.scanErrorTitle')
                      : t('setup.scanningTitle')
                }}
              </h1>
              <p class="text-body-small mt-0.5 text-on-surface-variant">
                {{ t('setup.scanStepHint') }}
              </p>
            </div>
          </div>

          <div
            v-if="createdLibrary"
            class="mb-5 space-y-3 rounded-corner-md bg-surface-container p-4"
          >
            <div class="flex flex-wrap items-center gap-2">
              <span
                class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
              >
                <component :is="DOMAIN_ICONS[createdLibrary.media_kind]" class="h-3 w-3" />
                <span>{{ t(`app.domains.${createdLibrary.media_kind}`) }}</span>
              </span>
              <span class="text-title-small text-on-surface">{{ createdLibrary.name }}</span>
            </div>

            <p
              v-if="createdLibrary.kind.type === 'local_mount'"
              class="text-body-small truncate font-mono text-on-surface-variant"
            >
              {{ createdLibrary.kind.root_path }}
            </p>

            <MdLinearProgress
              :value="scanProgressPercent"
              :indeterminate="!isScanCompleted && !isScanFailed && scanProgressPercent === null"
              :tone="isScanFailed ? 'error' : isScanCompleted ? 'success' : 'primary'"
              :aria-label="scanStatusSummary"
            />

            <p
              class="text-body-small"
              :class="isScanFailed ? 'text-error' : 'text-on-surface-variant'"
            >
              {{ scanStatusSummary }}
            </p>
          </div>

          <div class="flex flex-col gap-2.5">
            <MdButton
              variant="filled"
              block
              :disabled="triggeringScan"
              @click="enterDomainLibrary"
            >
              <span>{{
                t('setup.openDomainLibraryBtn', {
                  domain: t(`app.domains.${createdLibrary?.media_kind ?? selectedDomain}`),
                })
              }}</span>
              <template #trailing>
                <LuArrowRight class="h-4 w-4" />
              </template>
            </MdButton>

            <MdButton
              v-if="isScanFailed && createdLibrary"
              variant="tonal"
              block
              :icon="LuRefreshCw"
              :loading="triggeringScan"
              @click="triggerInitialScan(createdLibrary.id)"
            >
              {{ t('setup.retryScanBtn') }}
            </MdButton>
          </div>
        </template>
      </MdCard>
    </main>
  </div>
</template>
