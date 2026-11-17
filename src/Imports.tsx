import { useEffect, useRef, useState, type DragEvent } from 'react'
import { Check, CloudUpload, Download, FileText, History, RotateCcw, Upload, X } from 'lucide-react'
import { api, type Action, type Batch, type Choice, type ImportAction, type Preview, type Transfer } from './api'
import { Empty, Modal, Notice } from './components'
import { date, kindName, money, shares } from './format'

export function Imports({ batches, timezone, review, transfer }: {
  batches: Batch[]; timezone: string; review: (action: Action, title: string) => void; transfer: (operation: Transfer) => Promise<void>
}) {
  const [dragging,setDragging] = useState(false)
  const [error,setError] = useState('')
  const input = useRef<HTMLInputElement>(null)
  const read = async (file?: File) => {
    if (!file) return
    setError('')
    if (!file.name.toLowerCase().endsWith('.csv')) {setError('Choose a CSV file using the Portfolio template.'); return}
    if (file.size > 8 * 1024 * 1024) {setError('CSV files must be 8 MB or smaller.'); return}
    try {
      const contents = new TextDecoder('utf-8', {fatal:true}).decode(await file.arrayBuffer())
      review({action:'import',filename:file.name,contents,choices:{},replace_opening:[]}, 'Review CSV import')
    } catch {setError('This file could not be read as UTF-8. Save the CSV in UTF-8 and try again.')}
  }
  const drop = (e: DragEvent) => {e.preventDefault();setDragging(false);void read(e.dataTransfer.files[0])}
  return <>
    <section className="card import-card">
      <div className="section-head"><div><h2>Import CSV</h2><p>Use the Portfolio template for personal USD stock and ETF transactions.</p></div></div>
      <div className="import-body">
        <div onDragOver={e => {e.preventDefault();setDragging(true)}} onDragLeave={() => setDragging(false)} onDrop={drop}
          className={'dropzone ' + (dragging ? 'dragging' : '')}>
          <div className="empty-icon"><CloudUpload size={25}/></div><h3>Drop a CSV file here</h3><p>Choose a file to review the transactions before saving.</p>
          <input ref={input} type="file" accept=".csv,text/csv" hidden onChange={e => {void read(e.target.files?.[0]); e.target.value = ''}}/>
          <button className="button primary" onClick={() => input.current?.click()}><Upload size={16}/>Choose CSV</button>
        </div>
        {error && <Notice error>{error}</Notice>}
        <div className="import-downloads">
          <button className="button secondary" onClick={() => void transfer('template')}><Download size={15}/>CSV template</button>
          <button className="button secondary" onClick={() => void transfer('example')}><Download size={15}/>Example CSV</button>
          <button className="button secondary" onClick={() => void transfer('guide')}><FileText size={15}/>Broker PDF to CSV guide</button>
        </div>
      </div>
    </section>
    <section className="card">
      <div className="section-head"><div><h2>Import history</h2><p>Review or undo a previous batch.</p></div></div>
      {batches.length ? <div className="table-scroll"><table><thead><tr><th>File</th><th>Imported</th><th>Added</th><th>Replaced</th><th>Skipped</th><th>Status</th><th aria-label="Import actions"/></tr></thead><tbody>
        {batches.map(b => <tr key={b.id}><td><span className="symbol"><FileText size={15}/><strong>{b.filename}</strong></span></td><td>{date(b.time,timezone)}</td><td>{b.added}</td><td>{b.replaced}</td><td>{b.skipped}</td><td><span className={'badge ' + (!b.undone ? 'type-buy' : '')}>{b.undone ? 'Undone' : 'Imported'}</span></td>
          <td>{!b.undone && <button className="button secondary small" onClick={() => review({action:'undo',batch:b.id}, 'Undo import')}><RotateCcw size={13}/>Undo</button>}</td></tr>)}
      </tbody></table></div> : <Empty icon={<History size={22}/>} title="No imports yet" detail="Your first import will appear here."/>}
    </section>
  </>
}

