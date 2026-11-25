<script setup lang="ts" generic="S extends OnlineSourceSummaryDto">
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCheckCircle2,
  LuKeyRound,
  LuLoader2,
} from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  extractApiError,
  isSafeHttpUrl,
  type LoginActionResponseDto,
  type LoginUiItem,
  type OnlineSourceSummaryDto,
  type UpdateSourcePatch,
} from './types'

const props = defineProps<{
  open: boolean
  domainLabel: string
  source: S | null
  getSourceVariable: (id: string) => Promise<{ variable: string }>
  runLoginAction: (
    id: string,
    loginInfo: Record<string, string>,
    action?: string,
  ) => Promise<LoginActionResponseDto<S>>
  updateSource: (id: string, patch: UpdateSourcePatch) => Promise<S>
}>()

const emit = defineEmits<{
  close: []
  updated: [source: S]
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

const formValues = reactive<Record<string, string>>({})
const runningActionKey = ref<string | null>(null)
const showVarEditor = ref(false)
const rawVariableDraft = ref('')
const loadingVar = ref(false)
const varLoaded = ref(false)
const varLoadError = ref<string | null>(null)
const savingVar = ref(false)

let actionSeq = 0
let varLoadSeq = 0

const hasCustomActionButtons = computed(() =>
  Boolean(props.source?.login_ui.some((item) => item.type === 'button' && item.action?.trim())),
)

const hasInputFields = computed(() =>
  Boolean(
    props.source?.login_ui.some((item) => item.type === 'text' || item.type === 'password'),
  ),
)

const showFallbackLoginSubmit = computed(
  () =>
    Boolean(props.source && (props.source.has_login_url || hasInputFields.value)) &&
    !hasCustomActionButtons.value,
)

function resetEditorState(): void {
  actionSeq += 1
  varLoadSeq += 1
  for (const k of Object.keys(formValues)) {
    delete formValues[k]
  }
  runningActionKey.value = null
  showVarEditor.value = false
  rawVariableDraft.value = ''
  loadingVar.value = false
  varLoaded.value = false
  varLoadError.value = null
  savingVar.value = false
}

async function loadFullVariable(sourceId: string): Promise<void> {
  const seq = ++varLoadSeq
  loadingVar.value = true
  varLoadError.value = null
  try {
    const res = await props.getSourceVariable(sourceId)
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    rawVariableDraft.value = res.variable ?? ''
    varLoaded.value = true
  } catch (err) {
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    varLoadError.value = extractApiError(err)
    varLoaded.value = false
  } finally {
    if (seq === varLoadSeq) {
      loadingVar.value = false
    }
  }
}

watch(
  () => [props.open, props.source?.id] as const,
  ([isOpen, srcId], prev) => {
    const src = props.source
    if (!isOpen || !src || !srcId) {
      resetEditorState()
      return
    }
    const wasOpen = prev?.[0] ?? false
    const prevId = prev?.[1]
    if (wasOpen && prevId === srcId) return

    resetEditorState()

    for (const k of Object.keys(formValues)) {
      delete formValues[k]
    }
    for (const item of src.login_ui) {
      if (item.type === 'text' || item.type === 'password') {
        formValues[item.name] = src.login_info[item.name] ?? ''
      }
    }
  },
  { immediate: true },
)

function fieldInputId(idx: number): string {
  const sid = props.source?.id ?? 'src'
  return `online-source-login-${sid}-${idx}`
}

function actionKeyFor(item: LoginUiItem, idx: number): string {
  return `${idx}:${item.name}`
}

function toggleVarEditor(): void {
  showVarEditor.value = !showVarEditor.value
  if (showVarEditor.value && props.source && !varLoaded.value && !loadingVar.value) {
    void loadFullVariable(props.source.id)
  }
}

function buttonSpanClass(item: LoginUiItem): string {
  const basis = item.style?.layout_flex_basis_percent ?? 0.5
  if (basis >= 0.85) return 'col-span-4'
  if (basis >= 0.4) return 'col-span-2'
  return 'col-span-1'
}

function isDividerButton(item: LoginUiItem): boolean {
  return item.type === 'button' && (!item.action || !item.action.trim())
}

function openExternalUrlSafely(url: string): void {
  if (!isSafeHttpUrl(url)) {
    snackbar.show({ message: t('onlineSources.login.blockedExternalUrl'), tone: 'error' })
    return
  }
  window.open(url.trim(), '_blank', 'noopener,noreferrer')
}

async function executeLoginCall(
  actionLabel: string,
  actionKey: string,
  action?: string,
): Promise<void> {
  const src = props.source
  if (!src || runningActionKey.value !== null) return
  const sourceId = src.id
  const seq = ++actionSeq
  runningActionKey.value = actionKey
  try {
    const resp = await props.runLoginAction(sourceId, { ...formValues }, action)
    if (seq !== actionSeq || !props.open || props.source?.id !== sourceId) return

    varLoaded.value = false
    if (showVarEditor.value) {
      void loadFullVariable(sourceId)
    }
    emit('updated', resp.source)

    const toasts = resp.diagnostics.toasts.map((v) => v.trim()).filter(Boolean)
    const rawLinks = resp.diagnostics.browser_urls
    const safeLink = rawLinks.find((link) => isSafeHttpUrl(link.url))
    const hasBlockedLink = rawLinks.length > 0 && !safeLink

    if (toasts.length > 0) {
      const finalMsg = toasts[toasts.length - 1]!
      snackbar.show({
        message: finalMsg,
        actionLabel: safeLink ? safeLink.title || t('onlineSources.login.openLink') : undefined,
        onAction: safeLink ? () => openExternalUrlSafely(safeLink.url) : undefined,
      })
    } else if (safeLink) {
      snackbar.show({
        message: t('onlineSources.login.openExternalPage', { name: actionLabel }),
        actionLabel: safeLink.title || t('onlineSources.login.openLink'),
        onAction: () => openExternalUrlSafely(safeLink.url),
      })
    } else if (hasBlockedLink) {
      snackbar.show({
        message: t('onlineSources.login.blockedRedirectUrl', { name: actionLabel }),
        tone: 'error',
      })
    } else if (resp.result) {
      snackbar.show({ message: `${actionLabel}: ${resp.result}` })
    } else {
      snackbar.show({
        message: t('onlineSources.login.executedAction', { name: actionLabel }),
      })
    }
  } catch (err) {
    if (seq !== actionSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === actionSeq) {
      runningActionKey.value = null
    }
  }
}

async function triggerAction(item: LoginUiItem, idx: number): Promise<void> {
  const act = item.action?.trim()
  if (!act) return
  await executeLoginCall(item.name, actionKeyFor(item, idx), act)
}

async function triggerFallbackLogin(): Promise<void> {
  await executeLoginCall(t('onlineSources.login.submitLoginBtn'), '__default_login__')
}

async function saveRawVariable(): Promise<void> {
  const src = props.source
  if (!src || savingVar.value || loadingVar.value || !varLoaded.value || varLoadError.value) return
  const sourceId = src.id
  const seq = varLoadSeq
  savingVar.value = true
  try {
    const updated = await props.updateSource(sourceId, {
      variable: rawVariableDraft.value,
    })
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    emit('updated', updated)
    showVarEditor.value = false
    snackbar.show({ message: t('onlineSources.login.variableSaved') })
  } catch (err) {
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === varLoadSeq) {
      savingVar.value = false
    }
  }
}
</script>

