import { type ButtonHTMLAttributes, type ReactNode, forwardRef, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

type Variant = "primary" | "secondary" | "ghost" | "danger";

export const Button = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }>(
  function Button({ variant = "secondary", size = "md", className = "", ...rest }, ref) {
    return <button ref={ref} type="button" className={`btn btn-${variant} btn-${size} ${className}`} {...rest} />;
  },
);

export const IconButton = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean }>(
  function IconButton({ label, active, className = "", children, ...rest }, ref) {
    return (
      <button ref={ref} type="button" aria-label={label} title={label} aria-pressed={active} className={`icon-btn ${active ? "is-active" : ""} ${className}`} {...rest}>
        {children}
      </button>
    );
  },
);

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export function Spinner({ size = 14 }: { size?: number }) {
  return <span className="spinner" style={{ width: size, height: size }} aria-hidden />;
}

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
export const mod = isMac ? "⌘" : "Ctrl";

// ---- Modal dialog -----------------------------------------------------------------------------

export function Modal({
  open,
  onClose,
  title,
  children,
  footer,
  width = 440,
  labelledBy,
}: {
  open: boolean;
  onClose: () => void;
  title?: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  labelledBy?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  if (!open) return null;
  return createPortal(
    <dialog
      ref={ref}
      className="modal"
      style={{ width }}
      aria-labelledby={labelledBy ?? (title ? "modal-title" : undefined)}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal-body">
        {title && (
          <h2 id="modal-title" className="modal-title">
            {title}
          </h2>
        )}
        {children}
      </div>
      {footer && <div className="modal-footer">{footer}</div>}
    </dialog>,
    document.body,
  );
}

// ---- Popover menu -------------------------------------------------------------------------------

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  danger?: boolean;
  shortcut?: string;
  disabled?: boolean;
}

export type MenuEntry = MenuItem | "separator";

/** A menu at a point (context menu) or under an element, closed by Esc, outside clicks or a choice. */
export function Menu({ at, items, onClose }: { at: { x: number; y: number }; items: MenuEntry[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState(at);
  const [active, setActive] = useState(-1);
  const choices = items.filter((i): i is MenuItem => i !== "separator");

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ x: Math.min(at.x, window.innerWidth - r.width - 8), y: Math.min(at.y, window.innerHeight - r.height - 8) });
    el.focus();
  }, [at]);

  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const blur = () => onClose();
    window.addEventListener("mousedown", close, true);
    window.addEventListener("blur", blur);
    window.addEventListener("resize", blur);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("blur", blur);
      window.removeEventListener("resize", blur);
    };
  }, [onClose]);

  const choose = (item: MenuItem) => {
    if (item.disabled) return;
    onClose();
    item.onSelect();
  };

  return createPortal(
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      className="menu"
      style={{ left: pos.x, top: pos.y }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onClose();
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          setActive((a) => (a + 1) % choices.length);
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          setActive((a) => (a <= 0 ? choices.length - 1 : a - 1));
        } else if (e.key === "Enter" && active >= 0) {
          e.preventDefault();
          const item = choices[active];
          if (item) choose(item);
        }
      }}
    >
      {items.map((item, i) =>
        item === "separator" ? (
          <div key={`sep-${i}`} className="menu-sep" role="separator" />
        ) : (
          <button
            key={item.label}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            className={`menu-item ${item.danger ? "is-danger" : ""} ${choices[active] === item ? "is-active" : ""}`}
            onMouseEnter={() => setActive(choices.indexOf(item))}
            onClick={() => choose(item)}
          >
            <span className="menu-icon">{item.icon}</span>
            <span className="menu-label">{item.label}</span>
            {item.shortcut && <span className="menu-shortcut">{item.shortcut}</span>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
