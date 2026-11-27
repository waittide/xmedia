import { computed, ref, watch, type Component, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSnackbarStore } from '@/stores/snackbar'
import type { RuleFieldDef } from './RuleFieldsSection.vue'
import {
  extractApiError,
  type OnlineSourceManagerAdapter,
  type OnlineSourceSummaryDto,
  type UpdateSourcePatch,
} from './types'

export interface EditTabItem<T extends string = string> {
  id: T
  label: string
  icon: Component
  activeDot: boolean
}

export function strVal(v: unknown): string {
  if (v === null || v === undefined) return ''
  if (typeof v === 'string') return v
  if (typeof v === 'number' || typeof v === 'boolean') return String(v)
  try {
    return JSON.stringify(v)
  } catch {
    return ''
  }
}

export function boolVal(v: unknown, fallback = true): boolean {
  if (typeof v === 'boolean') return v
  if (typeof v === 'number') return v !== 0
  if (typeof v === 'string') {
    const lower = v.trim().toLowerCase()
    if (lower === 'false' || lower === '0' || lower === '') return false
    return true
  }
  return fallback
}

export function numVal(v: unknown, fallback = 0): number {
  if (typeof v === 'number' && Number.isFinite(v)) return Math.trunc(v)
  if (typeof v === 'string' && v.trim() !== '') {
    const n = Number(v.trim())
    if (Number.isFinite(n)) return Math.trunc(n)
  }
  return fallback
}

export function readRuleMap(
  rawRule: unknown,
  fields: RuleFieldDef[],
): Record<string, string> {
  const out: Record<string, string> = {}
  const srcObj =
    rawRule && typeof rawRule === 'object' && !Array.isArray(rawRule)
      ? (rawRule as Record<string, unknown>)
      : {}
  for (const f of fields) {
    out[f.key] = strVal(srcObj[f.key])
  }
  return out
}

export function hasAnyRule(map: Record<string, string>): boolean {
  return Object.values(map).some((v) => v.trim().length > 0)
}

export function coerceEditedValue(original: unknown, edited: string): unknown {
  if (typeof original === 'boolean') {
    const lower = edited.trim().toLowerCase()
    if (lower === 'true') return true
    if (lower === 'false') return false
  } else if (typeof original === 'number') {
    const n = Number(edited.trim())
    if (edited.trim() !== '' && Number.isFinite(n)) return n
  } else if (original !== null && typeof original === 'object') {
    try {
      const parsed = JSON.parse(edited)
      if (parsed !== null && typeof parsed === 'object') {
        return parsed
      }
    } catch {
      // fall back to string
    }
  }
  return edited
}

export function mergeRuleSection(
  baseRule: unknown,
  editedMap: Record<string, string>,
  initialMap: Record<string, string> | undefined,
  fields: RuleFieldDef[],
): Record<string, unknown> | undefined {
  const baseObj =
    baseRule && typeof baseRule === 'object' && !Array.isArray(baseRule)
      ? (baseRule as Record<string, unknown>)
      : null
  const merged: Record<string, unknown> = baseObj ? { ...baseObj } : {}

  for (const f of fields) {
    const val = editedMap[f.key] ?? ''
    const initVal = initialMap?.[f.key] ?? ''
    if (val === initVal) {
      continue
    }
    if (val.trim().length > 0) {
      merged[f.key] = coerceEditedValue(baseObj?.[f.key], val)
    } else {
      delete merged[f.key]
    }
  }
  return Object.keys(merged).length > 0 ? merged : undefined
}

export function mergeOptionalField(
  rawBase: Record<string, unknown>,
  target: Record<string, unknown>,
  key: string,
  edited: string,
  initial: string | undefined,
  trim = false,
): void {
  if (initial !== undefined && edited === initial) {
    return
  }
  const next = trim ? edited.trim() : edited
  if (next.trim().length > 0) {
    target[key] = coerceEditedValue(rawBase[key], next)
  } else {
    delete target[key]
  }
}

