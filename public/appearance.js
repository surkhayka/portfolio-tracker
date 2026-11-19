document.documentElement.dataset.platform = /Windows/.test(navigator.userAgent) ? 'windows' : 'macos'

// Apply the saved palette before the page stylesheet can paint.
try {
  const preference = localStorage.getItem('appearance')
  if (preference === 'dark' ||
      (preference === 'system' && matchMedia('(prefers-color-scheme: dark)').matches)) {
    document.documentElement.dataset.theme = 'dark'
  }
} catch {
  // The default light palette is available if storage is inaccessible.
}
