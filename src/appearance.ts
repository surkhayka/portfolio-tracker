import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

export type Appearance = 'light' | 'dark' | 'system'

export function readAppearance(): Appearance {
  const saved = localStorage.getItem('appearance')
  return saved === 'dark' || saved === 'system' ? saved : 'light'
}

export function applyAppearance(preference: Appearance) {
  const system = preference === 'system' ? matchMedia('(prefers-color-scheme: dark)') : null
  function updatePalette() {
    document.documentElement.dataset.theme = system ? (system.matches ? 'dark' : 'light') : preference
  }

  updatePalette()
  if (isTauri()) {
    void getCurrentWindow().setTheme(preference === 'system' ? null : preference)
      .catch(error => console.warn('Could not update the window appearance', error))
  }
  system?.addEventListener('change', updatePalette)
  return () => system?.removeEventListener('change', updatePalette)
}
