import { useState, type FormEvent } from 'react'
import { Clock3, DatabaseBackup, Download, KeyRound, Upload } from 'lucide-react'
import { api, type Settings as Model, type Transfer } from './api'
import { Modal, Notice } from './components'

export default function Settings({ settings, update, transfer, autoRefresh, setAutomatic }: { settings: Model; update: () => Promise<void>; transfer: (operation: Transfer) => Promise<void>; autoRefresh: boolean; setAutomatic: (enabled: boolean) => void }) {
  const [key,setKey] = useState('')
  const [secret,setSecret] = useState('')
  const [live,setLive] = useState(false)
  const [timezone,setTimezone] = useState(settings.timezone)
  const [busy,setBusy] = useState(false)
  const [message,setMessage] = useState('')
  const [error,setError] = useState('')
  const [restore,setRestore] = useState(false)
  async function run(operation: () => Promise<void>, success: string) {
    setError('')
    setMessage('')
    setBusy(true)
    try {await operation();await update();setMessage(success)}
    catch(e) {setError(String(e))}
    finally {setBusy(false)}
  }
  function connect(e: FormEvent) {
    e.preventDefault()
    void run(async () => {
      await api.connect({key,secret,live})
      setKey('')
      setSecret('')
    }, 'Alpaca is connected.')
  }
  return <>
    {error && <Notice error>{error}</Notice>}{message && <Notice>{message}</Notice>}
    <section className="card settings-card">
      <div className="section-head"><div><h2><KeyRound size={18}/>Alpaca market data</h2><p>Consolidated US prices delayed by at least 15 minutes.</p></div><span className={'badge '+(settings.connected ? 'type-buy' : '')}>{settings.connected ? 'Connected' : 'Disconnected'}</span></div>
      <div className="settings-body">
        {settings.credential_error && <Notice error>{settings.credential_error}</Notice>}
        <form onSubmit={connect} className="form-grid credentials" autoComplete="off">
          <label>API key ID<input required value={key} onChange={e => setKey(e.target.value)} spellCheck={false} autoComplete="off"/></label>
          <label>API secret<input type="password" required value={secret} onChange={e => setSecret(e.target.value)} autoComplete="new-password"/></label>
          <label>Alpaca account<select value={live ? 'live':'paper'} onChange={e => setLive(e.target.value==='live')}><option value="paper">Paper</option><option value="live">Live</option></select></label>
          <div className="actions credentials-action"><button className="button primary" disabled={busy}>{busy ? 'Working…' : settings.connected ? 'Replace credentials' : 'Validate and connect'}</button>
            {settings.connected && <button type="button" className="button secondary" disabled={busy} onClick={() => void run(api.disconnect,'Alpaca credentials removed.')}>Disconnect</button>}</div>
        </form>
        <label className="toggle"><input type="checkbox" checked={autoRefresh} onChange={e => setAutomatic(e.target.checked)}/>Refresh prices automatically while Holdings is active</label>
        <p className="muted">Credentials are saved in your operating system’s credential store.</p>
      </div>
    </section>
    <section className="card settings-card">
      <div className="section-head"><div><h2><Clock3 size={18}/>Display timezone</h2><p>Controls timestamps in the holdings details, transaction ledger, and import history.</p></div></div>
      <div className="settings-body"><form className="inline-wrap" onSubmit={e => {e.preventDefault();void run(() => api.timezone(timezone.trim()),'Timezone saved.')}}>
        <input required aria-label="Display timezone" list="timezones" value={timezone} onChange={e => setTimezone(e.target.value)} placeholder="system"/>
        <datalist id="timezones">{['system',...(Intl.supportedValuesOf?.('timeZone') ?? ['America/New_York','Asia/Hong_Kong','UTC'])].map(zone => <option value={zone} key={zone}/>)}</datalist>
        <button className="button secondary" disabled={busy}>Save timezone</button>
      </form></div>
    </section>
    <section className="card settings-card">
      <div className="section-head"><div><h2><DatabaseBackup size={18}/>Backups and export</h2><p>A complete backup keeps your ledger, import history, exclusions, prices, and settings.</p></div></div>
      <div className="settings-body">
        <div className="inline-wrap"><button className="button secondary" onClick={() => void transfer('backup')}><Download size={15}/>Create backup</button>
          <button className="button secondary" onClick={() => setRestore(true)}><Upload size={15}/>Restore backup</button><button className="button secondary" onClick={() => void transfer('export')}><Download size={15}/>Export CSV</button></div>
        <p className="muted">Use the CSV to move active transactions. Use a SQLite backup to recover exclusions, import history, and settings.</p>
      </div>
    </section>
    {restore && <Modal title="Restore a backup" description="The selected backup will replace your current database. Portfolio saves a copy of your current database first." onClose={() => setRestore(false)}>
      <div className="dialog-actions"><button className="button secondary" onClick={() => setRestore(false)}>Cancel</button><button className="button primary" onClick={() => {setRestore(false);void transfer('restore')}}>Choose backup</button></div>
    </Modal>}
  </>
}
