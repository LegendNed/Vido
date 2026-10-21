import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { Button } from "./controls";

/**
 * Modal confirmation for actions that replace or remove files. Enter confirms,
 * Esc cancels; other keys don't reach the page underneath while it is open.
 */
export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  children?: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  // Latest callbacks for the (mount-once) key listener.
  const actions = useRef({ onConfirm, onCancel });
  useEffect(() => {
    actions.current = { onConfirm, onCancel };
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Tab") return;
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        actions.current.onCancel();
      } else if (e.key === "Enter") {
        e.preventDefault();
        actions.current.onConfirm();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
    };
  }, []);

  return createPortal(
    <div className="confirm-scrim" role="presentation" onClick={onCancel}>
      <div
        className="confirm-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => {
          e.stopPropagation();
        }}
      >
        <h3>{title}</h3>
        {children && <div className="body">{children}</div>}
        <div className="row">
          <span className="spacer" />
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant="primary" onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
