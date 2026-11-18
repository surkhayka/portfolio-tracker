import type { ReactNode } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { AlertCircle, Search, X } from 'lucide-react'

export function Modal({ title, description, children, onClose, drawer = false, wide = false }: {
  title: string; description: string; children: ReactNode; onClose: () => void; drawer?: boolean; wide?: boolean
}) {
  return <Dialog.Root open onOpenChange={open => { if (!open) onClose() }}>
    <Dialog.Portal>
      <Dialog.Overlay className="overlay" />
      <Dialog.Content className={drawer ? 'drawer' : wide ? 'dialog wide' : 'dialog'}>
        <div className="dialog-heading">
          <div><Dialog.Title>{title}</Dialog.Title><Dialog.Description>{description}</Dialog.Description></div>
          <Dialog.Close className="icon-btn" aria-label="Close"><X size={19}/></Dialog.Close>
        </div>
        {children}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
}
export function Notice({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <div className={'notice ' + (error ? 'notice-error' : '')} role={error ? 'alert' : 'status'}><AlertCircle size={17}/><div>{children}</div></div>
}
export function SearchBox({ value, onChange, placeholder }: { value: string; onChange: (q: string) => void; placeholder: string }) {
  return <label className="search-box"><Search size={16}/><input data-search="" aria-label={placeholder} placeholder={placeholder} value={value} onChange={e => onChange(e.target.value)}/>{value && <button className="icon-btn clear-search" aria-label="Clear search" onClick={() => onChange('')}><X size={14}/></button>}</label>
}
export function Empty({ icon, title, detail, children }: { icon: ReactNode; title: string; detail: string; children?: ReactNode }) {
  return <div className="empty"><div className="empty-icon">{icon}</div><h3>{title}</h3><p>{detail}</p>{children}</div>
}

export const PAGE_SIZE = 50

export function TablePager({ count, page, setPage }: { count: number; page: number; setPage: (page: number) => void }) {
  if (count <= PAGE_SIZE) return null
  return <div className="table-footer pager">
    <span>{page * PAGE_SIZE + 1}–{Math.min(count, (page + 1) * PAGE_SIZE)} of {count}</span>
    <div className="actions">
      <button className="button secondary small" disabled={page === 0} onClick={() => setPage(page - 1)}>Previous</button>
      <button className="button secondary small" disabled={(page + 1) * PAGE_SIZE >= count} onClick={() => setPage(page + 1)}>Next</button>
    </div>
  </div>
}

export function validPage(page: number, count: number) {
  return Math.min(page, Math.max(0, Math.ceil(count / PAGE_SIZE) - 1))
}
