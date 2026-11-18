import { useMemo, useState } from 'react'
import { createColumnHelper, flexRender, getCoreRowModel, getSortedRowModel, useReactTable, type SortingState, type VisibilityState } from '@tanstack/react-table'
import { ArrowDown, ArrowDownUp, ArrowUp, ChevronRight, Coins, Layers3, ListFilter, Plus, TrendingUp, Wallet, CircleDollarSign, History, Pencil, X } from 'lucide-react'
import type { FormEvent } from 'react'
import { api, type Holding, type Portfolio, type Record } from './api'
import { age, date, kindName, localInput, money, percent, shares, tone } from './format'
import { Empty, Modal, Notice, PAGE_SIZE, SearchBox, TablePager, validPage } from './components'
import { LogoAttribution, SymbolLogo } from './SymbolLogo'
const column = createColumnHelper<Holding>()
const cmp = (a: string | null, b: string | null) => Number(a ?? 0) - Number(b ?? 0)
const amount = (v?: string | null) => <span className={tone(v)}>{money(v, true)}</span>
const cols = [
  column.accessor('symbol', {header: 'Symbol', cell: info => <span className="symbol"><SymbolLogo symbol={info.getValue()}/><strong>{info.getValue()}</strong>{Number(info.row.original.quantity) === 0 && <span className="badge">Closed</span>}</span>, enableHiding: false}),
  column.accessor('quantity', {header: 'Shares', cell: c => shares(c.getValue()), sortingFn: (a,b) => cmp(a.original.quantity,b.original.quantity)}),
  column.accessor(h => h.quote?.price, {id:'price', sortUndefined:'last', sortDescFirst:true, header:'Price', cell: c => <span title={c.row.original.quote ? date(c.row.original.quote.at) : 'Price unavailable'}>{money(c.getValue())}{c.row.original.quote?.manual && <sup> M</sup>}</span>, sortingFn: (a,b) => cmp(a.original.quote?.price ?? null,b.original.quote?.price ?? null)}),
  column.accessor(h => h.allocation ?? undefined, {id:'allocation', sortUndefined:'last', sortDescFirst:true, header:'Allocation',cell: c => percent(c.getValue()), sortingFn: (a,b) => cmp(a.original.allocation,b.original.allocation)}),
  column.accessor(h => h.average ?? undefined, {id:'average', sortUndefined:'last', sortDescFirst:true, header:'Avg. cost',cell: c => money(c.getValue()), sortingFn: (a,b) => cmp(a.original.average,b.original.average)}),
  column.accessor(h => h.value ?? undefined, {id:'value', sortUndefined:'last', sortDescFirst:true, header:'Market value',cell: c => <strong>{money(c.getValue())}</strong>, sortingFn: (a,b) => cmp(a.original.value,b.original.value)}),
  column.accessor(h => h.unrealized ?? undefined, {id:'unrealized', sortUndefined:'last', sortDescFirst:true, header:'Unrealized gain',cell: c => amount(c.getValue()), sortingFn: (a,b) => cmp(a.original.unrealized,b.original.unrealized)}),
  column.accessor(h => h.unrealized_percent ?? undefined, {id:'unrealized_percent', sortUndefined:'last', sortDescFirst:true, header:'Unrealized %',cell: c => <span className={tone(c.getValue())}>{percent(c.getValue(), true)}</span>, sortingFn: (a,b) => cmp(a.original.unrealized_percent,b.original.unrealized_percent)}),
  column.accessor(h => h.daily ?? undefined, {id:'daily', sortUndefined:'last', sortDescFirst:true, header:'Daily price move',cell: c => <span className={tone(c.getValue())}>{money(c.getValue(), true)}{c.row.original.daily_percent !== null && <small className="subnum">{percent(c.row.original.daily_percent,true)}</small>}</span>, sortingFn: (a,b) => cmp(a.original.daily,b.original.daily)}),
  column.accessor(h => h.total ?? undefined, {id:'total', sortUndefined:'last', sortDescFirst:true, header:'Total gain',cell: c => amount(c.getValue()), sortingFn: (a,b) => cmp(a.original.total,b.original.total)}),
]
const labels: { [key: string]: string } = {quantity:'Shares',price:'Price',allocation:'Allocation',average:'Avg. cost',value:'Market value',unrealized:'Unrealized gain',unrealized_percent:'Unrealized %',daily:'Daily price move',total:'Total gain'}

