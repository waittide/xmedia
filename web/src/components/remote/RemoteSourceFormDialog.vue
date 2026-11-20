<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuBookOpen,
  LuCheckCircle2,
  LuCloud,
  LuDisc3,
  LuLoader2,
} from 'vue-icons-plus/lu'
import type {
  RemoteProtocol,
  RemoteSourceSummaryDto,
  TestRemoteSourceResponse,
} from '@/api/appHost'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useAppClientStore } from '@/stores/appServers'
import { useSnackbarStore } from '@/stores/snackbar'

const props = withDefaults(
  defineProps<{
    open: boolean
    editingSource?: RemoteSourceSummaryDto | null
    defaultProtocol?: RemoteProtocol
  }>(),
  {
    editingSource: null,
    defaultProtocol: 'opds',
  },
)

const emit = defineEmits<{
  close: []
  saved: [source: RemoteSourceSummaryDto]
}>()

const { t } = useI18n()
const appClientStore = useAppClientStore()
const snackbarStore = useSnackbarStore()

const protocol = ref<RemoteProtocol>('opds')
const name = ref('')
const url = ref('')
const username = ref('')
const password = ref('')
const allowInsecureTls = ref(false)
const testing = ref(false)
const saving = ref(false)
const errorMsg = ref('')
const testResult = ref<TestRemoteSourceResponse | null>(null)

const isEditing = computed(() => props.editingSource !== null)
const isBusy = computed(() => testing.value || saving.value)

const protocolOptions = computed<SegmentedOption<RemoteProtocol>[]>(() => [
  { value: 'opds', label: t('remote.protocolOpds'), icon: LuBookOpen },
  { value: 'subsonic', label: t('remote.protocolSubsonic'), icon: LuDisc3 },
])

const urlPlaceholder = computed(() =>
  protocol.value === 'opds'
    ? t('remote.urlPlaceholderOpds')
    : t('remote.urlPlaceholderSubsonic'),
)

const passwordPlaceholder = computed(() =>
  isEditing.value && props.editingSource?.has_password
    ? t('remote.passwordKeepPlaceholder')
    : t('remote.passwordPlaceholder'),
)

function normalizeUrlInput(raw: string): string {
  const trimmed = raw.trim()
  if (!trimmed) return ''
  if (/^[a-zA-Z][a-zA-Z\d+\-.]*:\/\//.test(trimmed)) {
    return trimmed
  }
  return `http://${trimmed}`
}

