/**
 * Single-seed Material Design 3 dynamic color.
 *
 * One seed color produces the full light & dark MD3 color schemes (tonal-spot variant):
 * five tonal palettes (primary / secondary / tertiary / neutral / neutral-variant) plus
 * error / success / warning palettes, sampled at the official MD3 tones.
 *
 * Tone is CIE L* (identical to HCT tone); hue & chroma are expressed in OKLCH and gamut-mapped
 * by reducing chroma, which keeps every role at its exact MD3 tone and therefore inherits the
 * MD3 contrast guarantees (e.g. tone 40 on tone 100, tone 10 on tone 90).
 */

export type ThemeMode = 'light' | 'dark' | 'system'

export interface SeedPreset {
  id: string
  hex: string
}

/** Curated seeds offered in Appearance settings (first one is the app default). */
export const SEED_PRESETS: readonly SeedPreset[] = [
  { id: 'blue', hex: '#0b57d0' },
  { id: 'violet', hex: '#6750a4' },
  { id: 'teal', hex: '#006a6a' },
  { id: 'green', hex: '#386a20' },
  { id: 'amber', hex: '#8b5000' },
  { id: 'red', hex: '#b3261e' },
  { id: 'pink', hex: '#984061' },
] as const

export const DEFAULT_SEED_COLOR = SEED_PRESETS[0]!.hex

// ---------------------------------------------------------------------------
// Hex / sRGB helpers
// ---------------------------------------------------------------------------

export function parseHexRgb(hex: string): [number, number, number] {
  const clean = hex.trim().replace(/^#/, '')
  if (/^[0-9a-fA-F]{3}$/.test(clean)) {
    const r = Number.parseInt(clean[0]! + clean[0]!, 16)
    const g = Number.parseInt(clean[1]! + clean[1]!, 16)
    const b = Number.parseInt(clean[2]! + clean[2]!, 16)
    return [r, g, b]
  }
  if (/^[0-9a-fA-F]{6}$/.test(clean)) {
    const num = Number.parseInt(clean, 16)
    return [(num >> 16) & 255, (num >> 8) & 255, num & 255]
  }
  return [128, 128, 128]
}

export function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  return (
    '#' +
    [clamp(r), clamp(g), clamp(b)]
      .map((x) => x.toString(16).padStart(2, '0'))
      .join('')
  )
}

/** Normalizes `#rgb` / `#rrggbb` (with or without `#`) to lowercase `#rrggbb`, else `fallback`. */
export function normalizeHex(input: string, fallback: string): string {
  const trimmed = input.trim().replace(/^#/, '')
  if (/^[0-9a-fA-F]{6}$/.test(trimmed)) {
    return `#${trimmed.toLowerCase()}`
  }
  if (/^[0-9a-fA-F]{3}$/.test(trimmed)) {
    const expanded = trimmed
      .toLowerCase()
      .split('')
      .map((c) => c + c)
      .join('')
    return `#${expanded}`
  }
  return fallback
}

function srgbChannelToLinear(channel: number): number {
  const s = channel / 255
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
}

function linearToSrgbChannel(linear: number): number {
  const clamped = Math.max(0, Math.min(1, linear))
  const s = clamped <= 0.0031308 ? 12.92 * clamped : 1.055 * clamped ** (1 / 2.4) - 0.055
  return Math.round(s * 255)
}

/** WCAG 2.1 relative luminance in [0, 1]. */
export function relativeLuminance(hex: string): number {
  const [r, g, b] = parseHexRgb(hex)
  return (
    0.2126 * srgbChannelToLinear(r) +
    0.7152 * srgbChannelToLinear(g) +
    0.0722 * srgbChannelToLinear(b)
  )
}

/** CIE 1976 L* perceptual lightness (MD3 / HCT tone) in [0, 100]. */
export function hexToTone(hex: string): number {
  const y = relativeLuminance(hex)
  const ft = y > 216 / 24389 ? Math.cbrt(y) : ((24389 / 27) * y) / 116 + 16 / 116
  return Math.max(0, Math.min(100, 116 * ft - 16))
}

function toneToLuminance(tone: number): number {
  const t = Math.max(0, Math.min(100, tone))
  const ft = (t + 16) / 116
  return t > 8 ? ft ** 3 : (t * 27) / 24389
}

/** WCAG 2.1 contrast ratio in [1, 21]. */
export function contrastRatio(hexA: string, hexB: string): number {
  const l1 = relativeLuminance(hexA)
  const l2 = relativeLuminance(hexB)
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)
}