export default function Holdings({ data, timezone, priceStatus, refresh, connected, onImport, onAdd, onReload, onEdit }: {
  data: Portfolio; timezone: string; priceStatus: string; refresh: boolean; connected: boolean; onImport: () => void
  onAdd: () => void; onReload: () => Promise<void>; onEdit: (record: Record) => void
}) {
  const [query, setQuery] = useState('')
  const [position, setPosition] = useState('open')
  const [gain, setGain] = useState('any')
  const [page, setPage] = useState(0)
  const [selection, setSelection] = useState<string | null>(null)
  const [sorting, setSorting] = useState<SortingState>([{id:'value',desc:true}])
  const [visibility, setVisibility] = useState<VisibilityState>(() => {
    try { return JSON.parse(localStorage.getItem('columns') ?? '{}') } catch { return {} }
  })
  const rows = useMemo(() => {
    const ticker = query.trim().toUpperCase()
    return data.holdings.filter(h => {
      const isOpen = Number(h.quantity) !== 0
      if (position !== 'all' && isOpen !== (position === 'open')) return false
      if (!h.symbol.includes(ticker)) return false
      if (gain === 'any') return true
      if (gain === 'unpriced') return isOpen && h.quote === null
      if (h.total === null) return false
      const total = Number(h.total)
      return gain === 'positive' ? total > 0 : gain === 'negative' ? total < 0 : total === 0
    })
  }, [data.holdings, position, gain, query])
  const pageIndex = validPage(page, rows.length)
  const table = useReactTable({data: rows, columns:cols, state:{sorting,columnVisibility:visibility}, onSortingChange: update => { setSorting(update); setPage(0) }, enableSortingRemoval:false, enableMultiSort:false,
    onColumnVisibilityChange: updater => setVisibility(current => {
      const next = typeof updater === 'function' ? updater(current) : updater
      localStorage.setItem('columns', JSON.stringify(next))
      return next
    }), getCoreRowModel:getCoreRowModel(),getSortedRowModel:getSortedRowModel()})
  const summary = data.summary
  const latest = data.holdings.reduce<string | null>((last, h) => h.quote && !h.quote.manual && (!last || h.quote.at > last) ? h.quote.at : last, null)
  const selected = data.holdings.find(h => h.symbol === selection)
  const active = data.holdings.filter(h => Number(h.quantity) > 0).length
  const hasActivity = data.holdings.length > 0
  const cards = [
    {label:'Holdings value', Icon:Wallet, value:hasActivity ? money(summary.value) : '—', className:'', bottom:hasActivity ? money(summary.basis) + ' invested cost' : 'Market value of open positions'},
    {label:'Unrealized gain', Icon:TrendingUp,value:hasActivity ? money(summary.unrealized,true) : '—',className:tone(summary.unrealized),bottom:summary.unrealized_percent !== null ? percent(summary.unrealized_percent,true) + ' on invested cost' : 'Value minus remaining cost'},
    {label:'Realized gain', Icon:Coins,value:hasActivity ? money(summary.realized,true) : '—',className:tone(summary.realized),bottom:hasActivity ? money(summary.dividends) + ' recorded dividends' : 'Gains and losses on sales'},
    {label:summary.incomplete ? 'Recorded total gain' : 'Total gain', Icon:CircleDollarSign,value:hasActivity ? money(summary.total,true) : '—',className:tone(summary.total),bottom:summary.incomplete ? 'Includes opening balances' : 'Realized, unrealized and dividends'},
  ]
  return <>
    <div className="summary-grid">{cards.map(({label,Icon,value,className,bottom}) => <article className="card metric" key={label}>
      <div className="metric-label"><Icon size={15}/>{label}</div><div className={'metric-number ' + className}>{value}</div><div className="muted">{bottom}</div>
    </article>)}</div>
    {summary.missing.length > 0 && <Notice>Valuation is incomplete. Prices are unavailable for {summary.missing.join(', ')}.</Notice>}
    <section className="card holdings">
      <div className="section-head">
        <div><h2>Holdings</h2><p>{active} open {active === 1 ? 'position' : 'positions'}{data.holdings.length > active && ` · ${data.holdings.length - active} closed`}</p></div>
        <div className="table-tools">
          <SearchBox value={query} onChange={value => {setQuery(value); setPage(0)}} placeholder="Find a ticker"/>
          <select aria-label="Position status" value={position} onChange={e => {setPosition(e.target.value); setPage(0)}}>
            <option value="open">Open positions</option><option value="closed">Closed positions</option><option value="all">All positions</option>
          </select>
          <select aria-label="Filter by total gain" value={gain} onChange={e => {setGain(e.target.value); setPage(0)}}>
            <option value="any">Any gain</option><option value="positive">Gainers</option><option value="negative">Losers</option>
            <option value="flat">Break-even</option><option value="unpriced">Missing price</option>
          </select>
          <details className="columns"><summary className="button secondary" aria-label="Configure columns"><ListFilter size={15}/>Columns</summary><div className="column-menu">
            {table.getAllLeafColumns().filter(c => c.getCanHide()).map(c => <label key={c.id}><input type="checkbox" checked={c.getIsVisible()} onChange={c.getToggleVisibilityHandler()}/>{labels[c.id]}</label>)}
          </div></details>
        </div>
      </div>
      {hasActivity ? <div className="table-scroll"><table className="holdings-table">
        <thead>{table.getHeaderGroups().map(group => <tr key={group.id}>{group.headers.map(h => {
          const order = h.column.getIsSorted()
          return <th key={h.id} aria-sort={order === 'asc' ? 'ascending' : order === 'desc' ? 'descending' : 'none'}><button onClick={h.column.getToggleSortingHandler()}>
            {flexRender(h.column.columnDef.header,h.getContext())}{order === 'asc' ? <ArrowUp size={12}/> : order === 'desc' ? <ArrowDown size={12}/> : <ArrowDownUp size={11} className="quiet-sort"/>}
          </button></th>
        })}<th aria-label="Details"/></tr>)}</thead>
        <tbody>{table.getRowModel().rows.slice(pageIndex * PAGE_SIZE, (pageIndex + 1) * PAGE_SIZE).map(row => <tr key={row.original.symbol} onClick={() => setSelection(row.original.symbol)}>
          {row.getVisibleCells().map(c => <td key={c.id}>{flexRender(c.column.columnDef.cell,c.getContext())}</td>)}
          <td><button className="icon-btn" aria-label={`View ${row.original.symbol} details`}><ChevronRight size={16}/></button></td>
        </tr>)}</tbody>
      </table>{rows.length === 0 && <Empty icon={<Layers3 size={23}/>} title="No matching holdings" detail="Try a different ticker or change the filters."/>}</div> :
      <Empty icon={<Layers3 size={26}/>} title="Add your first investment" detail="Import a CSV of your trades or enter the shares you already hold.">
        <div className="actions center"><button className="button primary" onClick={onImport}>Import transactions</button><button className="button secondary" onClick={onAdd}><Plus size={16}/>Add existing holding</button></div>
      </Empty>}
      <TablePager count={rows.length} page={pageIndex} setPage={setPage}/>
      <div className="table-footer"><span title="Daily price move measures currently held shares against the previous regular-session close."><span className={`status-dot ${latest ? 'ok' : ''}`}/>{refresh ? 'Refreshing prices…' : priceStatus || (latest ? `${connected ? 'Alpaca SIP' : 'Cached Alpaca SIP'} · 15-minute delayed · latest price ${age(latest)}` : 'Alpaca SIP · 15-minute delayed')}</span><span>USD</span></div>
    </section>
    {selected && <HoldingDetail holding={selected} records={data.transactions.filter(r => r.event.symbol === selected.symbol)} timezone={timezone} onClose={() => setSelection(null)} onReload={onReload} onEdit={row => { setSelection(null); onEdit(row) }}/>}
  </>
}
function HoldingDetail({ holding: h, records, timezone, onClose, onReload, onEdit }: {
  holding: Holding; records: Record[]; timezone: string; onClose: () => void; onReload: () => Promise<void>; onEdit: (row: Record) => void
}) {
  const [manual, setManual] = useState(false)
  const [page, setPage] = useState(0)
  const pageIndex = validPage(page, records.length)
  const [error,setError] = useState('')
  const [busy,setBusy] = useState(false)
  const [price,setPrice] = useState(h.quote?.manual ? h.quote.price : '')
  const [time,setTime] = useState(localInput(h.quote?.manual ? h.quote.at : undefined))
  async function update(value: string | null) {
    setBusy(true)
    setError('')
    try {
      await api.override(h.symbol,value,value ? new Date(time).toISOString() : null)
      await onReload()
      setManual(false)
    } catch(e) { setError(String(e)) } finally { setBusy(false) }
  }
  function submit(event: FormEvent) { event.preventDefault(); void update(price) }
  return <Modal drawer onClose={onClose} title={h.symbol} description="Holding details">
    <div className="dialog-body">
      <article className="detail-hero"><SymbolLogo symbol={h.symbol}/><div><span className="muted">Market value</span><strong>{money(h.value)}</strong></div><LogoAttribution/></article>
      <dl className="stats">{[
        ['Shares',shares(h.quantity)],['Price',money(h.quote?.price)],['Average cost',money(h.average)],['Remaining cost',money(h.basis)],
        ['Unrealized gain',amount(h.unrealized)],['Realized gain',amount(h.realized)],['Dividends',money(h.dividends)],['Daily price move',amount(h.daily)],
      ].map(([label,value]) => <div key={String(label)}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
      {h.incomplete && <Notice>Realized gain and dividends reflect only recorded activity. Earlier history is unavailable.</Notice>}
      <section className="detail-section"><div className="subhead"><h3>Price source</h3>{Number(h.quantity) > 0 && <button className="button secondary small" onClick={() => setManual(!manual)}><Pencil size={14}/>Manual price</button>}</div>
      <p className="muted">{h.quote ? (h.quote.manual ? 'Manual override' : 'Alpaca SIP · 15-minute delayed') + ' · ' + date(h.quote.at,timezone) + ' · ' + age(h.quote.at) : 'Price unavailable.'}</p>
      {h.has_override && <button className="text-button" disabled={busy} onClick={() => void update(null)}><X size={13}/>Remove override</button>}
      {manual && <form onSubmit={submit} className="form-grid"><label>Price (USD)<input required value={price} onChange={e => setPrice(e.target.value)} inputMode="decimal"/></label><label>As of (local time)<input type="datetime-local" step="1" required value={time} onChange={e => setTime(e.target.value)}/></label><button className="button primary" disabled={busy}>Save manual price</button></form>}
      {error && <Notice error>{error}</Notice>}</section>
      <section className="detail-section"><h3><History size={16}/> Transactions</h3>
        <ul className="detail-transactions">{records.slice(pageIndex * PAGE_SIZE, (pageIndex + 1) * PAGE_SIZE).map(row => <li key={row.event.transaction_id}><div><strong>{kindName[row.event.type]}</strong>{row.excluded && <span className="badge">Excluded</span>}<small>{date(row.event.timestamp,timezone)}</small></div>
          <span>{row.event.quantity ? shares(row.event.quantity) + ' shares' : ''}</span><button className="icon-btn" aria-label={`View ${kindName[row.event.type]} transaction`} onClick={() => onEdit(row)}><ChevronRight size={15}/></button></li>)}</ul>
        <TablePager count={records.length} page={pageIndex} setPage={setPage}/>
      </section>
    </div>
  </Modal>
}
