import { useEffect, useMemo, useState, type FormEvent } from 'react'
import { Eye, EyeOff, Pencil, Plus, ReceiptText } from 'lucide-react'
import { api, type Action, type Event, type Kind, type Record } from './api'
import { Empty, Modal, Notice, PAGE_SIZE, SearchBox, TablePager, validPage } from './components'
import { SymbolLogo } from './SymbolLogo'
import { date, kindName, localInput, money, shares } from './format'

export function Transactions({ rows, timezone, add, edit, change }: {
  rows: Record[]; timezone: string; add: () => void; edit: (r: Record) => void; change: (action: Action, title: string) => void
}) {
  const [term,setTerm] = useState('')
  const [kind,setKind] = useState('')
  const [status,setStatus] = useState('included')
  const [page,setPage] = useState(0)
  const filtered = useMemo(() => {
    const ticker = term.trim().toUpperCase()
    return rows.filter(r => (status === 'all' || r.excluded === (status === 'excluded'))
      && (!kind || r.event.type === kind) && r.event.symbol.includes(ticker))
  }, [rows, kind, status, term])
  const pageIndex = validPage(page, filtered.length)
  return <section className="card">
    <div className="section-head"><div><h2>Ledger</h2><p>{filtered.length} of {rows.length} transactions</p></div>
      <div className="table-tools"><SearchBox value={term} onChange={value => {setTerm(value); setPage(0)}} placeholder="Find a ticker"/>
        <select aria-label="Filter by transaction type" value={kind} onChange={e => {setKind(e.target.value); setPage(0)}}><option value="">All types</option>{Object.entries(kindName).map(([key,label]) => <option key={key} value={key}>{label}</option>)}</select>
        <select aria-label="Transaction inclusion" value={status} onChange={e => {setStatus(e.target.value); setPage(0)}}><option value="included">Included</option><option value="excluded">Excluded only</option><option value="all">All transactions</option></select>
      </div>
    </div>
    {filtered.length ? <div className="table-scroll ledger-scroll"><table><thead><tr><th>Date & time</th><th>Symbol</th><th>Type</th><th>Shares</th><th>Price</th><th>Amount</th><th aria-label="Transaction actions"/></tr></thead>
      <tbody>{filtered.slice(pageIndex * PAGE_SIZE, (pageIndex + 1) * PAGE_SIZE).map(row => <tr key={row.event.transaction_id} className={row.excluded ? 'disabled-row' : ''}>
        <td>{date(row.event.timestamp,timezone)}</td><td><span className="symbol"><SymbolLogo symbol={row.event.symbol}/><strong>{row.event.symbol}</strong></span></td>
        <td><span className={'badge type-' + row.event.type.toLowerCase()}>{kindName[row.event.type]}</span>{row.excluded && <span className="badge">Excluded</span>}</td>
        <td>{shares(row.reconciled_quantity ?? row.event.quantity)}</td><td>{money(row.event.unit_price)}</td><td title={row.event.amount ? undefined : 'Calculated from execution price and quantity when applicable'}>{money(row.gross)}</td>
        <td><div className="row-actions"><button className="icon-btn" aria-label={`Edit ${row.event.symbol} transaction`} onClick={() => edit(row)}><Pencil size={15}/></button><button className="icon-btn" aria-label={row.excluded ? `Include ${row.event.symbol} transaction` : `Exclude ${row.event.symbol} transaction`} onClick={() => change({action:'exclude',id:row.event.transaction_id,excluded:!row.excluded},row.excluded ? 'Include transaction' : 'Exclude transaction')}>{row.excluded ? <Eye size={16}/> : <EyeOff size={16}/>}</button></div></td>
      </tr>)}</tbody></table></div> :
      <Empty icon={<ReceiptText size={25}/>} title={rows.length ? 'No matching transactions' : 'No transactions yet'} detail={rows.length ? 'Change the filters to see more of your ledger.' : 'Record a trade or import a CSV to start your ledger.'}>
        {!rows.length && <button className="button primary" onClick={add}><Plus size={16}/>Record transaction</button>}
      </Empty>}
    <TablePager count={filtered.length} page={pageIndex} setPage={setPage}/>
  </section>
}

