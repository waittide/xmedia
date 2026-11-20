<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuTrash2, LuUserPlus } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import type { UserDto, UserRole } from '@/types/api'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'

const emit = defineEmits<{
  error: [message: string]
}>()

const { t, locale } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

const isZh = computed(() => String(locale.value).toLowerCase().startsWith('zh'))
const users = ref<UserDto[]>([])
const newUsername = ref('')
const newDisplayName = ref('')
const newPassword = ref('')
const newRole = ref<UserRole>('member')
const creatingUser = ref(false)
const resetPwdMap = ref<Record<string, string>>({})

const roleOptions = computed(() => [
  { value: 'member', label: t('auth.roleMember') },
  { value: 'admin', label: t('auth.roleAdmin') },
])

onMounted(async () => {
  try {
    users.value = await api.listUsers()
  } catch {
    // ignore
  }
})

async function handleCreateUser(): Promise<void> {
  emit('error', '')
  creatingUser.value = true
  try {
    await api.createUser({
      username: newUsername.value.trim(),
      display_name: newDisplayName.value.trim() || undefined,
      password: newPassword.value,
      role: newRole.value,
    })
    newUsername.value = ''
    newDisplayName.value = ''
    newPassword.value = ''
    newRole.value = 'member'
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    creatingUser.value = false
  }
}

async function handleRoleChange(u: UserDto, role: string): Promise<void> {
  emit('error', '')
  try {
    await api.updateUser(u.id, { role: role as UserRole })
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleActiveToggle(u: UserDto, nextActive: boolean): Promise<void> {
  emit('error', '')
  try {
    await api.updateUser(u.id, { is_active: nextActive })
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleResetUserPassword(u: UserDto): Promise<void> {
  const pwd = resetPwdMap.value[u.id]?.trim()
  if (!pwd) return
  const confirmed = await dialogStore.confirm({
    title: t('admin.resetPasswordBtn'),
    message: isZh.value
      ? `确定重置用户「${u.username}」的登录密码？`
      : `Reset password for user "${u.username}"?`,
    confirmText: t('admin.resetPasswordBtn'),
    danger: true,
  })
  if (!confirmed) return
  emit('error', '')
  try {
    await api.updateUser(u.id, { password: pwd })
    resetPwdMap.value[u.id] = ''
    snackbarStore.show(
      isZh.value ? `已重置用户「${u.username}」的密码` : `Password reset for "${u.username}"`,
    )
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleDeleteUser(u: UserDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('admin.deleteUser'),
    message: t('admin.confirmDeleteUser', { username: u.username }),
    danger: true,
  })
  if (!confirmed) return
  emit('error', '')
  try {
    await api.deleteUser(u.id)
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}
</script>

<template>
  <section class="md-card-outlined p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('admin.createUserTitle') }}
    </h2>
    <p class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]">
      {{ t('admin.createUserDesc') }}
    </p>

    <form class="mt-4 space-y-4" @submit.prevent="handleCreateUser">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('auth.username') }}
          </label>
          <input
            v-model="newUsername"
            type="text"
            required
            minlength="2"
            :placeholder="t('auth.usernamePlaceholder')"
            class="md-input h-10 w-full text-xs"
          />
        </div>

        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('auth.displayName') }}
          </label>
          <input
            v-model="newDisplayName"
            type="text"
            :placeholder="t('auth.displayNamePlaceholder')"
            class="md-input h-10 w-full text-xs"
          />
        </div>

        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('auth.password') }}
          </label>
          <input
            v-model="newPassword"
            type="password"
            required
            minlength="6"
            :placeholder="t('auth.passwordPlaceholder')"
            class="md-input h-10 w-full text-xs"
          />
        </div>

        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('admin.userRoleLabel') }}
          </label>
          <AppSelect
            :model-value="newRole"
            :options="roleOptions"
            @update:model-value="newRole = $event as UserRole"
          />
        </div>
      </div>

      <button
        type="submit"
        :disabled="creatingUser"
        class="md-btn md-btn-filled"
      >
        <LuUserPlus class="h-4 w-4" />
        <span>{{ t('admin.createUserBtn') }}</span>
      </button>
    </form>
  </section>

  <section class="md-card-outlined space-y-3 p-6">
    <div class="flex items-center justify-between gap-3">
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('admin.usersListTitle') }}
      </h2>
      <RefreshButton
        :action="async () => { users = await api.listUsers() }"
      />
    </div>

    <div
      v-for="u in users"
      :key="u.id"
      class="md-card-filled flex flex-col justify-between gap-3 p-4 lg:flex-row lg:items-center"
    >
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <span class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ u.display_name || u.username }}
          </span>
          <span class="font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">
            @{{ u.username }}
          </span>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-3">
        <div class="w-36">
          <AppSelect
            :model-value="u.role"
            :options="roleOptions"
            :disabled="u.id === authStore.user?.id"
            variant="compact"
            @update:model-value="handleRoleChange(u, $event)"
          />
        </div>

        <AppSwitch
          :model-value="u.is_active"
          :disabled="u.id === authStore.user?.id"
          :label="u.is_active ? t('auth.activeAccount') : t('auth.disabledAccount')"
          @update:model-value="handleActiveToggle(u, $event)"
        />

        <form
          class="flex items-center gap-1.5"
          @submit.prevent="handleResetUserPassword(u)"
        >
          <input
            v-model="resetPwdMap[u.id]"
            type="password"
            minlength="6"
            :placeholder="t('admin.resetPasswordPlaceholder')"
            class="md-input h-8 w-36 !px-2.5 text-xs"
          />
          <button
            type="submit"
            :disabled="!resetPwdMap[u.id]?.trim()"
            class="md-btn md-btn-sm md-btn-outlined"
          >
            {{ t('admin.resetPasswordBtn') }}
          </button>
        </form>

        <button
          v-if="u.id !== authStore.user?.id"
          type="button"
          class="md-icon-btn md-icon-btn-sm hover:!text-[var(--md-sys-color-error)]"
          :title="t('admin.deleteUser')"
          :aria-label="t('admin.deleteUser')"
          @click="handleDeleteUser(u)"
        >
          <LuTrash2 class="h-4 w-4" />
        </button>
      </div>
    </div>
  </section>
</template>
