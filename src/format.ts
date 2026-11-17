import type { Kind } from './api'

const dollars = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' })
const signedDollars = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', signDisplay: 'exceptZero' })
const percentages = new Intl.NumberFormat('en-US', { style: 'percent', minimumFractionDigits: 2, maximumFractionDigits: 2 })
const signedPercentages = new Intl.NumberFormat('en-US', { style: 'percent', minimumFractionDigits: 2, maximumFractionDigits: 2, signDisplay: 'exceptZero' })
export const money = (value?: string | null, signed = false) => value == null ? '—' : (signed ? signedDollars : dollars).format(Number(value))
export const percent = (value?: string | null, signed = false) => value == null ? '—' : (signed ? signedPercentages : percentages).format(Number(value))
export const tone = (value?: string | null) => Number(value) > 0 ? 'gain' : Number(value) < 0 ? 'loss' : ''
export const shares = (value?: string | null) => value == null ? '—' : value.replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '')
export const kindName: Record<Kind, string> = { BUY: 'Buy', SELL: 'Sell', OPENING_BALANCE: 'Opening balance', DIVIDEND: 'Dividend', SPLIT: 'Split' }

const dateFormats = new Map<string, Intl.DateTimeFormat>()

export function date(value: string, timezone = 'system', seconds = false) {
  try {
    const key = `${timezone}:${seconds}`
    let format = dateFormats.get(key)
    if (!format) {
      format = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: seconds ? 'medium' : 'short', timeZone: timezone === 'system' ? undefined : timezone })
      dateFormats.set(key, format)
    }
    return format.format(new Date(value))
  } catch { return value }
}
export function localInput(value = new Date().toISOString()) {
  const time = new Date(value)
  const shifted = new Date(time.getTime() - time.getTimezoneOffset() * 60_000)
  return shifted.toISOString().slice(0, 19)
}
export function age(value: string) {
  const mins = Math.max(0, Math.floor((Date.now() - Date.parse(value)) / 60_000))
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins} min ago`
  if (mins < 1440) return `${Math.floor(mins / 60)} hr ago`
  return `${Math.floor(mins / 1440)} days ago`
}
