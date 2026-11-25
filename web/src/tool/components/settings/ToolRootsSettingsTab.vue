<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheckCircle2, LuFolderPlus, LuRotateCw, LuSave, LuTrash2 } from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { useDialogStore } from '@/stores/dialog'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import type { ManagedRootDto, PublicSettingsDto } from '../../types'

const props = defineProps<{
  settings: PublicSettingsDto | null
  domainSubTab: MediaKind
}>()

const emit = defineEmits<{
  updated: [settings: PublicSettingsDto]
  error: [message: string | null]
  saved: []
}>()

const { t } = useI18n()
const dialogStore = useDialogStore()

const newRootName = ref('')
const newRootPath = ref('')
const newRootReadOnly = ref(false)
const addingRoot = ref(false)

const inboxPath = ref('')
const inboxTargetRootId = ref('')
const inboxUseHardlink = ref(true)
const inboxDeleteSource = ref(false)
const inboxAutoScrape = ref(false)
const inboxOrganize = ref(true)
const scanningInbox = ref(false)
const inboxNotice = ref<string | null>(null)

const webhookEnabled = ref(false)
const webhookBaseUrl = ref('')
const webhookApiKey = ref('')
const webhookNotifyRename = ref(true)
const webhookNotifyScrape = ref(true)
const webhookNotifyInbox = ref(true)
const testingWebhook = ref(false)
const webhookNotice = ref<string | null>(null)
const saving = ref(false)

const domainLabel = computed(() => t(`tool.domains.${props.domainSubTab}`))

const inboxRootOptions = computed(() => [
  { value: '', label: t('tool.settings.roots.selectRegisteredRoot') },
  ...(props.settings?.roots ?? []).map((r) => ({
    value: r.id,
    label: `${r.name} (${r.path})`,
  })),
])

const filteredRoots = computed<ManagedRootDto[]>(() => {
  const all = props.settings?.roots ?? []
  return all.filter((r) => r.default_kind === props.domainSubTab)
})

watch(
  () => props.settings,
  (s) => {
    if (!s) return
    if (s.inbox) {
      inboxPath.value = s.inbox.inbox_path ?? ''
      inboxTargetRootId.value = s.inbox.target_root_id ?? ''
      inboxUseHardlink.value = s.inbox.use_hardlink
      inboxDeleteSource.value = s.inbox.delete_source
      inboxAutoScrape.value = s.inbox.auto_scrape
      inboxOrganize.value = s.inbox.organize_by_template
    }
    webhookEnabled.value = Boolean(s.xmedia_webhook_enabled)
    webhookBaseUrl.value = s.xmedia_webhook_base_url ?? ''
    webhookNotifyRename.value = s.xmedia_webhook_notify_on_rename ?? true
    webhookNotifyScrape.value = s.xmedia_webhook_notify_on_scrape ?? true
    webhookNotifyInbox.value = s.xmedia_webhook_notify_on_inbox ?? true
  },
  { immediate: true },
)

async function handleAddRoot(): Promise<void> {
  if (!newRootPath.value.trim() || addingRoot.value) return
  addingRoot.value = true
  emit('error', null)
  try {
    await toolApi.addRoot(
      newRootName.value.trim(),
      newRootPath.value.trim(),
      props.domainSubTab,
      newRootReadOnly.value,
    )
    newRootName.value = ''
    newRootPath.value = ''
    newRootReadOnly.value = false
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    addingRoot.value = false
  }
}

