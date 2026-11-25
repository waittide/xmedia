<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuExternalLink,
  LuKey,
  LuRotateCcw,
  LuSave,
  LuSettings,
  LuShieldAlert,
} from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { toolApi } from '../../api'
import { ADULT_PROVIDER_IDS } from '../../providers'
import type {
  ProviderConfigOverrideDto,
  ProviderStatusDto,
  PublicSettingsDto,
  UpdateToolSettingsInput,
} from '../../types'

const props = defineProps<{
  open: boolean
  provider: ProviderStatusDto | null
  settings: PublicSettingsDto | null
}>()

const emit = defineEmits<{
  close: []
  updated: [settings: PublicSettingsDto]
}>()

const { t } = useI18n()

const localeOptions = computed(() => [
  { value: '', label: t('tool.settings.providerDialog.inheritLocale') },
  { value: 'zh-CN', label: '简体中文 (zh-CN)' },
  { value: 'zh-TW', label: '繁體中文 (zh-TW)' },
  { value: 'ja-JP', label: '日本語 (ja-JP)' },
  { value: 'en-US', label: 'English (en-US)' },
])

const enabled = ref(true)
const httpProxy = ref('')
const locale = ref('')
const timeoutSecs = ref<string>('')
const userAgent = ref('')
const secretInput = ref('')
const saving = ref(false)
const clearingSecret = ref(false)
const errorMsg = ref<string | null>(null)

const providerId = computed(() => props.provider?.id ?? '')
const isAdultProvider = computed(() => ADULT_PROVIDER_IDS.has(providerId.value))

const secretSpec = computed<{
  label: string
  placeholder: string
  hasSecret: boolean
  masked: string | null
  field: 'tmdb_api_key' | 'bangumi_access_token' | 'qishui_cookie'
} | null>(() => {
  if (providerId.value === 'tmdb') {
    return {
      label: t('tool.settings.providerDialog.tmdbLabel'),
      placeholder: t('tool.settings.providerDialog.tmdbPlaceholder'),
      hasSecret: Boolean(props.settings?.has_tmdb_api_key),
      masked: props.settings?.tmdb_api_key_masked ?? null,
      field: 'tmdb_api_key',
    }
  }
  if (providerId.value === 'bangumi') {
    return {
      label: t('tool.settings.providerDialog.bangumiLabel'),
      placeholder: t('tool.settings.providerDialog.bangumiPlaceholder'),
      hasSecret: Boolean(props.settings?.has_bangumi_token),
      masked: props.settings?.bangumi_token_masked ?? null,
      field: 'bangumi_access_token',
    }
  }
  if (providerId.value === 'qishui') {
    return {
      label: t('tool.settings.providerDialog.qishuiLabel'),
      placeholder: t('tool.settings.providerDialog.qishuiPlaceholder'),
      hasSecret: Boolean(props.settings?.has_qishui_cookie),
      masked: props.settings?.qishui_cookie_masked ?? null,
      field: 'qishui_cookie',
    }
  }
  return null
})

const hasCustomOverrides = computed(() =>
  Boolean(
    httpProxy.value.trim() ||
      locale.value.trim() ||
      timeoutSecs.value.trim() ||
      userAgent.value.trim(),
  ),
)

watch(
  () => [props.open, props.provider?.id, props.settings] as const,
  ([isOpen, id, s]) => {
    if (!isOpen || !id) return
    errorMsg.value = null
    secretInput.value = ''
    const cfg: ProviderConfigOverrideDto | undefined = s?.provider_configs?.[id]
    enabled.value = props.provider?.enabled ?? cfg?.enabled ?? !isAdultProvider.value
    httpProxy.value = cfg?.http_proxy ?? ''
    locale.value = cfg?.locale ?? ''
    timeoutSecs.value = cfg?.timeout_secs ? String(cfg.timeout_secs) : ''
    userAgent.value = cfg?.user_agent ?? ''
  },
  { immediate: true },
)

function resetNetworkOverrides(): void {
  httpProxy.value = ''
  locale.value = ''
  timeoutSecs.value = ''
  userAgent.value = ''
}

async function handleClearSecret(): Promise<void> {
  if (!secretSpec.value || clearingSecret.value) return
  clearingSecret.value = true
  errorMsg.value = null
  try {
    const payload: UpdateToolSettingsInput = { [secretSpec.value.field]: '' }
    const next = await toolApi.updateSettings(payload)
    emit('updated', next)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    clearingSecret.value = false
  }
}

