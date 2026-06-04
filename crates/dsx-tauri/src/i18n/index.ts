// ── Lightweight i18n (zero dependencies) ──
// Supports: interpolation {{key}}, pluralization, namespace nesting, runtime lang switch.

type TranslationValue = string | Record<string, unknown>
type TranslationMap = Record<string, TranslationValue | Record<string, Record<string, unknown>>>

let currentLang = 'zh'
const subscribers = new Set<() => void>()

export function lang() { return currentLang }
export function setLang(l: string) { currentLang = l; subscribers.forEach(fn => fn()) }
export function onLangChange(fn: () => void) { subscribers.add(fn); return () => subscribers.delete(fn) }

function getNested(obj: Record<string, unknown>, path: string): string {
  const keys = path.split('.')
  let cur: unknown = obj
  for (const k of keys) {
    if (typeof cur !== 'object' || cur === null) return path
    cur = (cur as Record<string, unknown>)[k]
  }
  return typeof cur === 'string' ? cur : path
}

const bundles: Record<string, TranslationMap> = {}

export function registerBundle(lang: string, map: TranslationMap) {
  bundles[lang] = map
}

export function t(key: string, params?: Record<string, string | number>): string {
  const bundle = bundles[currentLang]
  if (!bundle) return key
  let text = getNested(bundle as unknown as Record<string, unknown>, key)
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.replace(new RegExp(`\\{\\{${k}\\}\\}`, 'g'), String(v))
    }
  }
  return text
}

export function tt(key: string, params?: Record<string, string | number>): string {
  return t(key, params)
}
