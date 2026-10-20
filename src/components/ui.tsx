import { useEffect, useState, type ReactNode } from "react";

import {
  highlight,
  nodeText,
  SettingsSearchContext,
  useSearchRow,
  useSearchScope,
} from "../lib/settingsSearch";
import { Icon, type IconName } from "./Icon";

export function Button({
  children,
  onClick,
  variant,
  small,
  icon,
  disabled,
  title,
  type = "button",
}: {
  children?: ReactNode;
  onClick?: () => void;
  variant?: "primary" | "ghost" | "danger";
  small?: boolean;
  icon?: IconName;
  disabled?: boolean;
  title?: string;
  type?: "button" | "submit";
}) {
  const cls = ["btn", variant, small && "small", icon && !children && "icon"]
    .filter(Boolean)
    .join(" ");
  return (
    <button
      type={type}
      className={cls}
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={title}
    >
      {icon && <Icon name={icon} size={small ? 14 : 16} />}
      {children}
    </button>
  );
}

/** Two-step button for destructive actions (no blocking confirm dialogs). */
export function ConfirmButton({
  children,
  onConfirm,
  icon,
  small,
  title,
}: {
  children?: ReactNode;
  onConfirm: () => void;
  icon?: IconName;
  small?: boolean;
  title?: string;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => {
      setArmed(false);
    }, 3000);
    return () => {
      clearTimeout(t);
    };
  }, [armed]);
  return (
    <button
      type="button"
      className={[
        "btn",
        "danger",
        small && "small",
        armed && "armed",
        icon && !children && !armed && "icon",
      ]
        .filter(Boolean)
        .join(" ")}
      title={armed ? "Click again to confirm" : title}
      aria-label={title}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else setArmed(true);
      }}
    >
      {icon && <Icon name={icon} size={small ? 14 : 16} />}
      {armed ? "Confirm" : children}
    </button>
  );
}

export function Toggle({
  value,
  onChange,
  disabled,
  label,
}: {
  value: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={value}
      aria-label={label}
      disabled={disabled}
      className={`toggle${value ? " on" : ""}`}
      onClick={() => {
        onChange(!value);
      }}
    />
  );
}

/** Checkbox; `children` is its clickable label. Styled in styles/audio.css. */
export function Checkbox({
  checked,
  onChange,
  disabled,
  title,
  children,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  title?: string;
  children?: ReactNode;
}) {
  return (
    <label className={`checkbox${disabled ? " disabled" : ""}`} title={title}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => {
          onChange(e.target.checked);
        }}
      />
      <span className="box" aria-hidden="true">
        <Icon name="check" size={12} />
      </span>
      {children}
    </label>
  );
}

export interface Option<T extends string | number> {
  value: T;
  label: string;
  disabled?: boolean;
}