export function TransactionForm({ initial, opening, hidden, onClose, onSubmit }: {
  initial?: Record; opening: boolean; hidden?: boolean; onClose: () => void; onSubmit: (action: Action) => void
}) {
  const event = initial?.event
  const [kind,setKind] = useState<Kind>(event?.type ?? (opening ? 'OPENING_BALANCE' : 'BUY'))
  const [symbol,setSymbol] = useState(event?.symbol ?? '')
  const [time,setTime] = useState(localInput(event?.timestamp))
  const [quantity,setQuantity] = useState(event?.quantity ?? '')
  const [price,setPrice] = useState(event?.unit_price ?? '')
  const [amount,setAmount] = useState(event?.amount ?? '')
  const [fees,setFees] = useState(event?.fees ?? '')
  const [close,setClose] = useState(event?.close_position ?? false)
  const [split,setSplit] = useState(event?.split_ratio ?? '')
  const [reported,setReported] = useState(event?.reported_pnl ?? '')
  const [notes,setNotes] = useState(event?.notes ?? '')
  const [costMode,setCostMode] = useState('total')
  const [counterpart,setCounterpart] = useState<string | null>(null)
  const [error,setError] = useState('')
  const isOpening = kind === 'OPENING_BALANCE'
  const trade = kind === 'BUY' || kind === 'SELL'
  const disabled = initial?.excluded ?? false
  useEffect(() => {
    setCounterpart(null)
    if (!isOpening || !quantity || !amount) return
    let active = true
    api.openingCost(quantity,amount,costMode === 'average').then(result => {if (active) setCounterpart(result)}).catch(() => {})
    return () => { active = false }
  }, [isOpening,quantity,amount,costMode])
  const submit = (e: FormEvent) => {
    e.preventDefault()
    setError('')
    const next: Event = {
      transaction_id:event?.transaction_id ?? crypto.randomUUID(), timestamp:'', type:kind, symbol, currency:'USD',
      quantity:trade || isOpening ? quantity : null, unit_price:trade ? price : null,
      amount:trade || kind === 'DIVIDEND' || (isOpening && costMode === 'total') ? amount || null : null,
      fees:trade || kind === 'DIVIDEND' ? fees || null : null, close_position:kind === 'SELL' && close ? true : null,
      split_ratio:kind === 'SPLIT' ? split : null, reported_pnl:kind === 'SELL' ? reported || null : null, notes:notes || null,
    }
    try {
      next.timestamp = event && time === localInput(event.timestamp) ? event.timestamp : new Date(time).toISOString()
      onSubmit({action:'save',event:next,editing:!!initial,opening_average:isOpening && costMode === 'average' ? amount : undefined})
    } catch(e) {setError(String(e))}
  }
  if (hidden) return null
  return <Modal title={disabled ? 'View transaction' : initial ? 'Edit transaction' : isOpening ? 'Add existing holding' : 'Record transaction'}
    description={isOpening ? 'Set the starting quantity and cost for a holding without earlier records.' : 'Enter the original trade or event details.'} onClose={onClose}>
    <form onSubmit={submit}>
      <div className="dialog-body"><fieldset disabled={disabled} className="form-grid">
        {!isOpening && <label>Type<select value={kind} onChange={e => {setKind(e.target.value as Kind); setAmount(''); setFees('')}}>{(['BUY','SELL','DIVIDEND','SPLIT'] as const).map(k => <option key={k} value={k}>{kindName[k]}</option>)}</select></label>}
        <label>Ticker<input required maxLength={16} value={symbol} onChange={e => setSymbol(e.target.value.toUpperCase())} placeholder="AAPL"/></label>
        <label>{isOpening ? 'As of' : 'Date & time'} (local)<input type="datetime-local" step="1" value={time} required onChange={e => setTime(e.target.value)}/></label>
        {(trade || isOpening) && <label>Shares<input required inputMode="decimal" placeholder="0.00" value={quantity} onChange={e => setQuantity(e.target.value)}/></label>}
        {trade && <label>Execution price (USD)<input required inputMode="decimal" placeholder="0.00" value={price} onChange={e => setPrice(e.target.value)}/></label>}
        {isOpening && <label>Enter cost as<select value={costMode} onChange={e => {const result=counterpart; setCostMode(e.target.value); setAmount(result ?? '')}}><option value="total">Total cost basis</option><option value="average">Average cost per share</option></select></label>}
        {(trade || isOpening || kind === 'DIVIDEND') && <label>{isOpening ? (costMode === 'total' ? 'Total cost basis' : 'Average cost per share') : kind === 'DIVIDEND' ? 'Dividend received' : 'Gross amount (optional)'}<input required={!trade} inputMode="decimal" placeholder="0.00" value={amount} onChange={e => setAmount(e.target.value)}/></label>}
        {isOpening && <label>{costMode === 'total' ? 'Average cost per share' : 'Total cost basis'}<input value={counterpart ?? ''} placeholder="Calculated" readOnly/></label>}
        {(trade || kind === 'DIVIDEND') && <label>Fees (optional)<input inputMode="decimal" placeholder="0.00" value={fees} onChange={e => setFees(e.target.value)}/></label>}
        {kind === 'SELL' && <><label>Reported PnL (optional)<input inputMode="decimal" value={reported} onChange={e => setReported(e.target.value)}/></label><label className="toggle form-wide"><input type="checkbox" checked={close} onChange={e => setClose(e.target.checked)}/>This sale closes my position</label></>}
        {kind === 'SPLIT' && <label>New shares per old share<input required inputMode="decimal" placeholder="2 for a 2-for-1 split" value={split} onChange={e => setSplit(e.target.value)}/></label>}
        <label className="form-wide">Notes (optional)<textarea rows={2} value={notes} onChange={e => setNotes(e.target.value)}/></label>
      </fieldset>
      {initial?.reconciled_quantity && <Notice>Closing quantity reconciled from {shares(event?.quantity)} to {shares(initial.reconciled_quantity)} shares.</Notice>}
      {disabled && <Notice>This transaction is excluded. Include it from the ledger to make edits.</Notice>}
      {error && <Notice error>{error}</Notice>}</div>
      <footer className="dialog-actions"><button type="button" className="button secondary" onClick={onClose}>{disabled ? 'Close' : 'Cancel'}</button>{!disabled && <button className="button primary">Review changes</button>}</footer>
    </form>
  </Modal>
}