async function handleToggleRootReadOnly(id: string, currentReadOnly: boolean): Promise<void> {
  emit('error', null)
  try {
    await toolApi.setRootReadOnly(id, !currentReadOnly)
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleRemoveRoot(id: string): Promise<void> {
  const target = (props.settings?.roots ?? []).find((r) => r.id === id)
  const confirmed = await dialogStore.confirm({
    title: t('tool.settings.roots.removeRootTitle'),
    message: t('tool.settings.roots.removeRootMsg', {
      name: target?.name || target?.path || id,
    }),
    confirmText: t('tool.settings.roots.removeRootConfirm'),
    danger: true,
  })
  if (!confirmed) return
  emit('error', null)
  try {
    await toolApi.removeRoot(id)
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleSaveInbox(): Promise<void> {
  saving.value = true
  emit('error', null)
  inboxNotice.value = null
  try {
    const next = await toolApi.updateSettings({
      inbox: {
        enabled: Boolean(inboxPath.value.trim() && inboxTargetRootId.value),
        inbox_path: inboxPath.value.trim() || null,
        target_root_id: inboxTargetRootId.value || null,
        use_hardlink: inboxUseHardlink.value,
        delete_source: inboxDeleteSource.value,
        auto_scrape: inboxAutoScrape.value,
        organize_by_template: inboxOrganize.value,
      },
    })
    emit('updated', next)
    emit('saved')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    saving.value = false
  }
}

async function handleSaveWebhook(): Promise<void> {
  saving.value = true
  emit('error', null)
  webhookNotice.value = null
  try {
    const next = await toolApi.updateSettings({
      xmedia_webhook_enabled: webhookEnabled.value,
      xmedia_webhook_base_url: webhookBaseUrl.value.trim(),
      xmedia_webhook_api_key: webhookApiKey.value.trim() || undefined,
      xmedia_webhook_notify_on_rename: webhookNotifyRename.value,
      xmedia_webhook_notify_on_scrape: webhookNotifyScrape.value,
      xmedia_webhook_notify_on_inbox: webhookNotifyInbox.value,
    })
    webhookApiKey.value = ''
    emit('updated', next)
    emit('saved')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    saving.value = false
  }
}

async function handleScanInbox(): Promise<void> {
  scanningInbox.value = true
  emit('error', null)
  inboxNotice.value = null
  try {
    const rep = await toolApi.scanInbox()
    inboxNotice.value =
      t('tool.settings.roots.inboxReport', {
        imported: rep.imported_count,
        skipped: rep.skipped_count,
      }) +
      (rep.failed_count > 0
        ? t('tool.settings.roots.inboxFailedPart', { failed: rep.failed_count })
        : '')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    scanningInbox.value = false
  }
}

async function handleTestWebhook(): Promise<void> {
  testingWebhook.value = true
  emit('error', null)
  webhookNotice.value = null
  try {
    const rep = await toolApi.testWebhook()
    webhookNotice.value =
      rep.message +
      (rep.matched_libraries.length > 0
        ? t('tool.settings.roots.matchedLibraries', {
            libs: rep.matched_libraries.join(' / '),
          })
        : '')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    testingWebhook.value = false
  }
}
</script>

<template>
  <section class="md-card-filled p-6">
    <h2 class="text-title-small text-on-surface">
      {{ t('tool.settings.roots.addTitle', { domain: domainLabel }) }}
    </h2>

    <form class="mt-4 space-y-4" @submit.prevent="handleAddRoot">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-12">
        <div class="sm:col-span-4">
          <MdTextField
            v-model="newRootName"
            :label="t('tool.settings.roots.aliasLabel')"
            :placeholder="t('tool.settings.roots.aliasPlaceholder')"
          />
        </div>

        <div class="sm:col-span-8">
          <MdTextField
            v-model="newRootPath"
            required
            :label="t('tool.settings.roots.absPathLabel')"
            :placeholder="t('tool.settings.roots.absPathPlaceholder', { domain: domainSubTab })"
          />
        </div>
      </div>

      <div class="flex justify-end">
        <MdButton
          type="submit"
          variant="filled"
          :icon="LuFolderPlus"
          :loading="addingRoot"
          :disabled="!newRootPath.trim()"
        >
          {{ t('tool.settings.roots.addToDomain', { domain: domainLabel }) }}
        </MdButton>
      </div>
    </form>
  </section>

  <section class="md-card-filled space-y-3 p-6">
    <h2 class="text-title-small text-on-surface">
      {{
        t('tool.settings.roots.registeredTitle', {
          domain: domainLabel,
          count: filteredRoots.length,
        })
      }}
    </h2>

    <div
      v-if="filteredRoots.length === 0"
      class="text-body-small rounded-corner-lg bg-surface py-10 text-center text-on-surface-variant"
    >
      {{ t('tool.settings.roots.emptyDomainRoots', { domain: domainLabel }) }}
    </div>

    <div
      v-for="r in filteredRoots"
      :key="r.id"
      class="flex items-center justify-between gap-3 rounded-corner-lg bg-surface p-4"
    >
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <span class="text-title-small truncate text-on-surface">{{ r.name }}</span>
          <span
            class="text-label-small rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
          >
            {{ domainLabel }}
          </span>
          <span
            v-if="r.read_only"
            class="text-label-small rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
          >
            {{ t('tool.settings.roots.readOnlyBadge') }}
          </span>
        </div>
        <p class="text-body-small mt-1 truncate font-mono text-on-surface-variant">{{ r.path }}</p>
      </div>

      <div class="flex shrink-0 items-center gap-1.5">
        <MdButton
          :variant="r.read_only ? 'tonal' : 'outlined'"
          size="sm"
          :title="t('tool.settings.roots.readOnlyHint')"
          @click="handleToggleRootReadOnly(r.id, Boolean(r.read_only))"
        >
          {{
            r.read_only
              ? t('tool.settings.roots.unsetReadOnly')
              : t('tool.settings.roots.setReadOnly')
          }}
        </MdButton>
        <MdIconButton
          size="sm"
          :icon="LuTrash2"
          :tooltip="t('tool.settings.roots.removeRootHint')"
          :aria-label="t('tool.settings.roots.removeRootHint')"
          @click="handleRemoveRoot(r.id)"
        />
      </div>
    </div>
  </section>

  <!-- Inbox / Watch Folder Card -->
  <section class="md-card-filled space-y-4 p-6">
    <div>
      <h2 class="text-title-small text-on-surface">
        {{ t('tool.settings.roots.inboxTitle') }}
      </h2>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{ t('tool.settings.roots.inboxDesc') }}
      </p>
    </div>
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <MdTextField
        v-model="inboxPath"
        :label="t('tool.settings.roots.inboxPathLabel')"
        placeholder="/data/inbox"
      />
      <div class="flex flex-col gap-1">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('tool.settings.roots.inboxTargetLabel') }}
        </span>
        <MdSelect v-model="inboxTargetRootId" :options="inboxRootOptions" />
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-5">
      <AppCheckbox v-model="inboxUseHardlink" :label="t('tool.settings.roots.optHardlink')" />
      <AppCheckbox v-model="inboxDeleteSource" :label="t('tool.settings.roots.optDeleteSource')" />
      <AppCheckbox v-model="inboxAutoScrape" :label="t('tool.settings.roots.optAutoScrape')" />
      <AppCheckbox v-model="inboxOrganize" :label="t('tool.settings.roots.optOrganize')" />
    </div>
    <div
      v-if="inboxNotice"
      class="text-body-small rounded-corner-lg bg-success-container px-3.5 py-2 text-on-success-container"
    >
      {{ inboxNotice }}
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <MdButton variant="filled" size="sm" :icon="LuSave" :loading="saving" @click="handleSaveInbox">
        {{ t('tool.settings.roots.saveInbox') }}
      </MdButton>
      <MdButton
        variant="outlined"
        size="sm"
        :icon="LuRotateCw"
        :loading="scanningInbox"
        :disabled="!inboxPath.trim() || !inboxTargetRootId"
        @click="handleScanInbox"
      >
        {{
          scanningInbox
            ? t('tool.settings.roots.scanningInbox')
            : t('tool.settings.roots.scanInboxNow')
        }}
      </MdButton>
    </div>
  </section>

  <!-- xmedia Main Server Webhook Card -->
  <section class="md-card-filled space-y-4 p-6">
    <div class="flex items-center justify-between gap-3">
      <div>
        <h2 class="text-title-small text-on-surface">
          {{ t('tool.settings.roots.webhookTitle') }}
        </h2>
        <p class="text-body-small mt-1 text-on-surface-variant">
          {{ t('tool.settings.roots.webhookDesc') }}
        </p>
      </div>
      <MdSwitch v-model="webhookEnabled" :label="t('tool.settings.roots.enableWebhook')" />
    </div>
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <MdTextField
        v-model="webhookBaseUrl"
        :label="t('tool.settings.roots.webhookUrlLabel')"
        placeholder="http://127.0.0.1:8080"
      />
      <MdTextField
        v-model="webhookApiKey"
        type="password"
        :label="
          props.settings?.xmedia_webhook_api_key_masked
            ? t('tool.settings.roots.webhookKeySavedLabel', {
                masked: props.settings.xmedia_webhook_api_key_masked,
              })
            : t('tool.settings.roots.webhookKeyLabel')
        "
        :placeholder="t('tool.settings.roots.webhookKeyPlaceholder')"
      />
    </div>
    <div class="flex flex-wrap items-center gap-5">
      <AppCheckbox v-model="webhookNotifyRename" :label="t('tool.settings.roots.notifyRename')" />
      <AppCheckbox v-model="webhookNotifyScrape" :label="t('tool.settings.roots.notifyScrape')" />
      <AppCheckbox v-model="webhookNotifyInbox" :label="t('tool.settings.roots.notifyInbox')" />
    </div>
    <div
      v-if="webhookNotice"
      class="text-body-small rounded-corner-lg bg-success-container px-3.5 py-2 text-on-success-container"
    >
      {{ webhookNotice }}
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <MdButton
        variant="filled"
        size="sm"
        :icon="LuSave"
        :loading="saving"
        @click="handleSaveWebhook"
      >
        {{ t('tool.settings.roots.saveWebhook') }}
      </MdButton>
      <MdButton
        variant="outlined"
        size="sm"
        :icon="LuCheckCircle2"
        :loading="testingWebhook"
        @click="handleTestWebhook"
      >
        {{
          testingWebhook
            ? t('tool.settings.roots.testingWebhook')
            : t('tool.settings.roots.testWebhook')
        }}
      </MdButton>
    </div>
  </section>
</template>
