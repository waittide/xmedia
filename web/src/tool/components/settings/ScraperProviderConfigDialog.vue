<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuExternalLink,
  LuKey,
  LuLoader2,
  LuRotateCcw,
  LuSave,
  LuSettings,
  LuShieldAlert,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import { toolApi } from '../../api'
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

const localeOptions = [
  { value: '', label: '继承全局默认语言' },
  { value: 'zh-CN', label: '简体中文 (zh-CN)' },
  { value: 'zh-TW', label: '繁體中文 (zh-TW)' },
  { value: 'ja-JP', label: '日本語 (ja-JP)' },
  { value: 'en-US', label: 'English (en-US)' },
]

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
const isAdultProvider = computed(
  () => providerId.value === 'nhentai' || providerId.value === 'hanime1',
)

const secretSpec = computed<{
  label: string
  placeholder: string
  hasSecret: boolean
  masked: string | null
  field: 'tmdb_api_key' | 'bangumi_access_token' | 'qishui_cookie'
} | null>(() => {
  if (providerId.value === 'tmdb') {
    return {
      label: 'TMDB API Key / v4 Read Access Token',
      placeholder: '输入新的 TMDB v3 API Key 或 v4 Bearer Token（留空保持不变）',
      hasSecret: Boolean(props.settings?.has_tmdb_api_key),
      masked: props.settings?.tmdb_api_key_masked ?? null,
      field: 'tmdb_api_key',
    }
  }
  if (providerId.value === 'bangumi') {
    return {
      label: 'Bangumi Access Token（可选，解锁更高频限额）',
      placeholder: '输入 Bangumi 个人令牌（留空保持不变）',
      hasSecret: Boolean(props.settings?.has_bangumi_token),
      masked: props.settings?.bangumi_token_masked ?? null,
      field: 'bangumi_access_token',
    }
  }
  if (providerId.value === 'qishui') {
    return {
      label: '汽水音乐 Cookie（可选，增强歌词与曲目抓取）',
      placeholder: '输入汽水音乐请求 Cookie（留空保持不变）',
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
    :title="provider ? `${provider.name} · 刮削源设置` : '刮削源设置'"
    :subtitle="provider?.description"
    max-width="max-w-lg"
    @close="!saving && emit('close')"
  >
    <template #icon>
      <LuSettings class="h-5 w-5 text-[var(--md-sys-color-primary)]" />
    </template>

    <div v-if="provider" class="space-y-4 py-1">
      <div
        v-if="errorMsg"
        class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ errorMsg }}
      </div>

      <!-- Enable Switch + Website Link -->
      <div
        class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-4 py-3"
      >
        <div class="min-w-0">
          <div class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            启用此刮削源
          </div>
          <a
            v-if="provider.website"
            :href="provider.website"
            target="_blank"
            rel="noopener noreferrer"
            class="mt-0.5 inline-flex items-center gap-1 text-[11px] text-[var(--md-sys-color-primary)] hover:underline"
          >
            <span class="truncate">{{ provider.website }}</span>
            <LuExternalLink class="h-3 w-3 shrink-0" />
          </a>
        </div>
        <AppSwitch v-model="enabled" />
      </div>

      <!-- Adult / R18 Provider Advisory -->
      <div
        v-if="isAdultProvider"
        class="flex items-start gap-2.5 rounded-2xl bg-[var(--md-sys-color-warning-container)]/75 px-3.5 py-3 text-xs text-[var(--md-sys-color-on-warning-container)]"
      >
        <LuShieldAlert class="mt-0.5 h-4 w-4 shrink-0" />
        <p class="leading-relaxed">
          此源为 R18 / 成人向刮削源，默认关闭。建议按需启用并配置独立代理或全局代理。
        </p>
      </div>

      <!-- Provider-Specific Secret Credential -->
      <div
        v-if="secretSpec"
        class="space-y-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-1.5 text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            <LuKey class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
            <span>{{ secretSpec.label }}</span>
          </div>
          <div class="flex items-center gap-2">
            <span
              v-if="secretSpec.hasSecret"
              class="rounded-full bg-[var(--md-sys-color-success-container)] px-2.5 py-0.5 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-success-container)]"
            >
              已配置 {{ secretSpec.masked }}
            </span>
            <button
              v-if="secretSpec.hasSecret"
              type="button"
              :disabled="clearingSecret"
              class="text-[11px] font-medium text-[var(--md-sys-color-error)] hover:underline disabled:opacity-50"
              @click="handleClearSecret"
            >
              清空
            </button>
          </div>
        </div>
        <input
          v-model="secretInput"
          type="password"
          :placeholder="secretSpec.placeholder"
          class="md-input h-10 w-full px-3.5 font-mono text-xs"
        />
      </div>

      <!-- Independent Network & Locale Overrides -->
      <div class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <div class="flex items-center justify-between gap-2">
          <div>
            <div class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              独立请求设置（可选）
            </div>
            <p class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              留空则自动继承全局默认语言、全局代理与默认超时
            </p>
          </div>
          <button
            v-if="hasCustomOverrides"
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state"
            @click="resetNetworkOverrides"
          >
            <LuRotateCcw class="h-3.5 w-3.5" />
            <span>恢复继承</span>
          </button>
        </div>

        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div>
            <label class="mb-1.5 block text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]">
              独立抓取语言
            </label>
            <AppSelect v-model="locale" :options="localeOptions" size="sm" />
          </div>

          <div>
            <label class="mb-1.5 block text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]">
              独立超时 (秒，5–120)
            </label>
            <input
              v-model="timeoutSecs"
              type="number"
              min="5"
              max="120"
              placeholder="默认 20"
              class="md-input h-9 w-full px-3 font-mono text-xs"
            />
          </div>
        </div>

        <div>
          <label class="mb-1.5 block text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]">
            独立 HTTP / SOCKS5 代理
          </label>
          <input
            v-model="httpProxy"
            type="text"
            :placeholder="
              settings?.http_proxy
                ? `留空继承全局代理：${settings.http_proxy}`
                : '例如 http://127.0.0.1:7890 或 socks5h://127.0.0.1:1080'
            "
            class="md-input h-9 w-full px-3 font-mono text-xs"
          />
        </div>

        <div>
          <label class="mb-1.5 block text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]">
            自定义 User-Agent
          </label>
          <input
            v-model="userAgent"
            type="text"
            placeholder="留空使用默认 User-Agent"
            class="md-input h-9 w-full px-3 font-mono text-xs"
          />
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          :disabled="saving"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="saving"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleSave"
        >
          <LuLoader2 v-if="saving" class="h-3.5 w-3.5 animate-spin" />
          <LuSave v-else class="h-3.5 w-3.5" />
          <span>{{ saving ? '保存中…' : '保存源设置' }}</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
