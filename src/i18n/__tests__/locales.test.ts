/**
 * Locale parity guard — translations rot fast when keys live in two JSON
 * files and only one gets touched. This pins:
 *   - every key in `en` exists in `zh` (and vice versa)
 *   - new permission keys touched by this PR are present in both
 */

import { describe, it, expect } from 'vitest'
import en from '../locales/en.json'
import zh from '../locales/zh.json'

function flatten(obj: Record<string, unknown>, prefix = ''): string[] {
  const keys: string[] = []
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      keys.push(...flatten(v as Record<string, unknown>, path))
    } else {
      keys.push(path)
    }
  }
  return keys
}

describe('i18n locale parity', () => {
  it('en and zh have identical key sets', () => {
    const enKeys = new Set(flatten(en as Record<string, unknown>))
    const zhKeys = new Set(flatten(zh as Record<string, unknown>))

    const missingInZh = [...enKeys].filter((k) => !zhKeys.has(k))
    const missingInEn = [...zhKeys].filter((k) => !enKeys.has(k))

    expect(missingInZh, `Keys missing in zh: ${missingInZh.join(', ')}`).toEqual([])
    expect(missingInEn, `Keys missing in en: ${missingInEn.join(', ')}`).toEqual([])
  })

  it.each([
    'capsule.accessibilityRequired',
    'capsule.microphoneDenied',
    'settings.accessibilityPermission',
    'settings.accessibilityGranted',
    'settings.accessibilityRequired',
    'settings.grantPermission',
    'permissions.title',
    'permissions.description',
    'permissions.microphone.title',
    'permissions.microphone.required',
    'permissions.microphone.denied',
    'permissions.microphone.grantedHint',
    'permissions.accessibility.title',
    'permissions.accessibility.required',
    'permissions.accessibility.grantedHint',
    'permissions.accessibility.afterClickHint',
    'permissions.openSettings',
    'permissions.grant',
    'permissions.skip',
  ])('key "%s" is present in both locales with non-empty values', (key) => {
    const enKeys = new Set(flatten(en as Record<string, unknown>))
    const zhKeys = new Set(flatten(zh as Record<string, unknown>))
    expect(enKeys.has(key), `en missing ${key}`).toBe(true)
    expect(zhKeys.has(key), `zh missing ${key}`).toBe(true)

    // Walk the key to ensure the value is a non-empty string.
    const lookup = (obj: Record<string, unknown>, k: string): unknown =>
      k.split('.').reduce((acc, seg) => (acc as Record<string, unknown>)?.[seg], obj as unknown)
    expect(typeof lookup(en as Record<string, unknown>, key)).toBe('string')
    expect((lookup(en as Record<string, unknown>, key) as string).length).toBeGreaterThan(0)
    expect(typeof lookup(zh as Record<string, unknown>, key)).toBe('string')
    expect((lookup(zh as Record<string, unknown>, key) as string).length).toBeGreaterThan(0)
  })
})