function extractDefaultName(raw: string): string {
  const normalized = normalizeUrlInput(raw)
  if (!normalized) return ''
  try {
    const parsed = new URL(normalized)
    return parsed.host || normalized
  } catch {
    return normalized.replace(/^https?:\/\//i, '').replace(/\/.*$/, '') || normalized
  }
}

watch(
  () => [props.open, props.editingSource, props.defaultProtocol] as const,
  ([isOpen, src, defProto]) => {
    if (!isOpen) return
    errorMsg.value = ''
    testResult.value = null
    if (src) {
      protocol.value = src.protocol
      name.value = src.name
      url.value = src.url
      username.value = src.username
      password.value = ''
      allowInsecureTls.value = src.allow_insecure_tls
    } else {
      protocol.value = defProto
      name.value = ''
      url.value = ''
      username.value = ''
      password.value = ''
      allowInsecureTls.value = false
    }
  },
  { immediate: true },
)

watch([protocol, url, username, password, allowInsecureTls], () => {
  testResult.value = null
  if (errorMsg.value) errorMsg.value = ''
})

async function handleTest(): Promise<void> {
  const normalized = normalizeUrlInput(url.value)
  if (!normalized) {
    errorMsg.value = t('remote.urlRequired')
    return
  }

  errorMsg.value = ''
  testResult.value = null
  testing.value = true
  try {
    const res = await appClientStore.testSource({
      protocol: protocol.value,
      url: normalized,
      username: username.value.trim() || undefined,
      password: password.value || undefined,
      allow_insecure_tls: allowInsecureTls.value,
    })
    testResult.value = res
  } catch (err) {
    testResult.value = {
      ok: false,
      latency_ms: 0,
      protocol: protocol.value,
      error: err instanceof Error ? err.message : String(err),
    }
  } finally {
    testing.value = false
  }
}

async function handleSubmit(): Promise<void> {
  const normalized = normalizeUrlInput(url.value)
  if (!normalized) {
    errorMsg.value = t('remote.urlRequired')
    return
  }

  errorMsg.value = ''
  saving.value = true
  try {
    const resolvedName = name.value.trim() || extractDefaultName(normalized)
    let saved: RemoteSourceSummaryDto
    if (props.editingSource) {
      saved = await appClientStore.updateSource(props.editingSource.id, {
        name: resolvedName,
        protocol: protocol.value,
        url: normalized,
        username: username.value.trim(),
        password: password.value ? password.value : undefined,
        allow_insecure_tls: allowInsecureTls.value,
      })
      snackbarStore.show(t('remote.updatedMsg'))
    } else {
      saved = await appClientStore.createSource({
        name: resolvedName,
        protocol: protocol.value,
        url: normalized,
        username: username.value.trim() || undefined,
        password: password.value || undefined,
        allow_insecure_tls: allowInsecureTls.value,
      })
      snackbarStore.show(t('remote.savedMsg'))
    }
    emit('saved', saved)
    emit('close')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="isEditing ? t('remote.editSource') : t('remote.addSource')"
    max-width="max-w-lg"
    @close="!isBusy && emit('close')"
  >
    <template #icon>
      <LuCloud class="h-5 w-5" />
    </template>

    <form id="remote-source-form" class="space-y-4 py-1" @submit.prevent="handleSubmit">
      <div
        v-if="errorMsg"
        class="flex items-start gap-2 rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
        <span class="break-all">{{ errorMsg }}</span>
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('remote.protocolLabel') }}
        </label>
        <MdSegmentedButton
          v-model="protocol"
          :options="protocolOptions"
          size="sm"
          :show-check="false"
        />
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('remote.urlLabel') }}
        </label>
        <input
          v-model="url"
          type="text"
          inputmode="url"
          autocapitalize="off"
          spellcheck="false"
          required
          :disabled="isBusy"
          :placeholder="urlPlaceholder"
          class="md-input h-10 w-full font-mono text-sm"
        />
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('remote.nameLabel') }}
        </label>
        <input
          v-model="name"
          type="text"
          :disabled="isBusy"
          :placeholder="t('remote.namePlaceholder')"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('remote.usernameLabel') }}
          </label>
          <input
            v-model="username"
            type="text"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            :disabled="isBusy"
            :placeholder="t('remote.usernamePlaceholder')"
            class="md-input h-10 w-full text-sm"
          />
        </div>

        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('remote.passwordLabel') }}
          </label>
          <input
            v-model="password"
            type="password"
            autocomplete="new-password"
            :disabled="isBusy"
            :placeholder="passwordPlaceholder"
            class="md-input h-10 w-full text-sm"
          />
        </div>
      </div>

      <div
        class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-4 py-3"
      >
        <AppSwitch
          v-model="allowInsecureTls"
          :disabled="isBusy"
          :label="t('remote.allowInsecureTls')"
        />
      </div>

      <div
        v-if="testResult"
        class="flex items-center gap-2 rounded-[var(--md-sys-shape-corner-md)] px-3.5 py-2.5 text-xs font-medium"
        :class="
          testResult.ok
            ? 'bg-[color-mix(in_srgb,var(--md-sys-color-success,#16a34a)_14%,transparent)] text-[var(--md-sys-color-success,#15803d)]'
            : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
        "
      >
        <LuCheckCircle2 v-if="testResult.ok" class="h-4 w-4 shrink-0" />
        <LuAlertCircle v-else class="h-4 w-4 shrink-0" />
        <span class="min-w-0 flex-1 break-all">
          <template v-if="testResult.ok">
            {{ t('remote.testOk', { latency: testResult.latency_ms }) }}
            <span v-if="testResult.detail" class="ml-1.5 opacity-85">
              · {{ testResult.detail }}
            </span>
          </template>
          <template v-else>
            {{ testResult.error || t('remote.testFailed') }}
          </template>
        </span>
      </div>
    </form>

    <template #footer>
      <button
        type="button"
        :disabled="isBusy || !url.trim()"
        class="md-btn md-btn-tonal md-btn-sm"
        @click="handleTest"
      >
        <LuLoader2 v-if="testing" class="h-3.5 w-3.5 animate-spin" />
        <span>{{ testing ? t('remote.testingBtn') : t('remote.testBtn') }}</span>
      </button>

      <div class="flex items-center gap-2">
        <button
          type="button"
          :disabled="isBusy"
          class="md-btn md-btn-text md-btn-sm"
          @click="emit('close')"
        >
          {{ t('common.cancel') }}
        </button>
        <button
          type="submit"
          form="remote-source-form"
          :disabled="isBusy || !url.trim()"
          class="md-btn md-btn-filled md-btn-sm"
        >
          <LuLoader2 v-if="saving" class="h-3.5 w-3.5 animate-spin" />
          <span>{{ saving ? t('remote.savingBtn') : t('remote.saveBtn') }}</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
