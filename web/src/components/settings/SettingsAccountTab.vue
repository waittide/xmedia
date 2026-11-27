<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuCopy, LuEye, LuEyeOff, LuRotateCw, LuShieldCheck, LuUser } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { MdButton, MdCard, MdIconButton, MdTextField } from '@/components/ui'
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

const displayLabel = computed(
  () => authStore.user?.display_name || authStore.user?.username || '',
)

const avatarInitial = computed(() => {
  const raw = displayLabel.value.trim()
  return raw ? raw.charAt(0).toUpperCase() : '?'
})

async function submitPasswordChange(): Promise<void> {
  emit('error', '')
  pwdSuccess.value = ''
  if (nextPwd.value.length < 6) {
    emit('error', t('auth.passwordPlaceholder'))
    return
  }
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
    confirmText: t('settings.rotateApiKeyBtn'),
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
  <!-- Profile Summary -->
  <MdCard v-if="authStore.user" as="section" variant="tonal" padding="lg">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div class="flex min-w-0 items-center gap-3.5">
        <div
          class="text-title-large flex h-12 w-12 shrink-0 items-center justify-center rounded-corner-full bg-primary-container text-on-primary-container"
          aria-hidden="true"
        >
          {{ avatarInitial }}
        </div>
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h2 class="text-title-medium truncate text-on-surface">
              {{ displayLabel }}
            </h2>
            <span
              class="text-label-small inline-flex items-center gap-1 rounded-corner-full px-2.5 py-0.5"
              :class="
                authStore.isAdmin
                  ? 'bg-primary-container text-on-primary-container'
                  : 'bg-secondary-container text-on-secondary-container'
              "
            >
              <LuShieldCheck v-if="authStore.isAdmin" class="h-3 w-3" />
              <LuUser v-else class="h-3 w-3" />
              <span>{{ authStore.isAdmin ? t('auth.roleAdmin') : t('auth.roleMember') }}</span>
            </span>
          </div>
          <p class="text-body-small mt-0.5 truncate font-mono text-on-surface-variant">
            @{{ authStore.user.username }}
          </p>
        </div>
      </div>

      <span
        class="text-label-medium rounded-corner-full px-3 py-1"
        :class="
          authStore.user.is_active
            ? 'bg-success-container text-on-success-container'
            : 'bg-surface-container-high text-on-surface-variant'
        "
      >
        {{ authStore.user.is_active ? t('auth.activeAccount') : t('auth.disabledAccount') }}
      </span>
    </div>
  </MdCard>

  <!-- API Key -->
  <MdCard as="section" variant="tonal" padding="lg" class="space-y-4">
    <div>
      <h2 class="text-title-medium text-on-surface">
        {{ t('settings.apiKeyTitle') }}
      </h2>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{ t('settings.apiKeyDesc') }}
      </p>
    </div>

    <div class="flex flex-col gap-2.5 sm:flex-row sm:items-center">
      <div class="min-w-0 flex-1">
        <MdTextField
          :model-value="authStore.user?.api_key || ''"
          :type="showApiKey ? 'text' : 'password'"
          readonly
          size="sm"
          autocomplete="off"
          :aria-label="t('settings.apiKeyTitle')"
        >
          <template #trailing>
            <MdIconButton
              size="sm"
              :icon="showApiKey ? LuEyeOff : LuEye"
              :selected="showApiKey"
              :aria-label="showApiKey ? t('settings.hideApiKey') : t('settings.showApiKey')"
              @click="showApiKey = !showApiKey"
            />
          </template>
        </MdTextField>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <MdButton
          variant="tonal"
          :icon="copiedKey === 'apiKey' ? LuCheck : LuCopy"
          @click="copyText('apiKey', authStore.user?.api_key || '')"
        >
          {{ copiedKey === 'apiKey' ? t('common.copied') : t('common.copy') }}
        </MdButton>

        <MdButton
          variant="outlined"
          tone="error"
          :icon="LuRotateCw"
          :loading="rotatingKey"
          @click="handleRotateApiKey"
        >
          {{ t('settings.rotateApiKeyBtn') }}
        </MdButton>
      </div>
    </div>
  </MdCard>

  <!-- Change Password -->
  <MdCard as="section" variant="tonal" padding="lg" class="space-y-4">
    <div>
      <h2 class="text-title-medium text-on-surface">
        {{ t('settings.changePasswordTitle') }}
      </h2>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{ t('settings.changePasswordDesc') }}
      </p>
    </div>

    <div
      v-if="pwdSuccess"
      class="text-body-small rounded-corner-md bg-success-container px-3.5 py-2.5 text-on-success-container"
    >
      {{ pwdSuccess }}
    </div>

    <form class="max-w-md space-y-4" @submit.prevent="submitPasswordChange">
      <MdTextField
        v-model="currentPwd"
        type="password"
        required
        autocomplete="current-password"
        :label="t('auth.currentPassword')"
        :disabled="savingPwd"
      />

      <MdTextField
        v-model="nextPwd"
        type="password"
        required
        autocomplete="new-password"
        :label="t('auth.newPassword')"
        :placeholder="t('auth.passwordPlaceholder')"
        :disabled="savingPwd"
      />

      <MdButton type="submit" variant="filled" :loading="savingPwd">
        {{ t('common.save') }}
      </MdButton>
    </form>
  </MdCard>
</template>
