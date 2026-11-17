import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowDownToLine, BriefcaseBusiness, ChevronDown, CircleHelp, FileUp, Layers3, Plus, ReceiptText, RefreshCw, Settings2, X } from 'lucide-react'
import { api, type Action, type Batch, type Portfolio, type Record, type Settings as SettingsModel, type Transfer } from './api'
import { Notice } from './components'
import Holdings from './Holdings'
import { TransactionForm, Transactions } from './Transactions'
import { Imports, Review } from './Imports'
import Settings from './Settings'

const menu = [
  { name: 'Holdings', Icon: Layers3, description: 'Track your positions, cost basis, and gains.' },
  { name: 'Transactions', Icon: ReceiptText, description: 'Manage the trades and events in your personal ledger.' },
  { name: 'Imports', Icon: FileUp, description: 'Add trades from a CSV and manage previous imports.' },
  { name: 'Settings', Icon: Settings2, description: 'Manage pricing, display preferences, and local backups.' },
] as const
type Page = typeof menu[number]['name']
const REFRESH_INTERVAL = 120_000
const RETRY_INTERVAL = 300_000


export default function App() {
  const [page,setPage] = useState<Page>('Holdings')
  const [data,setData] = useState<Portfolio | null>(null)
  const [batches,setBatches] = useState<Batch[]>([])
  const [settings,setSettings] = useState<SettingsModel>({connected:false,timezone:'system',credential_error:null})
  const [loading,setLoading] = useState(true)
  const [error,setError] = useState('')
  const [notice,setNotice] = useState('')
  const [priceStatus,setPriceStatus] = useState('')
  const [refreshing,setRefreshing] = useState(false)
  const [autoRefresh,setAutoRefresh] = useState(() => localStorage.getItem('auto-refresh') !== 'off')
  const [editor,setEditor] = useState<{opening:boolean; row?: Record} | null>(null)
  const [review,setReview] = useState<{action: Action; title: string} | null>(null)
  const [settingsVersion,setSettingsVersion] = useState(0)
  const addMenu = useRef<HTMLDetailsElement>(null)
  const refreshingRef = useRef(false)
  const refreshAfter = useRef(0)
  const loadVersion = useRef(0)
  const updateSettings = useCallback(async () => { setSettings(await api.settings()) }, [])
  function setAutomatic(enabled: boolean) {
    localStorage.setItem('auto-refresh', enabled ? 'on' : 'off')
    refreshAfter.current = 0
    setAutoRefresh(enabled)
  }
  const load = useCallback(async () => {
    const version = ++loadVersion.current
    try {
      const next = await api.portfolio()
      if (version === loadVersion.current) {
        setData(next)
        setError('')
      }
      const history = await api.batches()
      if (version === loadVersion.current) setBatches(history)
    } catch(e) { if (version === loadVersion.current) setError(String(e)) }
    finally { if (version === loadVersion.current) setLoading(false) }
  }, [])
  const refresh = useCallback(async (manual = false) => {
    if (refreshingRef.current || (!manual && Date.now() < refreshAfter.current)) return
    refreshingRef.current = true
    const version = loadVersion.current
    if (manual) setRefreshing(true)
    try {
      const { received, valuation } = await api.refresh(manual)
      if (valuation && version === loadVersion.current) {
        setData(previous => previous && previous.revision === valuation.revision
          ? { ...previous, ...valuation } : previous)
      }
      setPriceStatus(received === 0 ? 'No prices returned · Alpaca SIP' : '')
      refreshAfter.current = Date.now() + REFRESH_INTERVAL
    } catch(e) {
      setPriceStatus(`Cached prices · ${String(e)}`)
      refreshAfter.current = Date.now() + RETRY_INTERVAL
    } finally {
      if (manual) setRefreshing(false)
      refreshingRef.current = false
    }
  }, [])
  useEffect(() => {
    void load()
    void updateSettings().catch(e => setError(String(e)))
  }, [load,updateSettings])
  const hasPositions = data?.holdings.some(holding => Number(holding.quantity) > 0) ?? false
  useEffect(() => {
    if (!settings.connected || !hasPositions || !autoRefresh || page !== 'Holdings' || editor || review) return
    function refreshVisible() {
      if (!document.hidden && document.hasFocus()) void refresh()
    }
    refreshVisible()
    const timer = window.setInterval(refreshVisible, 15_000)
    window.addEventListener('focus', refreshVisible)
    document.addEventListener('visibilitychange', refreshVisible)
    return () => {
      window.clearInterval(timer)
      window.removeEventListener('focus', refreshVisible)
      document.removeEventListener('visibilitychange', refreshVisible)
    }
  }, [settings.connected,hasPositions,autoRefresh,page,editor,review,refresh])
  useEffect(() => {
    function keydown(e: KeyboardEvent) {
      if (!(e.metaKey || e.ctrlKey) || editor || review) return
      if (e.key.toLowerCase() === 'n') {e.preventDefault();setEditor({opening:false})}
      if (e.key.toLowerCase() === 'f') {
        const field = document.querySelector<HTMLInputElement>('[data-search]')
        if (field) {e.preventDefault();field.focus()}
      }
    }
    window.addEventListener('keydown',keydown)
    return () => window.removeEventListener('keydown',keydown)
  }, [editor,review])
  async function transfer(operation: Transfer) {
    setError('')
    setNotice('')
    try {
      if (!(await api.transfer(operation))) return
      if (operation === 'restore') {
        await Promise.all([load(), updateSettings()])
        setSettingsVersion(x => x+1)
      }
      setNotice(operation === 'restore' ? 'Backup restored.' : operation === 'backup' ? 'Backup saved.' : 'File saved.')
    } catch(e) {setError(String(e))}
  }
  const startReview = (action: Action, title: string) => setReview({action,title})
  const add = (opening: boolean) => {
    if (addMenu.current) addMenu.current.open = false
    setEditor({opening})
  }
  const item = menu.find(i => i.name === page)!
  return <div className="app-shell">
    <div className="titlebar" data-tauri-drag-region/>
    <aside className="sidebar">
      <div className="brand"><span className="app-icon"><BriefcaseBusiness size={19}/></span><div><strong>Portfolio</strong><small>Personal tracker</small></div></div>
      <nav aria-label="Main navigation">{menu.map(({name,Icon}) => <button key={name} className={'nav-entry '+(name===page?'selected':'')} aria-current={name===page?'page':undefined} onClick={() => {setPage(name);setNotice('')}}><Icon size={18}/>{name}</button>)}</nav>
      <div className="sidebar-footer"><div className="local"><span className="status-dot ok"/>Local storage</div><button className="help" onClick={() => {setPage('Imports')}}><CircleHelp size={16}/>Import help</button></div>
    </aside>
    <main className="main">
      <div className="topbar"><span><BriefcaseBusiness size={15}/>Portfolio</span><span className="breadcrumb">/</span><strong>{page}</strong><span className="currency badge">USD</span></div>
      <div className="content">
        <header className="page-header">
          <div><h1>{page}</h1><p>{item.description}</p></div>
          <div className="actions page-actions">
            {page === 'Holdings' && <><button className="button secondary" disabled={refreshing} onClick={() => settings.connected ? void refresh(true) : setPage('Settings')}><RefreshCw size={15}/>{refreshing ? 'Refreshing…' : settings.connected ? 'Refresh prices' : 'Connect Alpaca'}</button><button className="button secondary" onClick={() => setPage('Imports')}><ArrowDownToLine size={16}/>Import CSV</button></>}
            {(page === 'Holdings' || page === 'Transactions') && <details ref={addMenu} className="add-menu"><summary className="button primary"><Plus size={17}/>Add<ChevronDown size={14}/></summary>
              <div className="column-menu"><button onClick={() => add(false)}><ReceiptText size={15}/>Record transaction</button><button onClick={() => add(true)}><Layers3 size={15}/>Add existing holding</button></div>
            </details>}
          </div>
        </header>
        {error && <Notice error>{error}{!data && <button className="text-button" onClick={() => void load()}>Retry</button>}</Notice>}
        {notice && <div className="dismissable"><Notice>{notice}</Notice><button className="icon-btn" aria-label="Dismiss message" onClick={() => setNotice('')}><X size={15}/></button></div>}
        {loading && <p role="status" className="muted">Opening your portfolio…</p>}
        {data && page === 'Holdings' && <Holdings data={data} timezone={settings.timezone} priceStatus={settings.connected ? priceStatus : ''} connected={settings.connected} refresh={refreshing} onImport={() => setPage('Imports')} onAdd={() => add(true)} onReload={load} onEdit={row => setEditor({opening:false,row})}/>}
        {data && page === 'Transactions' && <Transactions rows={data.transactions} timezone={settings.timezone} add={() => add(false)} edit={row=>setEditor({opening:false,row})} change={startReview}/>}
        {page === 'Imports' && <Imports batches={batches} timezone={settings.timezone} transfer={transfer} review={startReview}/>}
        {page === 'Settings' && <Settings key={settingsVersion} settings={settings} update={updateSettings} transfer={transfer} autoRefresh={autoRefresh} setAutomatic={setAutomatic}/>}
      </div>
    </main>
    {editor && <TransactionForm initial={editor.row} opening={editor.opening} hidden={!!review} onClose={() => setEditor(null)} onSubmit={action => startReview(action,'Review transaction')}/>}
    {review && <Review initial={review.action} title={review.title} timezone={settings.timezone}
      onClose={() => {setReview(null);setEditor(null)}} onBack={editor ? () => setReview(null) : undefined} saved={async () => {
        await load()
        setNotice(review.action.action === 'undo' ? 'Import batch undone.' : review.action.action === 'import' ? 'Transactions imported.' : 'Changes saved.')
      }}/>}
  </div>
}