/** Picks the more legible of a dark / light foreground against `bgHex`. */
export function chooseContrastText(
  bgHex: string,
  darkText = '#1b1b1f',
  lightText = '#ffffff',
): string {
  return contrastRatio(bgHex, darkText) >= contrastRatio(bgHex, lightText) ? darkText : lightText
}

// ---------------------------------------------------------------------------
// OKLCH + tone solver
// ---------------------------------------------------------------------------

/** sRGB hex -> OKLCH (L in [0,1], C >= 0, H in [0,360)). */
export function hexToOklch(hex: string): [number, number, number] {
  const [r8, g8, b8] = parseHexRgb(hex)
  const r = srgbChannelToLinear(r8)
  const g = srgbChannelToLinear(g8)
  const b = srgbChannelToLinear(b8)

  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)

  const okL = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const okA = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const okB = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s

  const c = Math.hypot(okA, okB)
  const h = c < 1e-6 ? 0 : ((((Math.atan2(okB, okA) * 180) / Math.PI) % 360) + 360) % 360
  return [okL, c, h]
}

function oklchToLinearRgb(okL: number, okC: number, okH: number): [number, number, number] {
  const rad = (okH * Math.PI) / 180
  const okA = okC * Math.cos(rad)
  const okB = okC * Math.sin(rad)

  const l_ = okL + 0.3963377774 * okA + 0.2158037573 * okB
  const m_ = okL - 0.1055613458 * okA - 0.0638541728 * okB
  const s_ = okL - 0.0894841775 * okA - 1.291485548 * okB

  const l = l_ * l_ * l_
  const m = m_ * m_ * m_
  const s = s_ * s_ * s_

  return [
    +4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

function isInSrgbGamut(rgb: [number, number, number]): boolean {
  return rgb.every((v) => v >= -1e-4 && v <= 1 + 1e-4)
}

/**
 * sRGB hex at an exact MD3 tone (CIE L*) for an OKLCH hue / target chroma.
 * Chroma is reduced (never the tone) when the color falls outside the sRGB gamut.
 */
export function tonalHex(okH: number, targetChroma: number, tone: number): string {
  const clampedTone = Math.max(0, Math.min(100, tone))
  if (clampedTone <= 0.01) return '#000000'
  if (clampedTone >= 99.99) return '#ffffff'

  const targetY = toneToLuminance(clampedTone)
  const hue = ((okH % 360) + 360) % 360

  const solveForChroma = (c: number): [number, number, number] => {
    let lo = 0
    let hi = 1
    let rgb: [number, number, number] = [targetY, targetY, targetY]
    for (let i = 0; i < 18; i += 1) {
      const mid = (lo + hi) * 0.5
      rgb = oklchToLinearRgb(mid, c, hue)
      const y =
        0.2126 * Math.max(0, Math.min(1, rgb[0])) +
        0.7152 * Math.max(0, Math.min(1, rgb[1])) +
        0.0722 * Math.max(0, Math.min(1, rgb[2]))
      if (y < targetY) lo = mid
      else hi = mid
    }
    return rgb
  }

  let rgb = solveForChroma(targetChroma)
  if (!isInSrgbGamut(rgb) && targetChroma > 0) {
    let cLo = 0
    let cHi = targetChroma
    let best = solveForChroma(0)
    for (let i = 0; i < 14; i += 1) {
      const cMid = (cLo + cHi) * 0.5
      const cand = solveForChroma(cMid)
      if (isInSrgbGamut(cand)) {
        best = cand
        cLo = cMid
      } else {
        cHi = cMid
      }
    }
    rgb = best
  }

  return rgbToHex(
    linearToSrgbChannel(rgb[0]),
    linearToSrgbChannel(rgb[1]),
    linearToSrgbChannel(rgb[2]),
  )
}

// ---------------------------------------------------------------------------
// Tonal palettes & schemes
// ---------------------------------------------------------------------------

type TonalPalette = (tone: number) => string

function palette(hue: number, chroma: number): TonalPalette {
  const cache = new Map<number, string>()
  return (tone: number) => {
    let hex = cache.get(tone)
    if (!hex) {
      hex = tonalHex(hue, chroma, tone)
      cache.set(tone, hex)
    }
    return hex
  }
}

function paletteFromHex(hex: string, chromaCap: number): TonalPalette {
  const [, c, h] = hexToOklch(hex)
  return palette(h, Math.min(c, chromaCap))
}

/** All MD3 color roles written as `--md-sys-color-<role>` CSS custom properties. */
export const MD3_COLOR_ROLES = [
  'primary',
  'on-primary',
  'primary-container',
  'on-primary-container',
  'primary-fixed',
  'primary-fixed-dim',
  'on-primary-fixed',
  'on-primary-fixed-variant',
  'secondary',
  'on-secondary',
  'secondary-container',
  'on-secondary-container',
  'secondary-fixed',
  'secondary-fixed-dim',
  'on-secondary-fixed',
  'on-secondary-fixed-variant',
  'tertiary',
  'on-tertiary',
  'tertiary-container',
  'on-tertiary-container',
  'tertiary-fixed',
  'tertiary-fixed-dim',
  'on-tertiary-fixed',
  'on-tertiary-fixed-variant',
  'error',
  'on-error',
  'error-container',
  'on-error-container',
  'success',
  'on-success',
  'success-container',
  'on-success-container',
  'warning',
  'on-warning',
  'warning-container',
  'on-warning-container',
  'surface',
  'surface-dim',
  'surface-bright',
  'surface-container-lowest',
  'surface-container-low',
  'surface-container',
  'surface-container-high',
  'surface-container-highest',
  'on-surface',
  'on-surface-variant',
  'outline',
  'outline-variant',
  'inverse-surface',
  'inverse-on-surface',
  'inverse-primary',
  'scrim',
  'shadow',
] as const

export type Md3ColorRole = (typeof MD3_COLOR_ROLES)[number]
export type Md3ColorScheme = Record<Md3ColorRole, string>

export interface Md3SchemePair {
  light: Md3ColorScheme
  dark: Md3ColorScheme
}

const ERROR_SEED = '#b3261e'
const SUCCESS_SEED = '#146c2e'
const WARNING_SEED = '#8b5000'

function accentRoles(
  name: 'primary' | 'secondary' | 'tertiary' | 'error' | 'success' | 'warning',
  p: TonalPalette,
  dark: boolean,
): Partial<Md3ColorScheme> {
  return {
    [name]: p(dark ? 80 : 40),
    [`on-${name}`]: p(dark ? 20 : 100),
    [`${name}-container`]: p(dark ? 30 : 90),
    [`on-${name}-container`]: p(dark ? 90 : 10),
  } as Partial<Md3ColorScheme>
}

function fixedRoles(
  name: 'primary' | 'secondary' | 'tertiary',
  p: TonalPalette,
): Partial<Md3ColorScheme> {
  return {
    [`${name}-fixed`]: p(90),
    [`${name}-fixed-dim`]: p(80),
    [`on-${name}-fixed`]: p(10),
    [`on-${name}-fixed-variant`]: p(30),
  } as Partial<Md3ColorScheme>
}

/**
 * Generates light & dark MD3 schemes from one seed (tonal-spot variant).
 * Achromatic seeds yield a monochrome scheme.
 */
export function generateMd3Schemes(seedHex: string): Md3SchemePair {
  const seed = normalizeHex(seedHex, DEFAULT_SEED_COLOR)
  const [, seedC, seedH] = hexToOklch(seed)
  const achromatic = seedC < 0.02

  // OKLCH chroma budgets roughly matching HCT tonal-spot chroma (36 / 16 / 24 / 6 / 8).
  const primaryC = achromatic ? seedC : Math.min(Math.max(seedC, 0.09), 0.13)
  const secondaryC = achromatic ? seedC : Math.min(primaryC / 3.5, 0.035)
  const tertiaryC = achromatic ? seedC : Math.min(primaryC * 0.65, 0.08)
  const neutralC = achromatic ? 0 : 0.008
  const neutralVariantC = achromatic ? 0 : 0.018

  const P = palette(seedH, primaryC)
  const S = palette(seedH, secondaryC)
  const T = palette((seedH + 60) % 360, tertiaryC)
  const N = palette(seedH, neutralC)
  const NV = palette(seedH, neutralVariantC)
  const E = paletteFromHex(ERROR_SEED, 0.19)
  const G = paletteFromHex(SUCCESS_SEED, 0.13)
  const W = paletteFromHex(WARNING_SEED, 0.13)

  const build = (dark: boolean): Md3ColorScheme =>
    ({
      ...accentRoles('primary', P, dark),
      ...accentRoles('secondary', S, dark),
      ...accentRoles('tertiary', T, dark),
      ...accentRoles('error', E, dark),
      ...accentRoles('success', G, dark),
      ...accentRoles('warning', W, dark),
      ...fixedRoles('primary', P),
      ...fixedRoles('secondary', S),
      ...fixedRoles('tertiary', T),
      surface: N(dark ? 6 : 98),
      'surface-dim': N(dark ? 6 : 87),
      'surface-bright': N(dark ? 24 : 98),
      'surface-container-lowest': N(dark ? 4 : 100),
      'surface-container-low': N(dark ? 10 : 96),
      'surface-container': N(dark ? 12 : 94),
      'surface-container-high': N(dark ? 17 : 92),
      'surface-container-highest': N(dark ? 22 : 90),
      'on-surface': N(dark ? 90 : 10),
      'on-surface-variant': NV(dark ? 80 : 30),
      outline: NV(dark ? 60 : 50),
      'outline-variant': NV(dark ? 30 : 80),
      'inverse-surface': N(dark ? 90 : 20),
      'inverse-on-surface': N(dark ? 20 : 95),
      'inverse-primary': P(dark ? 40 : 80),
      scrim: '#000000',
      shadow: '#000000',
    }) as Md3ColorScheme

  return { light: build(false), dark: build(true) }
}

// ---------------------------------------------------------------------------
// DOM application
// ---------------------------------------------------------------------------

function schemeDeclarations(scheme: Md3ColorScheme): string {
  return MD3_COLOR_ROLES.map((role) => `--md-sys-color-${role}:${scheme[role]};`).join('')
}

/**
 * CSS text defining the scheme for the document (light by default, dark under
 * `[data-theme="dark"]`) plus `.md-scheme-dark` / `.md-scheme-light` for subtrees that must
 * force a scheme (e.g. the always-dark video player stage). Mirrors the static defaults in
 * `styles/main.css`.
 */
export function buildSchemeCss(pair: Md3SchemePair): string {
  return [
    `:root,.md-scheme-light{${schemeDeclarations(pair.light)}}`,
    `:root[data-theme="dark"],.md-scheme-dark{${schemeDeclarations(pair.dark)}}`,
  ].join('\n')
}

/**
 * Normalizes a persisted preference payload into a canonical lowercase `#rrggbb` seed, or `null`
 * to mean the app default seed. Migrates legacy `accent_mode === 'custom'` + `custom_accent`
 * payloads and smoothly falls back to `null` for removed Catppuccin / third-party presets.
 */
export function resolveSeedColor(raw: {
  seed_color?: string | null
  accent_mode?: string | null
  custom_accent?: string | null
}): string | null {
  if (typeof raw.seed_color === 'string') {
    const normalized = normalizeHex(raw.seed_color, '')
    if (!normalized || normalized === DEFAULT_SEED_COLOR) return null
    return normalized
  }
  if (raw.seed_color === null) return null
  if (raw.accent_mode === 'custom' && typeof raw.custom_accent === 'string') {
    const normalized = normalizeHex(raw.custom_accent, '')
    if (!normalized || normalized === DEFAULT_SEED_COLOR) return null
    return normalized
  }
  return null
}

export function normalizeThemeMode(raw: unknown): ThemeMode {
  return raw === 'light' || raw === 'dark' || raw === 'system' ? raw : 'system'
}

const STYLE_ELEMENT_ID = 'md3-dynamic-scheme'

/**
 * Applies a seed + mode to the document. When `seedHex` is the default seed, the static
 * defaults bundled in `main.css` already match and the dynamic `<style>` is removed.
 */
export function applyMd3Theme(
  seedHex: string | null,
  isDark: boolean,
  precomputed?: Md3SchemePair,
): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.dataset.theme = isDark ? 'dark' : 'light'

  const seed = normalizeHex(seedHex ?? '', DEFAULT_SEED_COLOR)
  const pair = precomputed ?? generateMd3Schemes(seed)
  let style = document.getElementById(STYLE_ELEMENT_ID) as HTMLStyleElement | null
  if (seed === DEFAULT_SEED_COLOR) {
    style?.remove()
  } else {
    if (!style) {
      style = document.createElement('style')
      style.id = STYLE_ELEMENT_ID
      document.head.appendChild(style)
    }
    style.textContent = buildSchemeCss(pair)
  }

  let meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')
  if (!meta) {
    meta = document.createElement('meta')
    meta.name = 'theme-color'
    document.head.appendChild(meta)
  }
  meta.content = (isDark ? pair.dark : pair.light)['surface-container']
}