export function Select<T extends string | number>({
  value,
  options,
  onChange,
  disabled,
  width,
}: {
  value: T;
  options: Option<T>[];
  onChange: (v: T) => void;
  disabled?: boolean;
  width?: number;
}) {
  const idx = options.findIndex((o) => o.value === value);
  return (
    <select
      className="select"
      value={idx < 0 ? "" : String(idx)}
      disabled={disabled}
      style={width ? { width } : undefined}
      onChange={(e) => {
        const o = options[Number(e.target.value)];
        if (o) onChange(o.value);
      }}
    >
      {idx < 0 && <option value="">—</option>}
      {options.map((o, i) => (
        <option key={String(o.value)} value={String(i)} disabled={o.disabled}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: Option<T>[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup">
      {options.map((o) => (
        <button
          type="button"
          key={String(o.value)}
          role="radio"
          aria-checked={o.value === value}
          className={o.value === value ? "on" : ""}
          disabled={o.disabled}
          onClick={() => {
            onChange(o.value);
          }}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  onCommit,
  onDoubleClick,
  title,
  width,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  /** Called when the user releases the slider (for undo history). */
  onCommit?: () => void;
  /** E.g. reset to the default value. */
  onDoubleClick?: () => void;
  title?: string;
  width?: number;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <input
      type="range"
      className="slider"
      min={min}
      max={max}
      step={step}
      value={value}
      title={title}
      onDoubleClick={onDoubleClick}
      style={{ "--pct": `${pct}%`, ...(width ? { width } : {}) } as React.CSSProperties}
      onChange={(e) => {
        onChange(Number(e.target.value));
      }}
      onPointerUp={onCommit}
      onKeyUp={onCommit}
    />
  );
}

/** Numeric input that commits on blur/Enter and rejects out-of-range values. */
export function NumberInput({
  value,
  min,
  max,
  step = 1,
  onChange,
  suffix,
  width,
  decimals = 0,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  suffix?: string;
  width?: number;
  decimals?: number;
}) {
  // `draft` is only set while the user is typing; otherwise the prop is shown.
  const [draft, setDraft] = useState<string | null>(null);
  const invalid = draft !== null && !isValid(draft);
  function isValid(t: string) {
    const v = Number(t.replace(",", "."));
    return t.trim() !== "" && Number.isFinite(v) && v >= min && v <= max;
  }
  const commit = () => {
    if (draft === null || !isValid(draft)) return;
    const v = Number(draft.replace(",", "."));
    const rounded = Math.round(v / step) * step;
    setDraft(null);
    onChange(Number(rounded.toFixed(decimals)));
  };
  return (
    <span className="row" style={{ gap: 4 }}>
      <input
        className={`input num${invalid ? " invalid" : ""}`}
        style={width ? { width } : undefined}
        value={draft ?? value.toFixed(decimals)}
        inputMode="decimal"
        title={`${min}–${max}`}
        onBlur={commit}
        onChange={(e) => {
          setDraft(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setDraft(null);
          if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            e.preventDefault();
            const v = Math.min(max, Math.max(min, value + (e.key === "ArrowUp" ? step : -step)));
            onChange(Number(v.toFixed(decimals)));
          }
        }}
      />
      {suffix && <span className="muted small">{suffix}</span>}
    </span>
  );
}

export function TextInput({
  value,
  onCommit,
  placeholder,
  width,
  mono,
}: {
  value: string;
  onCommit: (v: string) => void;
  placeholder?: string;
  width?: number | string;
  mono?: boolean;
}) {
  // `draft` is only set while editing; otherwise the prop is shown.
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft !== null && draft !== value) onCommit(draft);
    setDraft(null);
  };
  return (
    <input
      className={`input${mono ? " mono" : ""}`}
      style={{ width: width ?? 280 }}
      value={draft ?? value}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => {
        setDraft(e.target.value);
      }}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setDraft(null);
      }}
    />
  );
}

export function Section({
  title,
  description,
  keywords,
  children,
}: {
  title: string;
  description?: string;
  /** Extra words the settings search should find this section by. */
  keywords?: string;
  children: ReactNode;
}) {
  const s = useSearchScope(`${title}\n${description ?? ""}\n${keywords ?? ""}`);
  return (
    <section className="card section" hidden={!s.visible}>
      <header>
        <h2>{highlight(title, s.terms)}</h2>
        {description && <p>{highlight(description, s.terms)}</p>}
      </header>
      <div className="body">
        <SettingsSearchContext value={s.value}>{children}</SettingsSearchContext>
      </div>
    </section>
  );
}

/**
 * A titled block without card chrome (e.g. content that brings its own cards) that
 * the settings search treats like a `Section`.
 */
export function SearchBlock({
  title,
  description,
  keywords,
  children,
}: {
  title: string;
  description?: ReactNode;
  keywords?: string;
  children: ReactNode;
}) {
  const s = useSearchScope(`${title}\n${nodeText(description)}\n${keywords ?? ""}`);
  return (
    <div className="search-block" hidden={!s.visible}>
      <header>
        <h2>{highlight(title, s.terms)}</h2>
        {description && <p>{highlight(description, s.terms)}</p>}
      </header>
      <SettingsSearchContext value={s.value}>{children}</SettingsSearchContext>
    </div>
  );
}

export function Field({
  label,
  description,
  keywords,
  children,
  error,
}: {
  label: string;
  description?: ReactNode;
  /** Extra words the settings search should find this row by (e.g. for custom controls). */
  keywords?: string;
  children: ReactNode;
  error?: boolean;
}) {
  const { hidden, terms } = useSearchRow(`${label}\n${nodeText(description)}\n${keywords ?? ""}`);
  return (
    <div className={`field${error ? " error" : ""}`} hidden={hidden}>
      <div className="label">
        <span>{highlight(label, terms)}</span>
        {description && <span className="desc">{highlight(description, terms)}</span>}
      </div>
      <div className="control">{children}</div>
    </div>
  );
}
export function Badge({
  children,
  tone,
}: {
  children: ReactNode;
  tone?: "ok" | "warn" | "err" | "accent";
}) {
  return <span className={`badge${tone ? ` ${tone}` : ""}`}>{children}</span>;
}

export function Keys({ accelerator }: { accelerator: string }) {
  return (
    <span className="kbd">
      {accelerator.split("+").map((k) => (
        <kbd key={k}>{k}</kbd>
      ))}
    </span>
  );
}

/** Level meter: `level` and `peak` are linear 0..1. */
export function Meter({ level, peak, width }: { level: number; peak: number; width?: number }) {
  const toPct = (v: number) => {
    // -60 dB .. 0 dB → 0..100 %
    const db = v <= 0.000_01 ? -100 : 20 * Math.log10(v);
    return Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
  };
  return (
    <div className="meter" style={width ? { width } : undefined}>
      <div className="fill" style={{ width: `${toPct(level)}%` }} />
      <div className="peak" style={{ left: `calc(${toPct(peak)}% - 2px)` }} />
    </div>
  );
}

export function Spinner() {
  return <span className="spinner" aria-label="Loading" />;
}

export function Progress({ value }: { value: number }) {
  return (
    <div
      className="progress"
      role="progressbar"
      aria-valuenow={Math.round(value)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div style={{ width: `${Math.max(0, Math.min(100, value))}%` }} />
    </div>
  );
}

const SEVERITY_GLYPH = {
  info: "glyphInfo",
  warning: "glyphWarning",
  error: "glyphError",
  success: "glyphSuccess",
} as const satisfies Record<string, IconName>;

/** Filled round status icon, as in Fluent InfoBars. */
export function SeverityIcon({ severity }: { severity: keyof typeof SEVERITY_GLYPH }) {
  return (
    <span className={`sev ${severity}`}>
      <Icon name={SEVERITY_GLYPH[severity]} size={10} />
    </span>
  );
}

/** Inline InfoBar. */
export function Banner({
  tone = "warn",
  children,
}: {
  tone?: "warn" | "error" | "info";
  children: ReactNode;
}) {
  return (
    <div className={`banner${tone === "warn" ? "" : ` ${tone}`}`}>
      <SeverityIcon severity={tone === "warn" ? "warning" : tone} />
      <div>{children}</div>
    </div>
  );
}
