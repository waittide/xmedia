import { onScopeDispose, ref, toValue, watch, type MaybeRefOrGetter, type Ref } from 'vue'

/**
 * Delays showing a loading indicator (skeleton / spinner) until `loading` has stayed
 * true for at least `delayMs`, eliminating brief flashes during fast route switches
 * and local/LAN API responses.
 */
export function useDelayedLoading(
  loading: MaybeRefOrGetter<boolean>,
  delayMs = 180,
): Readonly<Ref<boolean>> {
  const visible = ref(false)
  let timer: ReturnType<typeof setTimeout> | null = null

  function clearTimer(): void {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  watch(
    () => Boolean(toValue(loading)),
    (isLoading) => {
      clearTimer()
      if (!isLoading) {
        visible.value = false
        return
      }
      if (delayMs <= 0) {
        visible.value = true
        return
      }
      timer = setTimeout(() => {
        timer = null
        visible.value = true
      }, delayMs)
    },
    { immediate: true },
  )

  onScopeDispose(clearTimer)

  return visible
}
