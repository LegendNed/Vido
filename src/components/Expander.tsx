import { useId, type ReactNode } from "react";

import { Icon } from "./Icon";

/**
 * Fluent settings expander: a header row (title, description, chevron) that shows
 * or hides the rows below it. Rows are usually `Field`s.
 */
export function Expander({
  title,
  description,
  open,
  onToggle,
  children,
}: {
  title: string;
  description?: ReactNode;
  open: boolean;
  onToggle: (open: boolean) => void;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <div className={`expander${open ? " open" : ""}`}>
      <button
        type="button"
        className="expander-head"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => {
          onToggle(!open);
        }}
      >
        <span className="expander-label">
          <span>{title}</span>
          {description && <span className="desc">{description}</span>}
        </span>
        <span className="expander-chevron" aria-hidden="true">
          <Icon name="chevronRight" size={14} />
        </span>
      </button>
      {open && (
        <div className="expander-body" id={id}>
          {children}
        </div>
      )}
    </div>
  );
}
