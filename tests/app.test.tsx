import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import App from '../src/App'
import { api, type Action, type Holding, type Portfolio, type Preview, type Record } from '../src/api'
import { Review } from '../src/Imports'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const empty: Portfolio = {revision:0,holdings:[],transactions:[],summary:{basis:'0',value:'0',unrealized:'0',unrealized_percent:null,realized:'0',dividends:'0',total:'0',incomplete:false,missing:[]}}
const result: Preview = {revision:7,changed:1,conflicts:[],warnings:[],opening_conflicts:[],rows:[],adjustments:[],
  impacts:[{symbol:'AAPL',before:{quantity:'0',basis:'0',realized:'0',dividends:'0',incomplete:false},
    after:{quantity:'2',basis:'200',realized:'0',dividends:'0',incomplete:true}}]}

beforeEach(() => {
  vi.spyOn(api,'portfolio').mockResolvedValue(empty)
  vi.spyOn(api,'batches').mockResolvedValue([])
  vi.spyOn(api,'settings').mockResolvedValue({connected:false,timezone:'system',credential_error:null})
  vi.spyOn(api,'openingCost').mockResolvedValue('100')
  vi.spyOn(api,'preview').mockResolvedValue(result)
  vi.spyOn(api,'commit').mockResolvedValue(undefined)
})
afterEach(() => {cleanup();vi.restoreAllMocks()})

describe('desktop interactions', () => {
  it('keeps an opening balance draft when returning from preview, then commits its revision', async () => {
    const user = userEvent.setup()
    render(<App/>)
    await user.click(await screen.findByRole('button',{name:'Add existing holding'}))
    await user.type(screen.getByLabelText('Ticker'),'AAPL')
    await user.type(within(screen.getByRole('dialog')).getByLabelText('Shares'),'2')
    await user.type(screen.getByLabelText('Total cost basis'),'200')
    await waitFor(() => expect((screen.getByLabelText('Average cost per share') as HTMLInputElement).value).toBe('100'))
    await user.click(screen.getByRole('button',{name:'Review changes'}))
    expect(await screen.findByText('$0.00 → $200.00')).toBeTruthy()
    await user.click(screen.getByRole('button',{name:'Back to edit'}))
    expect((screen.getByLabelText('Ticker') as HTMLInputElement).value).toBe('AAPL')
    await user.click(screen.getByRole('button',{name:'Review changes'}))
    await user.click(await screen.findByRole('button',{name:'Confirm changes'}))
    await waitFor(() => expect(api.commit).toHaveBeenCalledWith(expect.objectContaining({action:'save',event:expect.objectContaining({type:'OPENING_BALANCE',quantity:'2',amount:'200'})}),7,[]))
    expect(await screen.findByText('Changes saved.')).toBeTruthy()
  })

  it('requires CSV conflict resolution and explicit rounding confirmation', async () => {
    const user = userEvent.setup()
    const initial: Action = {action:'import',filename:'sample.csv',contents:'csv',choices:{},replace_opening:[]}
    vi.mocked(api.preview).mockImplementation(async action => ({
      ...result, rows:[{row:2,event:null,error:null,status:action.action === 'import' && action.choices[2] === 'replace' ? 'replace' : 'conflict',message:'Trade ID differs.'}],
      conflicts:action.action === 'import' && action.choices[2] === 'replace' ? [] : ['Conflicting row'],
      adjustments:[{id:'trade-2',original:'2.0000',effective:'2.0001',confirmed:false}],
    }))
    render(<Review initial={initial} title="Review CSV import" saved={async() => {}} onClose={() => {}}/>)
    const dialog = await screen.findByRole('dialog')
    const decision = await within(dialog).findByRole('combobox',{name:'Decision for row 2'})
    const save = within(dialog).getByRole('button',{name:'Import 1 changes'}) as HTMLButtonElement
    expect(save.disabled).toBe(true)
    await user.selectOptions(decision,'replace')
    await waitFor(() => expect(within(dialog).queryByText('Conflicting row')).toBeNull())
    expect(save.disabled).toBe(true)
    await user.click(within(dialog).getByRole('checkbox',{name:/Reconcile close/}))
    expect(save.disabled).toBe(false)
    await user.click(save)
    expect(api.commit).toHaveBeenCalledWith(expect.objectContaining({choices:{2:'replace'}}),7,['trade-2'])
  })
})