<template>
  <MdDialog
    :open="open && source !== null"
    max-width="max-w-2xl"
    @close="emit('close')"
  >
    <template #header>
      <div v-if="source" class="flex min-w-0 items-start gap-3.5">
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
        >
          <LuKeyRound class="h-5 w-5" />
        </div>
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="text-title-medium truncate text-on-surface">
              {{ t('onlineSources.login.title', { domain: domainLabel, name: source.name }) }}
            </h3>
            <span
              v-if="source.cookie_hosts.length > 0"
              class="text-label-small inline-flex shrink-0 items-center gap-1 rounded-corner-full bg-success-container px-2 py-0.5 text-on-success-container"
            >
              <LuCheckCircle2 class="h-3 w-3" />
              <span>{{ t('onlineSources.login.cookieStored', { count: source.cookie_hosts.length }) }}</span>
            </span>
          </div>
          <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
            <template v-if="source.cookie_hosts.length > 0">
              {{
                t('onlineSources.login.cookieHostsPrefix', {
                  hosts: source.cookie_hosts.slice(0, 3).join(', '),
                })
              }}{{ source.cookie_hosts.length > 3 ? t('onlineSources.login.cookieHostsMore') : '' }}
            </template>
            <template v-else>
              {{ t('onlineSources.login.subtitleEmpty') }}
            </template>
          </p>
        </div>
      </div>
    </template>

    <div v-if="source" class="flex flex-col gap-4 py-1">
      <!-- Dynamic loginUi Controls -->
      <div
        v-if="source.login_ui.length > 0"
        class="grid grid-cols-4 gap-2.5"
      >
        <template v-for="(item, idx) in source.login_ui" :key="`ui-${idx}-${item.name}`">
          <!-- Text / Password Input -->
          <div
            v-if="item.type === 'text' || item.type === 'password'"
            class="col-span-4"
          >
            <label
              :for="fieldInputId(idx)"
              class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.name }}
            </label>
            <input
              :id="fieldInputId(idx)"
              v-model="formValues[item.name]"
              :type="item.type === 'password' ? 'password' : 'text'"
              :autocomplete="item.type === 'password' ? 'off' : undefined"
              class="md-input h-9 w-full text-xs"
              :placeholder="t('onlineSources.login.inputPlaceholder', { name: item.name })"
            />
          </div>

          <!-- Section Divider Label -->
          <div
            v-else-if="isDividerButton(item)"
            class="col-span-4 pt-2 text-center text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ item.name }}
          </div>

          <!-- Action Button -->
          <button
            v-else-if="item.type === 'button'"
            type="button"
            :disabled="runningActionKey !== null"
            class="md-btn md-btn-tonal md-state w-full justify-center"
            :class="buttonSpanClass(item)"
            @click="triggerAction(item, idx)"
          >
            <LuLoader2
              v-if="runningActionKey === actionKeyFor(item, idx)"
              class="h-4 w-4 animate-spin"
            />
            <span class="truncate">{{ item.name }}</span>
          </button>
        </template>

        <!-- Fallback submit button when source has loginUrl / inputs without explicit button item -->
        <div v-if="showFallbackLoginSubmit" class="col-span-4 pt-1">
          <button
            type="button"
            :disabled="runningActionKey !== null"
            class="md-btn md-btn-filled md-state w-full justify-center"
            @click="triggerFallbackLogin"
          >
            <LuLoader2
              v-if="runningActionKey === '__default_login__'"
              class="h-4 w-4 animate-spin"
            />
            <span>
              {{
                runningActionKey === '__default_login__'
                  ? t('onlineSources.login.submittingBtn')
                  : t('onlineSources.login.submitLoginBtn')
              }}
            </span>
          </button>
        </div>
      </div>

      <div
        v-else
        class="text-body-small rounded-corner-lg bg-surface-container p-4 text-center text-on-surface-variant"
      >
        <p>{{ t('onlineSources.login.noLoginUi', { domain: domainLabel }) }}</p>
        <button
          v-if="showFallbackLoginSubmit"
          type="button"
          :disabled="runningActionKey !== null"
          class="md-btn md-btn-filled md-state mt-3"
          @click="triggerFallbackLogin"
        >
          <LuLoader2
            v-if="runningActionKey === '__default_login__'"
            class="h-4 w-4 animate-spin"
          />
          <span>
            {{
              runningActionKey === '__default_login__'
                ? t('onlineSources.login.submittingBtn')
                : t('onlineSources.login.submitLoginBtn')
            }}
          </span>
        </button>
      </div>

      <!-- Advanced Raw Source Variable Editor -->
      <div class="rounded-corner-md bg-surface-container p-3">
        <button
          type="button"
          class="text-label-small cursor-pointer text-primary hover:underline"
          @click="toggleVarEditor"
        >
          {{
            showVarEditor
              ? t('onlineSources.login.hideVariable')
              : t('onlineSources.login.showVariable')
          }}
        </button>

        <div v-if="showVarEditor" class="mt-2 space-y-2">
          <div
            v-if="loadingVar"
            class="text-label-small flex items-center gap-2 rounded-corner-md bg-surface-container px-3 py-2 text-on-surface-variant"
          >
            <LuLoader2 class="h-3.5 w-3.5 animate-spin" />
            <span>{{ t('onlineSources.login.loadingVariable') }}</span>
          </div>
          <div
            v-else-if="varLoadError"
            class="text-label-small flex items-center justify-between gap-2 rounded-corner-md bg-error-container px-3 py-2 text-on-error-container"
          >
            <span class="min-w-0 truncate">
              {{ t('onlineSources.login.loadVariableFailed', { error: varLoadError }) }}
            </span>
            <button
              type="button"
              class="shrink-0 font-medium underline"
              @click="source && loadFullVariable(source.id)"
            >
              {{ t('onlineSources.login.retryBtn') }}
            </button>
          </div>
          <textarea
            v-else
            v-model="rawVariableDraft"
            rows="4"
            class="md-input w-full py-2 font-mono text-[11px]"
            :placeholder="t('onlineSources.login.variablePlaceholder')"
          />
          <div class="flex justify-end">
            <button
              type="button"
              :disabled="savingVar || loadingVar || !varLoaded || Boolean(varLoadError)"
              class="md-btn md-btn-filled md-state"
              @click="saveRawVariable"
            >
              {{
                savingVar
                  ? t('onlineSources.login.savingVariableBtn')
                  : t('onlineSources.login.saveVariableBtn')
              }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex w-full items-center justify-end gap-2">
        <button
          type="button"
          class="md-btn md-btn-tonal md-state"
          @click="emit('close')"
        >
          {{ t('onlineSources.login.doneBtn') }}
        </button>
      </div>
    </template>
  </MdDialog>
</template>
