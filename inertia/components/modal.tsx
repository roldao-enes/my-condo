import { useEffect, useRef, type PropsWithChildren } from 'react'
import { X } from 'lucide-react'
import { Notices } from './shell'

export default function Modal({
  title,
  onClose,
  children,
}: PropsWithChildren<{ title: string; onClose: () => void }>) {
  const dialog = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const element = dialog.current!
    element.showModal()
    return () => element.close()
  }, [])
  return (
    <dialog
      ref={dialog}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault()
          onClose()
        }
      }}
      onCancel={(event) => {
        event.preventDefault()
        onClose()
      }}
      aria-labelledby="dialog-title"
    >
      <div className="dialog-heading">
        <h2 id="dialog-title">{title}</h2>
        <button
          className="icon-button"
          type="button"
          title="Fechar"
          aria-label="Fechar"
          onClick={onClose}
        >
          <X size={20} />
        </button>
      </div>
      <div className="dialog-content">
        <Notices />
        {children}
      </div>
    </dialog>
  )
}