function holding(symbol: string, value: string | null, total: string | null, quantity = '1'): Holding {
  return {symbol, quantity, value, total, basis:'100', realized:'0', dividends:'0', incomplete:false,
    average:'100', allocation:null, unrealized:total, unrealized_percent:null, daily:null, daily_percent:null,
    has_override:false, quote:value === null || quantity === '0' ? null : {
      price:value, at:'2026-11-17T15:00:00Z', fetched_at:'2026-11-17T15:15:10Z', manual:false
    }}
}

it('sorts priced holdings numerically and combines position and gain filters', async () => {
  const user = userEvent.setup()
  vi.mocked(api.portfolio).mockResolvedValue({...empty, holdings:[
    holding('AAA','20','-80'), holding('BBB','125','25'), holding('CCC',null,null), holding('DDD','0','10','0'),
  ]})
  render(<App/>)
  const status = await screen.findByRole('combobox',{name:'Position status'})
  const gain = screen.getByRole('combobox',{name:'Filter by total gain'})
  const order = () => within(screen.getByRole('table')).getAllByRole('row').slice(1).map(row => within(row).getAllByRole('cell')[0].textContent)
  expect(order()).toEqual(['BBBBB','AAAAA','CCCCC'])
  await user.click(screen.getByRole('button',{name:'Market value'}))
  expect(order()).toEqual(['AAAAA','BBBBB','CCCCC'])
  await user.selectOptions(status,'all')
  await user.selectOptions(gain,'positive')
  expect(order()).toEqual(['DDDDDClosed','BBBBB'])
  await user.selectOptions(status,'closed')
  expect(order()).toEqual(['DDDDDClosed'])
  await user.selectOptions(status,'all')
  await user.selectOptions(gain,'unpriced')
  expect(order()).toEqual(['CCCCC'])
})

it('updates prices without reloading the ledger or polling when the user opens Transactions', async () => {
  vi.spyOn(document,'hasFocus').mockReturnValue(true)
  vi.spyOn(document,'hidden','get').mockReturnValue(false)
  const original = holding('AAPL','100','0')
  vi.mocked(api.portfolio).mockResolvedValue({...empty, holdings:[original]})
  vi.mocked(api.settings).mockResolvedValue({connected:true,timezone:'system',credential_error:null})
  const refresh = vi.spyOn(api,'refresh').mockResolvedValue({
    received:1,valuation:{revision:0,holdings:[holding('AAPL','137','37')],summary:empty.summary},
  })
  const user = userEvent.setup()
  render(<App/>)
  expect(await screen.findAllByText('$137.00')).toHaveLength(2)
  expect(api.portfolio).toHaveBeenCalledTimes(1)
  await act(async () => { window.dispatchEvent(new Event('focus')) })
  expect(refresh).toHaveBeenCalledTimes(1)
  await user.click(screen.getByRole('button',{name:'Transactions'}))
  await act(async () => { window.dispatchEvent(new Event('focus')) })
  expect(refresh).toHaveBeenCalledTimes(1)
})

it('keeps a large transaction history responsive with pages and filters across the full history', async () => {
  const records: Record[] = Array.from({length:2000}, (_, index) => ({
    sequence:index, excluded:index === 1999, reconciled_quantity:null, gross:'100',
    event:{transaction_id:`row-${index}`,timestamp:'2026-11-17T14:30:00Z',type:index === 1999 ? 'SELL' : 'BUY',
      symbol:index === 1999 ? 'LAST' : 'AAPL',currency:'USD',quantity:'1',unit_price:'100',amount:null,fees:null,
      close_position:null,split_ratio:null,reported_pnl:null,notes:null},
  }))
  vi.mocked(api.portfolio).mockResolvedValue({...empty, transactions:records})
  const user = userEvent.setup()
  render(<App/>)
  await user.click(screen.getByRole('button',{name:'Transactions'}))
  const table = await screen.findByRole('table')
  expect(table.querySelector('tbody')?.rows).toHaveLength(50)
  await user.click(screen.getByRole('button',{name:'Next'}))
  expect(screen.getByText('51–100 of 1999')).toBeTruthy()
  await user.selectOptions(screen.getByRole('combobox',{name:'Transaction inclusion'}),'all')
  await user.selectOptions(screen.getByRole('combobox',{name:'Filter by transaction type'}),'SELL')
  expect(table.querySelector('tbody')?.rows).toHaveLength(1)
  expect(within(table).getByText('LAST')).toBeTruthy()
})