const statuses: { [key: string]: string } = {new:'New',duplicate:'Duplicate',excluded:'Excluded',skipped:'Skipped',replace:'Replace',conflict:'Conflict',possible_duplicate:'Possible duplicate',blocked:'Needs review'}
function ReviewRows({ result, action, change, timezone }: {result: Preview; action: ImportAction; change: (value: ImportAction) => void; timezone: string}) {
  const [issues,setIssues] = useState(false)
  const select = (row: number, choice?: Choice) => {
    const next = {...action.choices}
    if (choice) next[row] = choice
    else delete next[row]
    change({...action,choices:next})
  }
  const counts = Object.entries(result.rows.reduce<{[key: string]: number}>((acc,row) => {acc[row.status]=(acc[row.status] ?? 0)+1;return acc},{}))
  const visible = result.rows.filter(row => !issues || ['blocked','conflict','possible_duplicate'].includes(row.status))
  return <>
    <div className="review-toolbar"><div className="inline-wrap">{counts.map(([key,count]) => <span className="badge" key={key}>{count} {statuses[key]?.toLowerCase()}</span>)}</div>
      <label className="toggle"><input type="checkbox" checked={issues} onChange={e => setIssues(e.target.checked)}/>Needs review only</label></div>
    {result.opening_conflicts.map(symbol => <Notice key={symbol}>{symbol} has an opening balance. Skip overlapping CSV rows to keep it, or replace it with complete history. <button className="text-button" onClick={() => change({...action,replace_opening:[...action.replace_opening,symbol]})}>Replace {symbol} opening balance</button></Notice>)}
    {action.replace_opening.length > 0 && <div className="inline-wrap">{action.replace_opening.map(symbol => <button key={symbol} className="button secondary small" onClick={() => change({...action,replace_opening:action.replace_opening.filter(s => s !== symbol)})}>Replacing {symbol} opening balance <X size={13}/></button>)}</div>}
    <div className="table-scroll review-rows"><table><thead><tr><th>Row</th><th>Trade</th><th>Shares</th><th>Price</th><th>Status</th><th>Decision</th></tr></thead><tbody>
      {visible.map(row => <tr key={row.row}><td>{row.row}</td><td>{row.event ? row.event.symbol + ' · ' + kindName[row.event.type] : 'Invalid row'}{row.event && <small className="row-message" title={row.event.transaction_id}>{date(row.event.timestamp, timezone, true)}</small>}</td><td>{shares(row.event?.quantity)}</td><td title={row.event?.amount ? `Stated amount: ${money(row.event.amount)}` : undefined}>{money(row.event?.unit_price)}</td>
        <td><span className={'badge ' + (row.status === 'new' ? 'type-buy' : row.message ? 'type-sell' : '')}>{statuses[row.status]}</span>{row.message && <small className="row-message">{row.message}</small>}</td>
        <td>{row.status === 'excluded' || row.status === 'duplicate' ? '—' : <select aria-label={`Decision for row ${row.row}`} value={action.choices[row.row] ?? ''} onChange={e => select(row.row, (e.target.value || undefined) as Choice | undefined)}>
          <option value="">Default</option><option value="skip">{row.status === 'conflict' ? 'Keep existing' : 'Skip row'}</option>
          {(row.status === 'possible_duplicate' || action.choices[row.row] === 'include') && <option value="include">Include separately</option>}
          {(row.status === 'conflict' || action.choices[row.row] === 'replace') && <option value="replace">Replace existing</option>}
        </select>}</td></tr>)}
    </tbody></table></div>
  </>
}

