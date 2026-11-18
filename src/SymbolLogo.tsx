import { createContext, memo, useContext, useEffect, useRef, useState, type MouseEvent } from 'react'
import { api } from './api'

export const CompanyLogosEnabled = createContext(false)

const attributionUrl = 'https://elbstream.com/logos'
const visibleLogos = new Map<Element, () => void>()
const failedUntil = new Map<string, number>()
const retryDelay = 5 * 60_000
let observer: IntersectionObserver | undefined

function observeLogo(element: Element, load: () => void) {
  observer ??= new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue
      const loadVisible = visibleLogos.get(entry.target)
      observer?.unobserve(entry.target)
      visibleLogos.delete(entry.target)
      loadVisible?.()
    }
  })
  visibleLogos.set(element, load)
  observer.observe(element)
  return () => {
    observer?.unobserve(element)
    visibleLogos.delete(element)
  }
}

function LogoImage({ symbol }: { symbol: string }) {
  const image = useRef<HTMLImageElement>(null)
  const [status, setStatus] = useState<'waiting' | 'loading' | 'loaded' | 'failed'>('waiting')
  const lookupSymbol = symbol.replace(/\.([A-Z])$/, '-$1')
  const url = `https://api.elbstream.com/logos/symbol/${encodeURIComponent(lookupSymbol)}?format=png&size=96`

  useEffect(() => {
    function load() {
      if (Date.now() < (failedUntil.get(url) ?? 0)) {
        setStatus('failed')
        return
      }
      failedUntil.delete(url)
      setStatus('loading')
    }
    if (typeof IntersectionObserver === 'undefined') {
      load()
      return
    }
    if (image.current) return observeLogo(image.current, load)
  }, [url])

  if (status === 'failed') return null
  return <img
    ref={image}
    className={status === 'loaded' ? 'symbol-logo loaded' : 'symbol-logo'}
    src={status === 'waiting' ? undefined : url}
    alt=""
    decoding="async"
    loading="lazy"
    referrerPolicy="no-referrer"
    onLoad={() => setStatus('loaded')}
    onError={() => {
      failedUntil.set(url, Date.now() + retryDelay)
      setStatus('failed')
    }}
  />
}

export const SymbolLogo = memo(function SymbolLogo({ symbol }: { symbol: string }) {
  const enabled = useContext(CompanyLogosEnabled)
  return <span className={`monogram shade-${symbol.charCodeAt(0) % 5}`} aria-hidden="true">
    {symbol.slice(0, 2)}
    {enabled && <LogoImage key={symbol} symbol={symbol}/>}
  </span>
})

export function LogoAttribution() {
  const enabled = useContext(CompanyLogosEnabled)
  if (!enabled) return null

  async function openLink(event: MouseEvent<HTMLAnchorElement>) {
    if (!api.isDesktop()) return
    event.preventDefault()
    await api.openLogoAttribution()
  }

  return <a className="logo-attribution" href={attributionUrl} target="_blank" rel="noreferrer" onClick={openLink}>Logos provided by Elbstream</a>
}
