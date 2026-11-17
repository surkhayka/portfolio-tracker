import { invoke } from '@tauri-apps/api/core'

export type Kind = 'BUY' | 'SELL' | 'OPENING_BALANCE' | 'DIVIDEND' | 'SPLIT'
export type Event = {
  transaction_id: string; timestamp: string; type: Kind; symbol: string; currency: string
  quantity: string | null; unit_price: string | null; amount: string | null; fees: string | null
  close_position: boolean | null; split_ratio: string | null; reported_pnl: string | null; notes: string | null
}
export type Record = { event: Event; sequence: number; excluded: boolean; reconciled_quantity: string | null; gross: string | null }
export type Account = { quantity: string; basis: string; realized: string; dividends: string; incomplete: boolean }
export type Quote = { price: string; at: string; fetched_at: string; manual: boolean }
export type Holding = Account & {
  symbol: string; average: string | null; value: string | null; allocation: string | null
  unrealized: string | null; unrealized_percent: string | null; total: string | null
  daily: string | null; daily_percent: string | null; has_override: boolean; quote: Quote | null
}
export type Valuation = {
  revision: number; holdings: Holding[]
  summary: Pick<Account, 'basis' | 'realized' | 'dividends' | 'incomplete'> & {
    value: string | null; unrealized: string | null; unrealized_percent: string | null
    total: string | null; missing: string[]
  }
}
export type Portfolio = Valuation & { transactions: Record[] }
export type Choice = 'skip' | 'include' | 'replace'
export type ImportAction = {
  action: 'import'; filename: string; contents: string; choices: { [row: number]: Choice }; replace_opening: string[]
}
export type Action =
  | ImportAction
  | { action: 'save'; event: Event; editing: boolean; opening_average?: string }
  | { action: 'exclude'; id: string; excluded: boolean }
  | { action: 'undo'; batch: number }
export type Preview = {
  revision: number; changed: number; conflicts: string[]; warnings: string[]; opening_conflicts: string[]
  rows: { row: number; event: Event | null; error: string | null; status: string; message: string | null }[]
  adjustments: { id: string; original: string; effective: string; confirmed: boolean }[]
  impacts: { symbol: string; before: Account; after: Account }[]
}
export type Settings = { connected: boolean; credential_error: string | null; timezone: string }
export type Batch = { id: number; filename: string; time: string; added: number; replaced: number; skipped: number; undone: boolean }
export type Transfer = 'backup' | 'restore' | 'export' | 'guide' | 'template' | 'example'
export const api = {
  portfolio: () => invoke<Portfolio>('portfolio'),
  batches: () => invoke<Batch[]>('list_batches'),
  preview: (action: Action) => invoke<Preview>('preview', { action }),
  commit: (action: Action, revision: number, confirm: string[]) => invoke<void>('commit', { action, revision, confirm }),
  settings: () => invoke<Settings>('get_settings'),
  timezone: (timezone: string) => invoke<void>('save_timezone', { timezone }),
  connect: (credentials: { key: string; secret: string; live: boolean }) => invoke<void>('connect', { credentials }),
  disconnect: () => invoke<void>('disconnect'),
  refresh: (manual: boolean) => invoke<{ received: number; valuation: Valuation | null }>('refresh_prices', { manual }),
  openingCost: (quantity: string, cost: string, average: boolean) => invoke<string>('opening_counterpart', { quantity, cost, average }),
  override: (symbol: string, value: string | null, at: string | null) => invoke<void>('set_override', { symbol, value, at }),
  transfer: (operation: Transfer) => invoke<boolean>('transfer', { operation }),
}