export function Review({ initial, title, saved, onClose, onBack, timezone = 'system' }: {
  initial: Action; title: string; saved: () => Promise<void>; onClose: () => void; onBack?: () => void; timezone?: string
}) {
  const [action,setAction] = useState(initial)
  const [result,setResult] = useState<Preview | null>(null)
  const [pending,setPending] = useState(true)
  const [saving,setSaving] = useState(false)
  const [error,setError] = useState('')
  const [confirmed,setConfirmed] = useState<string[]>([])
  const [retry,setRetry] = useState(0)
  useEffect(() => {
    let active = true
    setPending(true)
    setError('')
    setConfirmed([])
    api.preview(action).then(value => {if (active) setResult(value)}).catch(e => {
      if (active) {setResult(null);setError(String(e))}
    }).finally(() => {if (active) setPending(false)})
    return () => {active=false}
  }, [action,retry])
  async function save() {
    if (!result || pending) return
    setSaving(true)
    setError('')
    try {
      await api.commit(action,result.revision,confirmed)
    } catch(e) {
      setError(String(e))
      setSaving(false)
      return
    }
    onClose()
    await saved()
  }
  const blocked = !result || result.conflicts.length > 0 || result.adjustments.some(a => !a.confirmed && !confirmed.includes(a.id))
  return <Modal title={title} description={action.action === 'import' ? action.filename : 'Review the effect on your personal holdings before saving.'} onClose={() => {if (!saving) onClose()}} wide={action.action === 'import'}>
    <div className="dialog-body review" aria-busy={pending || saving}>
      {error && <Notice error>{error}{error.includes('preview again') && <button className="text-button" onClick={() => setRetry(x => x+1)}>Refresh preview</button>}</Notice>}
      {pending && <p className="muted" role="status">Checking the ledger…</p>}
      {result && <>
        {action.action === 'import' && <ReviewRows result={result} action={action} timezone={timezone} change={next => {setPending(true);setAction(next)}}/>}
        {result.conflicts.length > 0 && <Notice error><strong>{result.conflicts.length} {result.conflicts.length === 1 ? 'issue needs' : 'issues need'} attention</strong><ul>{result.conflicts.slice(0,8).map((s,i) => <li key={i}>{s}</li>)}</ul></Notice>}
        {result.warnings.length > 0 && <Notice><strong>Review before saving</strong><ul>{result.warnings.map((s,i) => <li key={i}>{s}</li>)}</ul></Notice>}
        {result.adjustments.map(a => <Notice key={a.id}>
          <label className="toggle adjustment"><input type="checkbox" checked={a.confirmed || confirmed.includes(a.id)} disabled={a.confirmed || pending || saving}
            onChange={e => setConfirmed(e.target.checked ? [...confirmed,a.id] : confirmed.filter(id => id !== a.id))}/>Reconcile close from {shares(a.original)} to {shares(a.effective)} shares ({a.id})</label>
        </Notice>)}
        <section><h3>Holding changes</h3>
          {result.impacts.length ? <div className="table-scroll"><table><thead><tr><th>Ticker</th><th>Shares</th><th>Cost basis</th><th>Realized gain</th></tr></thead><tbody>{result.impacts.map(i => <tr key={i.symbol}><td><strong>{i.symbol}</strong></td><td>{shares(i.before.quantity)} → {shares(i.after.quantity)}</td><td>{money(i.before.basis)} → {money(i.after.basis)}</td><td>{money(i.before.realized,true)} → {money(i.after.realized,true)}</td></tr>)}</tbody></table></div> : <p className="muted">No changes to holdings.</p>}
        </section>
      </>}
    </div>
    <footer className="dialog-actions"><button className="button secondary" disabled={saving} onClick={onBack ?? onClose}>{onBack ? 'Back to edit' : 'Cancel'}</button>
      <button className="button primary" disabled={saving || pending || blocked || (result?.changed === 0 && action.action !== 'undo')} onClick={() => void save()}><Check size={16}/>{saving ? 'Saving…' : action.action === 'import' ? `Import ${result?.changed ?? 0} changes` : action.action === 'undo' ? 'Undo batch' : 'Confirm changes'}</button></footer>
  </Modal>
}