async function handleSave(): Promise<void> {
  if (!providerId.value || saving.value) return
  saving.value = true
  errorMsg.value = null
  try {
    const parsedTimeout = timeoutSecs.value.trim()
      ? Math.max(5, Math.min(120, Math.round(Number(timeoutSecs.value))))
      : null
    const override: ProviderConfigOverrideDto = {
      enabled: enabled.value,
      http_proxy: httpProxy.value.trim() || null,
      locale: locale.value.trim() || null,
      timeout_secs: parsedTimeout && Number.isFinite(parsedTimeout) ? parsedTimeout : null,
      user_agent: userAgent.value.trim() || null,
    }
    const payload: UpdateToolSettingsInput = {
      provider_configs: {
        [providerId.value]: override,
      },
    }
    if (secretSpec.value && secretInput.value.trim()) {
      payload[secretSpec.value.field] = secretInput.value.trim()
    }
    const next = await toolApi.updateSettings(payload)
    secretInput.value = ''
    emit('updated', next)
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
    :open="open && Boolean(provider)"
    :title="
      provider
        ? t('tool.settings.providerDialog.titleWithProvider', { name: provider.name })
        : t('tool.settings.providerDialog.titleFallback')
    "
    :subtitle="provider?.description"
    max-width="max-w-lg"
    @close="!saving && emit('close')"
  >
    <template #icon>
      <LuSettings class="h-5 w-5 text-primary" aria-hidden="true" />
    </template>

    <div v-if="provider" class="space-y-4 py-1">
      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        {{ errorMsg }}
      </div>

      <!-- Enable Switch + Website Link -->
      <div
        class="flex items-center justify-between gap-3 rounded-corner-lg bg-surface-container px-4 py-3"
      >
        <div class="min-w-0">
          <div class="text-title-small text-on-surface">
            {{ t('tool.settings.providerDialog.enableProvider') }}
          </div>
          <a
            v-if="provider.website"
            :href="provider.website"
            target="_blank"
            rel="noopener noreferrer"
            class="text-body-small mt-0.5 inline-flex items-center gap-1 text-primary hover:underline"
          >
            <span class="truncate">{{ provider.website }}</span>
            <LuExternalLink class="h-3 w-3 shrink-0" aria-hidden="true" />
          </a>
        </div>
        <MdSwitch
          v-model="enabled"
          :aria-label="t('tool.settings.providerDialog.enableProvider')"
        />
      </div>

      <!-- Adult / R18 Provider Advisory -->
      <div
        v-if="isAdultProvider"
        class="text-body-small flex items-start gap-2.5 rounded-corner-lg bg-warning-container/75 px-3.5 py-3 text-on-warning-container"
      >
        <LuShieldAlert class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <p class="leading-relaxed">
          {{ t('tool.settings.providerDialog.adultAdvisory') }}
        </p>
      </div>

      <!-- Provider-Specific Secret Credential -->
      <div
        v-if="secretSpec"
        class="space-y-2.5 rounded-corner-lg bg-surface-container p-4"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="text-title-small flex items-center gap-1.5 text-on-surface">
            <LuKey class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ secretSpec.label }}</span>
          </div>
          <div v-if="secretSpec.hasSecret" class="flex items-center gap-2">
            <span
              class="text-label-small rounded-corner-full bg-success-container px-2.5 py-0.5 font-mono text-on-success-container"
            >
              {{ t('tool.settings.providerDialog.configuredPrefix', { masked: secretSpec.masked ?? '' }) }}
            </span>
            <MdButton
              variant="text"
              size="sm"
              tone="error"
              :disabled="clearingSecret"
              @click="handleClearSecret"
            >
              {{ t('tool.settings.providerDialog.clearSecret') }}
            </MdButton>
          </div>
        </div>
        <MdTextField
          v-model="secretInput"
          type="password"
          size="sm"
          :aria-label="secretSpec.label"
          :placeholder="secretSpec.placeholder"
        />
      </div>

      <!-- Independent Network & Locale Overrides -->
      <div class="space-y-3 rounded-corner-lg bg-surface-container p-4">
        <div class="flex items-center justify-between gap-2">
          <div>
            <div class="text-title-small text-on-surface">
              {{ t('tool.settings.providerDialog.overridesTitle') }}
            </div>
            <p class="text-body-small mt-0.5 text-on-surface-variant">
              {{ t('tool.settings.providerDialog.overridesDesc') }}
            </p>
          </div>
          <MdButton
            v-if="hasCustomOverrides"
            variant="text"
            size="sm"
            :icon="LuRotateCcw"
            @click="resetNetworkOverrides"
          >
            {{ t('tool.settings.providerDialog.resetInherit') }}
          </MdButton>
        </div>

        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div class="flex flex-col gap-1">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('tool.settings.providerDialog.overrideLocale') }}
            </span>
            <MdSelect v-model="locale" :options="localeOptions" size="sm" />
          </div>

          <MdTextField
            v-model="timeoutSecs"
            type="number"
            size="sm"
            min="5"
            max="120"
            :label="t('tool.settings.providerDialog.overrideTimeout')"
            :placeholder="t('tool.settings.providerDialog.timeoutPlaceholder')"
          />
        </div>

        <MdTextField
          v-model="httpProxy"
          size="sm"
          :label="t('tool.settings.providerDialog.overrideProxy')"
          :placeholder="
            settings?.http_proxy
              ? t('tool.settings.providerDialog.inheritProxyPlaceholder', {
                  proxy: settings.http_proxy,
                })
              : t('tool.settings.providerDialog.proxyPlaceholder')
          "
        />

        <MdTextField
          v-model="userAgent"
          size="sm"
          :label="t('tool.settings.providerDialog.overrideUa')"
          :placeholder="t('tool.settings.providerDialog.uaPlaceholder')"
        />
      </div>
    </div>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <MdButton variant="text" size="sm" :disabled="saving" @click="emit('close')">
          {{ t('tool.common.cancel') }}
        </MdButton>
        <MdButton variant="filled" size="sm" :icon="LuSave" :loading="saving" @click="handleSave">
          {{ saving ? t('tool.common.saving') : t('tool.settings.providerDialog.saveProvider') }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
