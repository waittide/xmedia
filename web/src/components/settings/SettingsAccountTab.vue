<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuCopy, LuEye, LuEyeOff, LuRotateCw } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useCopyFeedback } from './clipboard'

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()
const { copiedKey, copyText } = useCopyFeedback()

const currentPwd = ref('')
const nextPwd = ref('')
const pwdSuccess = ref('')
const savingPwd = ref(false)
const rotatingKey = ref(false)
const showApiKey = ref(false)

async function submitPasswordChange(): Promise<void> {
  emit('error', '')
  pwdSuccess.value = ''
  savingPwd.value = true
  try {
    const session = await api.changePassword({
      current_password: currentPwd.value,
      new_password: nextPwd.value,
    })
    authStore.setUser(session.user)
    currentPwd.value = ''
    nextPwd.value = ''
    pwdSuccess.value = t('auth.passwordUpdated')
    snackbarStore.show(t('auth.passwordUpdated'))
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    savingPwd.value = false
  }
}

async function handleRotateApiKey(): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('settings.rotateApiKey'),
    message: t('settings.rotateApiKeyConfirm'),
    danger: true,
  })
  if (!ok) return
  emit('error', '')
  rotatingKey.value = true
  try {
    const updated = await api.rotateApiKey()
    authStore.setUser(updated)
    snackbarStore.show(t('settings.apiKeyRotated'))
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    emit('error', msg)
    snackbarStore.show({ message: msg, tone: 'error' })
  } finally {
    rotatingKey.value = false
  }
}
</script>

<template>
  <section class="md-card-outlined space-y-4 p-6">
    <div>
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.apiKeyTitle') }}
      </h2>
      <p class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('settings.apiKeyDesc') }}
      </p>
    </div>

    <div class="flex flex-wrap items-center gap-2">
      <div class="relative min-w-0 flex-1">
        <input
          readonly
          :type="showApiKey ? 'text' : 'password'"
          :value="authStore.user?.api_key || ''"
          :aria-label="t('settings.apiKeyTitle')"
          autocomplete="off"
          class="md-input h-10 w-full !pr-10 font-mono text-xs"
        />
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state absolute top-1/2 right-1 -translate-y-1/2"
          :aria-label="showApiKey ? t('common.close') : t('settings.apiKeyTitle')"
          :aria-pressed="showApiKey"
          @click="showApiKey = !showApiKey"
        >
          <LuEyeOff v-if="showApiKey" class="h-4 w-4" />
          <LuEye v-else class="h-4 w-4" />
        </button>
      </div>
      <button
        type="button"
        class="md-btn md-btn-filled"
        @click="copyText('apiKey', authStore.user?.api_key || '')"
      >
        <LuCheck v-if="copiedKey === 'apiKey'" class="h-4 w-4" />
        <LuCopy v-else class="h-4 w-4" />
        <span>{{ copiedKey === 'apiKey' ? t('common.copied') : t('common.copy') }}</span>
      </button>
      <button
        type="button"
        :disabled="rotatingKey"
        class="md-btn md-btn-outlined !border-[var(--md-sys-color-error)]/50 !text-[var(--md-sys-color-error)]"
        @click="handleRotateApiKey"
      >
        <LuRotateCw class="h-4 w-4" :class="rotatingKey ? 'animate-spin' : ''" />
        <span>{{ t('settings.rotateApiKey') }}</span>
      </button>
    </div>
  </section>

  <section class="md-card-outlined p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('auth.password') }}
    </h2>

    <div
      v-if="pwdSuccess"
      class="mt-3 rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-success-container)] px-3.5 py-2.5 text-xs font-medium text-[var(--md-sys-color-on-success-container)]"
    >
      {{ pwdSuccess }}
    </div>

    <form class="mt-4 max-w-md space-y-4" @submit.prevent="submitPasswordChange">
      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('auth.currentPassword') }}
        </label>
        <input
          v-model="currentPwd"
          type="password"
          required
          autocomplete="current-password"
          class="md-input h-10 w-full text-xs"
        />
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('auth.newPassword') }}
        </label>
        <input
          v-model="nextPwd"
          type="password"
          required
          minlength="6"
          autocomplete="new-password"
          :placeholder="t('auth.passwordPlaceholder')"
          class="md-input h-10 w-full text-xs"
        />
      </div>

      <button
        type="submit"
        :disabled="savingPwd"
        class="md-btn md-btn-filled"
      >
        {{ t('common.save') }}
      </button>
    </form>
  </section>
</template>
