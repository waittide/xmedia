import { buildUploadedFontFaceCss } from '@/theme/customFonts'
import {
  hydrateXhtmlForBrowser as hydrateXhtmlForBrowserRaw,
  sanitizeEpubCssForPreview,
} from './xhtmlUtils'

export function readRootCssToken(name: string, fallback: string): string {
  if (typeof window === 'undefined') return fallback
  const val = window.getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return val || fallback
}

export interface EpubAssetContext {
  rootId: string
  relPath: string
}

export function buildLivePreviewSrcdoc(params: {
  xhtmlBody: string
  sourceHref: string | null
  epubAssetContext: EpubAssetContext | null
  stylesheets: string[]
  indentParagraphs: boolean
}): string {
  const hydrated = hydrateXhtmlForBrowserRaw(
    params.xhtmlBody,
    params.sourceHref,
    params.epubAssetContext,
  )
  const bg = sanitizeEpubCssForPreview(readRootCssToken('--md-sys-color-surface', 'Canvas'))
  const fg = sanitizeEpubCssForPreview(readRootCssToken('--md-sys-color-on-surface', 'CanvasText'))
  const primary = sanitizeEpubCssForPreview(readRootCssToken('--md-sys-color-primary', 'Highlight'))
  const outline = sanitizeEpubCssForPreview(
    readRootCssToken('--md-sys-color-outline-variant', 'GrayText'),
  )
  const epubCss = sanitizeEpubCssForPreview(params.stylesheets.join('\n'))
  const indentRule = params.indentParagraphs ? 'text-indent: 2em;' : 'text-indent: 0;'
  const originImgSrc =
    typeof window !== 'undefined' && window.location.origin && window.location.origin !== 'null'
      ? `${window.location.origin} `
      : ''
  const customFontCss = buildUploadedFontFaceCss()

  return `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8"/>
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${originImgSrc}data: blob:; font-src ${originImgSrc}data: blob:; style-src 'unsafe-inline';"/>
<style>
  ${customFontCss}
  html, body {
    margin: 0;
    padding: 20px 26px 48px;
    background: ${bg};
    color: ${fg};
    font-family: "Noto Serif CJK SC", "Songti SC", "PingFang SC", Georgia, serif;
    font-size: 16px;
    line-height: 1.85;
    word-break: break-word;
  }
  h1, h2, h3, h4 { line-height: 1.35; margin: 1em 0 0.7em; }
  p { margin: 0 0 0.8em; ${indentRule} text-align: justify; }
  img, svg { max-width: 100%; height: auto; border-radius: 6px; }
  blockquote { margin: 1em 0; padding: 0.4em 1em; border-left: 3px solid ${primary}; opacity: 0.9; }
  hr { border: none; border-top: 1px solid ${outline}; margin: 1.5em 0; }
  ${epubCss}
</style>
</head>
<body>${hydrated}</body>
</html>`
}