export interface UseOnlineSourceEditOptions<S extends OnlineSourceSummaryDto, T extends string> {
  open: Ref<boolean>
  source: Ref<S | null>
  adapter: OnlineSourceManagerAdapter<S>
  initialTab: T
  jsonTabId: T
  clearFormState: () => void
  hydrateFormFromObject: (obj: Record<string, unknown>, srcSummary?: S) => void
  buildMergedObject: () => Record<string, unknown>
  validateForm: () => string | null
  validateJsonObject: (obj: Record<string, unknown>) => string | null
  getGroupName: () => string
  getEnabled: () => boolean
  getWeight: () => number
  onSaved: (source: S) => void
  onSavedAndDebug: (source: S) => void
  onClose: () => void
}

export function useOnlineSourceEdit<S extends OnlineSourceSummaryDto, T extends string>(
  options: UseOnlineSourceEditOptions<S, T>,
) {
  const { t } = useI18n()
  const snackbar = useSnackbarStore()

  const activeTab = ref<T>(options.initialTab) as Ref<T>
  const loading = ref(false)
  const saving = ref(false)
  const loadError = ref<string | null>(null)
  const rawBaseObj = ref<Record<string, unknown>>({})
  const jsonText = ref('')
  const jsonDirty = ref(false)
  const variableText = ref('')
  const initialVariableText = ref('')
  let loadSeq = 0
  let saveSeq = 0

  const jsonValidation = computed<{ valid: boolean; error: string | null }>(() => {
    if (activeTab.value !== options.jsonTabId && !jsonDirty.value) {
      const formErr = options.validateForm()
      return formErr ? { valid: false, error: formErr } : { valid: true, error: null }
    }
    const txt = jsonText.value.trim()
    if (!txt) {
      return { valid: false, error: t('onlineSources.edit.jsonEmptyError') }
    }
    try {
      const parsed = JSON.parse(txt)
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        return { valid: false, error: t('onlineSources.edit.invalidJsonObject') }
      }
      const err = options.validateJsonObject(parsed as Record<string, unknown>)
      return err ? { valid: false, error: err } : { valid: true, error: null }
    } catch (err) {
      return {
        valid: false,
        error: err instanceof Error ? err.message : t('onlineSources.edit.jsonSyntaxError'),
      }
    }
  })

  function switchTab(next: T): void {
    if (next === activeTab.value) return
    if (activeTab.value === options.jsonTabId && jsonDirty.value) {
      try {
        const parsed = JSON.parse(jsonText.value)
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
          snackbar.show({
            message: t('onlineSources.edit.invalidJsonObject'),
            tone: 'error',
          })
          return
        }
        rawBaseObj.value = { ...(parsed as Record<string, unknown>) }
        options.hydrateFormFromObject(parsed as Record<string, unknown>)
        jsonDirty.value = false
      } catch (err) {
        snackbar.show({
          message: t('onlineSources.edit.jsonSwitchError', {
            error: extractApiError(err),
          }),
          tone: 'error',
        })
        return
      }
    }
    if (next === options.jsonTabId) {
      jsonText.value = JSON.stringify(options.buildMergedObject(), null, 2)
      jsonDirty.value = false
    }
    activeTab.value = next
  }

  async function loadSourceRaw(src: S): Promise<void> {
    const seq = ++loadSeq
    const targetId = src.id
    loading.value = true
    loadError.value = null
    activeTab.value = options.initialTab
    rawBaseObj.value = {}
    jsonText.value = ''
    jsonDirty.value = false
    variableText.value = ''
    initialVariableText.value = ''
    options.clearFormState()
    try {
      const [obj, varRes] = await Promise.all([
        options.adapter.fetchSourceJson(targetId),
        options.adapter.getSourceVariable(targetId),
      ])
      if (seq !== loadSeq || !options.open.value || options.source.value?.id !== targetId) return
      rawBaseObj.value = { ...obj }
      options.hydrateFormFromObject(obj, src)
      const fullVar = varRes.variable ?? ''
      variableText.value = fullVar
      initialVariableText.value = fullVar
      jsonText.value = JSON.stringify(options.buildMergedObject(), null, 2)
    } catch (err) {
      if (seq !== loadSeq || !options.open.value || options.source.value?.id !== targetId) return
      rawBaseObj.value = {}
      options.clearFormState()
      loadError.value = extractApiError(err)
    } finally {
      if (seq === loadSeq) {
        loading.value = false
      }
    }
  }

  watch(
    () => [options.open.value, options.source.value?.id] as const,
    ([isOpen]) => {
      saveSeq += 1
      saving.value = false
      if (isOpen && options.source.value) {
        void loadSourceRaw(options.source.value)
      } else {
        loadSeq += 1
        loading.value = false
        loadError.value = null
        rawBaseObj.value = {}
        jsonText.value = ''
        jsonDirty.value = false
        variableText.value = ''
        initialVariableText.value = ''
        options.clearFormState()
      }
    },
    { immediate: true },
  )

  function formatJson(): void {
    try {
      const parsed = JSON.parse(jsonText.value)
      jsonText.value = JSON.stringify(parsed, null, 2)
    } catch (err) {
      snackbar.show({
        message: t('onlineSources.edit.formatFailed', { error: extractApiError(err) }),
        tone: 'error',
      })
    }
  }

  async function copyJson(): Promise<void> {
    const txt =
      activeTab.value === options.jsonTabId
        ? jsonText.value
        : JSON.stringify(options.buildMergedObject(), null, 2)
    try {
      await navigator.clipboard.writeText(txt)
      snackbar.show({ message: t('onlineSources.edit.jsonCopied') })
    } catch {
      snackbar.show({ message: t('onlineSources.edit.copyFailed'), tone: 'error' })
    }
  }

  async function saveSource(openDebugAfter = false): Promise<void> {
    const src = options.source.value
    if (
      !src ||
      loading.value ||
      saving.value ||
      Boolean(loadError.value) ||
      !jsonValidation.value.valid
    ) {
      return
    }

    const targetId = src.id
    let finalObj: Record<string, unknown>
    if (activeTab.value === options.jsonTabId && jsonDirty.value) {
      try {
        finalObj = JSON.parse(jsonText.value) as Record<string, unknown>
        rawBaseObj.value = { ...finalObj }
        options.hydrateFormFromObject(finalObj)
      } catch (err) {
        snackbar.show({ message: extractApiError(err), tone: 'error' })
        return
      }
    } else {
      finalObj = options.buildMergedObject()
    }

    const seq = ++saveSeq
    saving.value = true
    try {
      const patch: UpdateSourcePatch = {
        enabled: options.getEnabled(),
        group_name: options.getGroupName().trim(),
        weight: options.getWeight(),
        raw_json: JSON.stringify(finalObj),
      }
      if (variableText.value !== initialVariableText.value) {
        patch.variable = variableText.value
      }
      const updated = await options.adapter.updateSource(targetId, patch)
      if (seq !== saveSeq || !options.open.value || options.source.value?.id !== targetId) return
      snackbar.show({
        message: t('onlineSources.edit.savedMsg', { name: updated.name }),
      })
      if (openDebugAfter) {
        options.onSavedAndDebug(updated)
      } else {
        options.onSaved(updated)
        options.onClose()
      }
    } catch (err) {
      if (seq !== saveSeq || !options.open.value || options.source.value?.id !== targetId) return
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      if (seq === saveSeq) {
        saving.value = false
      }
    }
  }

  return {
    activeTab,
    loading,
    saving,
    loadError,
    rawBaseObj,
    jsonText,
    jsonDirty,
    variableText,
    initialVariableText,
    jsonValidation,
    switchTab,
    formatJson,
    copyJson,
    saveSource,
  }
}
