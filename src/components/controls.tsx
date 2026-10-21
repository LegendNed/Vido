// The app's form controls, built on shadcn/ui (`./ui`). Screens use these small
// wrappers (value + onChange, settings-search aware) rather than the primitives.

import { CircleAlert, CircleCheck, CircleX, Info, LoaderCircle } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import {
  highlight,
  nodeText,
  SettingsSearchContext,
  useSearchRow,
  useSearchScope,
} from "../lib/settingsSearch";
import { cn } from "../lib/utils";
import { Icon, type IconName } from "./Icon";
import { Button as UiButton } from "./ui/button";
import { Checkbox as UiCheckbox } from "./ui/checkbox";
import {
  Alert,
  AlertDescription,
  Badge as UiBadge,
  Input,
  Kbd,
  Progress as UiProgress,
  ToggleGroup,
  ToggleGroupItem,
} from "./ui/misc";
import {
  Select as UiSelect,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import { Slider as UiSlider } from "./ui/slider";
import { Switch } from "./ui/switch";

export function Button({
  children,
  onClick,
  variant,
  small,
  icon,
  disabled,
  title,
  type = "button",
  className,
}: {
  children?: ReactNode;
  onClick?: () => void;
  variant?: "primary" | "ghost" | "danger";
  small?: boolean;
  icon?: IconName;
  disabled?: boolean;
  title?: string;
  type?: "button" | "submit";
  className?: string;
}) {
  const iconOnly = icon !== undefined && children === undefined;
  return (
    <UiButton
      type={type}
      variant={
        variant === "primary"
          ? "default"
          : variant === "ghost"
            ? "ghost"
            : variant === "danger"
              ? "destructive"
              : "outline"
      }
      size={iconOnly ? (small ? "icon-sm" : "icon") : small ? "sm" : "default"}
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={title}
      className={className}
    >
      {icon && <Icon name={icon} size={small ? 14 : 16} />}
      {children}
    </UiButton>
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
  const iconOnly = icon !== undefined && children === undefined && !armed;
  return (
    <UiButton
      type="button"
      variant={armed ? "destructive" : "ghost"}
      size={iconOnly ? (small ? "icon-sm" : "icon") : small ? "sm" : "default"}
      className={armed ? undefined : "text-destructive hover:text-destructive"}
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
    </UiButton>
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
    <Switch checked={value} onCheckedChange={onChange} disabled={disabled} aria-label={label} />
  );
}

/** Checkbox; `children` is its clickable label. */
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
    <label
      className={cn(
        "checkbox inline-flex cursor-pointer items-center gap-2 text-sm",
        disabled && "disabled cursor-not-allowed opacity-50",
      )}
      title={title}
    >
      <UiCheckbox
        checked={checked}
        disabled={disabled}
        onCheckedChange={(v) => {
          onChange(v === true);
        }}
      />
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
  id,
}: {
  value: T;
  options: Option<T>[];
  onChange: (v: T) => void;
  disabled?: boolean;
  width?: number;
  id?: string;
}) {
  // Radix wants string values: options are addressed by index.
  const idx = options.findIndex((o) => o.value === value);
  return (
    <UiSelect
      value={idx < 0 ? "" : String(idx)}
      disabled={disabled}
      onValueChange={(v) => {
        const o = options[Number(v)];
        if (o) onChange(o.value);
      }}
    >
      <SelectTrigger
        id={id}
        size="sm"
        className="min-w-[8rem] justify-between"
        style={width ? { width } : undefined}
      >
        <SelectValue placeholder="-" />
      </SelectTrigger>
      <SelectContent>
        {options.map((o, i) => (
          <SelectItem key={String(o.value)} value={String(i)} disabled={o.disabled}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </UiSelect>
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
  const idx = options.findIndex((o) => o.value === value);
  return (
    <ToggleGroup
      type="single"
      value={idx < 0 ? "" : String(idx)}
      onValueChange={(v) => {
        // Clicking the selected item deselects it in Radix: keep the selection.
        const o = v === "" ? undefined : options[Number(v)];
        if (o) onChange(o.value);
      }}
    >
      {options.map((o, i) => (
        <ToggleGroupItem key={String(o.value)} value={String(i)} disabled={o.disabled}>
          {o.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
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
  return (
    <UiSlider
      className="slider"
      value={[value]}
      min={min}
      max={max}
      step={step}
      title={title}
      // No width: fills its container (shadcn's `w-full`), e.g. a player's seek bar.
      style={width ? { width } : undefined}
      onDoubleClick={onDoubleClick}
      onValueChange={(v) => {
        const n = v[0];
        if (n !== undefined) onChange(n);
      }}
      onValueCommit={onCommit}
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
    <span className="inline-flex items-center gap-2">
      <Input
        className="h-8 text-right tabular-nums"
        style={{ width: width ?? 80 }}
        aria-invalid={invalid || undefined}
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
      {suffix && <span className="text-xs text-muted-foreground">{suffix}</span>}
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
    <Input
      className={cn("h-8", mono && "font-mono text-xs")}
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

/** A settings card: title, description and rows (`Field`s). */
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
    <section
      className="section rounded-xl border bg-card text-card-foreground shadow-xs"
      hidden={!s.visible}
    >
      <header className="px-5 pb-1 pt-4">
        <h2 className="text-sm font-semibold leading-none">{highlight(title, s.terms)}</h2>
        {description && (
          <p className="mt-1.5 text-sm text-muted-foreground">{highlight(description, s.terms)}</p>
        )}
      </header>
      <div className="body px-5 pb-1">
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
    <div className="search-block flex flex-col gap-3" hidden={!s.visible}>
      <header>
        <h2 className="text-sm font-semibold">{highlight(title, s.terms)}</h2>
        {description && (
          <p className="mt-1 text-sm text-muted-foreground">{highlight(description, s.terms)}</p>
        )}
      </header>
      <SettingsSearchContext value={s.value}>{children}</SettingsSearchContext>
    </div>
  );
}

/** One settings row: label + description on the left, the control on the right. */
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
    <div className={cn("field", error && "error")} hidden={hidden}>
      <div className="label">
        <span className="text-sm font-medium">{highlight(label, terms)}</span>
        {description && (
          <span
            className={cn("desc text-xs", error ? "text-destructive" : "text-muted-foreground")}
          >
            {highlight(description, terms)}
          </span>
        )}
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
  const variant =
    tone === "ok"
      ? "success"
      : tone === "warn"
        ? "warning"
        : tone === "err"
          ? "destructive"
          : tone === "accent"
            ? "default"
            : "secondary";
  return <UiBadge variant={variant}>{children}</UiBadge>;
}

export function Keys({ accelerator }: { accelerator: string }) {
  return (
    <span className="inline-flex items-center gap-1">
      {accelerator.split("+").map((k) => (
        <Kbd key={k}>{k}</Kbd>
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
    <div
      className="meter relative h-1.5 overflow-hidden rounded-full bg-muted"
      style={{ width: width ?? 120 }}
    >
      <div
        className="absolute inset-y-0 left-0 rounded-full bg-success transition-[width] duration-75"
        style={{ width: `${toPct(level)}%` }}
      />
      <div
        className="absolute inset-y-0 w-0.5 bg-foreground/70"
        style={{ left: `calc(${toPct(peak)}% - 1px)` }}
      />
    </div>
  );
}

export function Spinner() {
  return (
    <LoaderCircle
      className="spinner size-5 animate-spin text-muted-foreground"
      aria-label="Loading"
    />
  );
}

export function Progress({ value }: { value: number }) {
  return (
    <UiProgress
      className="progress h-1.5"
      value={Math.max(0, Math.min(100, value))}
      aria-valuenow={Math.round(value)}
    />
  );
}

/** Round status icon (info / warning / error / success). */
export function SeverityIcon({ severity }: { severity: "info" | "warning" | "error" | "success" }) {
  const cls = "size-4 shrink-0";
  switch (severity) {
    case "success":
      return <CircleCheck className={cn(cls, "text-success")} />;
    case "warning":
      return <CircleAlert className={cn(cls, "text-warning")} />;
    case "error":
      return <CircleX className={cn(cls, "text-destructive")} />;
    default:
      return <Info className={cn(cls, "text-info")} />;
  }
}

/** Inline alert. */
export function Banner({
  tone = "warn",
  children,
}: {
  tone?: "warn" | "error" | "info";
  children: ReactNode;
}) {
  return (
    <Alert
      variant={tone === "error" ? "destructive" : tone === "warn" ? "warning" : "default"}
      className="banner"
    >
      <SeverityIcon severity={tone === "warn" ? "warning" : tone} />
      <AlertDescription className="text-foreground/90">{children}</AlertDescription>
    </Alert>
  );
}
