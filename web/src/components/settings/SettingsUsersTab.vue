<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuTrash2, LuUserPlus } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import {
  MdButton,
  MdCard,
  MdIconButton,
  MdSelect,
  MdSwitch,
  MdTextField,
  RefreshButton,
} from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import type { UserDto, UserRole } from '@/types/api'

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

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

async function loadUsers(): Promise<void> {
  users.value = await api.listUsers()
}

onMounted(async () => {
  try {
    await loadUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
})

async function handleCreateUser(): Promise<void> {
  emit('error', '')
  const trimmedUsername = newUsername.value.trim()
  if (trimmedUsername.length < 2) {
    emit('error', t('auth.usernamePlaceholder'))
    return
  }
  if (newPassword.value.length < 6) {
    emit('error', t('auth.passwordPlaceholder'))
    return
  }
  creatingUser.value = true
  try {
    await api.createUser({
      username: trimmedUsername,
      display_name: newDisplayName.value.trim() || undefined,
      password: newPassword.value,
      role: newRole.value,
    })
    newUsername.value = ''
    newDisplayName.value = ''
    newPassword.value = ''
    newRole.value = 'member'
    await loadUsers()
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
    await loadUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleActiveToggle(u: UserDto, nextActive: boolean): Promise<void> {
  emit('error', '')
  try {
    await api.updateUser(u.id, { is_active: nextActive })
    await loadUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleResetUserPassword(u: UserDto): Promise<void> {
  const pwd = resetPwdMap.value[u.id] ?? ''
  if (pwd.length < 6) return
  const confirmed = await dialogStore.confirm({
    title: t('admin.resetPasswordBtn'),
    message: t('admin.confirmResetPassword', { username: u.username }),
    confirmText: t('admin.resetPasswordBtn'),
    danger: true,
  })
  if (!confirmed) return
  emit('error', '')
  try {
    await api.updateUser(u.id, { password: pwd })
    resetPwdMap.value[u.id] = ''
    snackbarStore.show(t('admin.passwordResetSuccess', { username: u.username }))
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
    await loadUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}
</script>

<template>
  <!-- Create New User -->
  <MdCard as="section" variant="tonal" padding="lg" class="space-y-4">
    <div>
      <h2 class="text-title-medium text-on-surface">
        {{ t('admin.createUserTitle') }}
      </h2>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{ t('admin.createUserDesc') }}
      </p>
    </div>

    <form class="space-y-4" @submit.prevent="handleCreateUser">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <MdTextField
          v-model="newUsername"
          required
          size="sm"
          :label="t('auth.username')"
          :placeholder="t('auth.usernamePlaceholder')"
          :disabled="creatingUser"
        />

        <MdTextField
          v-model="newDisplayName"
          size="sm"
          :label="t('auth.displayName')"
          :placeholder="t('auth.displayNamePlaceholder')"
          :disabled="creatingUser"
        />

        <MdTextField
          v-model="newPassword"
          type="password"
          required
          size="sm"
          autocomplete="new-password"
          :label="t('auth.password')"
          :placeholder="t('auth.passwordPlaceholder')"
          :disabled="creatingUser"
        />

        <div>
          <label class="text-label-medium mb-1 block text-on-surface-variant">
            {{ t('admin.userRoleLabel') }}
          </label>
          <MdSelect
            size="sm"
            :model-value="newRole"
            :options="roleOptions"
            :disabled="creatingUser"
            @update:model-value="newRole = $event as UserRole"
          />
        </div>
      </div>

      <MdButton type="submit" variant="filled" :icon="LuUserPlus" :loading="creatingUser">
        {{ t('admin.createUserBtn') }}
      </MdButton>
    </form>
  </MdCard>

  <!-- System Users List -->
  <MdCard as="section" variant="tonal" padding="none" class="overflow-hidden">
    <div class="flex items-center justify-between gap-3 px-5 pt-4 pb-2">
      <div class="flex items-center gap-2">
        <h2 class="text-title-medium text-on-surface">
          {{ t('admin.usersListTitle') }}
        </h2>
        <span
          class="text-label-small rounded-corner-full bg-surface-container-high px-2.5 py-0.5 tabular-nums text-on-surface-variant"
        >
          {{ users.length }}
        </span>
      </div>
      <RefreshButton size="sm" :action="loadUsers" />
    </div>

    <ul class="divide-y divide-on-surface/6">
      <li
        v-for="u in users"
        :key="u.id"
        class="flex flex-col justify-between gap-3 px-5 py-4 lg:flex-row lg:items-center"
      >
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-title-small truncate text-on-surface">
              {{ u.display_name || u.username }}
            </span>
            <span class="text-body-small font-mono text-on-surface-variant">
              @{{ u.username }}
            </span>
          </div>
        </div>

        <div class="flex flex-wrap items-center gap-3">
          <div class="w-32">
            <MdSelect
              :model-value="u.role"
              :options="roleOptions"
              :disabled="u.id === authStore.user?.id"
              variant="compact"
              @update:model-value="handleRoleChange(u, $event)"
            />
          </div>

          <MdSwitch
            :model-value="u.is_active"
            :disabled="u.id === authStore.user?.id"
            :label="u.is_active ? t('auth.activeAccount') : t('auth.disabledAccount')"
            @update:model-value="handleActiveToggle(u, $event)"
          />

          <form
            class="flex items-center gap-1.5"
            @submit.prevent="handleResetUserPassword(u)"
          >
            <div class="w-36">
              <MdTextField
                v-model="resetPwdMap[u.id]"
                type="password"
                size="sm"
                autocomplete="new-password"
                :placeholder="t('admin.resetPasswordPlaceholder')"
                :aria-label="t('admin.resetPasswordPlaceholder')"
              />
            </div>
            <MdButton
              type="submit"
              variant="outlined"
              size="sm"
              :disabled="(resetPwdMap[u.id]?.length ?? 0) < 6"
            >
              {{ t('admin.resetPasswordBtn') }}
            </MdButton>
          </form>

          <MdIconButton
            v-if="u.id !== authStore.user?.id"
            size="sm"
            :icon="LuTrash2"
            :tooltip="t('admin.deleteUser')"
            :aria-label="t('admin.deleteUser')"
            class="hover:text-error"
            @click="handleDeleteUser(u)"
          />
        </div>
      </li>
    </ul>
  </MdCard>
</template>
