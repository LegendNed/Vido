/**
 * Search over the settings UI. The Settings page provides `SettingsSearchContext`;
 * `Field`, `Section` and `SearchBlock` (components/ui.tsx) read it to hide themselves
 * when they don't match and to highlight the matched words. Outside the Settings page
 * there is no context and they render normally, so any page built from those
 * components is searchable without extra work.
 *
 * A row matches when every word of the query appears in its own text (label,
 * description, keywords) or in the text of a group/section around it - so "audio
 * codec" finds Codec under Audio format, and a section whose title matches shows
 * all of its rows. Containers stay hidden unless they match or a row inside does;
 * they stay mounted (only `hidden`) so their rows can report matches.
 */
import {
  cloneElement,
  createContext,
  Fragment,
  isValidElement,
  useContext,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

/** Counts matching rows below a container; `add` returns its undo. */
interface MatchCounter {
  add: () => () => void;
}

export interface SettingsSearch {
  /** Lower-case query words; empty = not searching. */
  terms: string[];
  /** Lower-case text of the enclosing groups/sections. */
  inherited: string;
  counter: MatchCounter | null;
}

export const SettingsSearchContext = createContext<SettingsSearch | null>(null);

export function searchTerms(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** Plain text of a React node (strings inside host elements and fragments). */
export function nodeText(n: ReactNode): string {
  if (n === null || n === undefined || typeof n === "boolean") return "";
  if (typeof n === "string" || typeof n === "number") return String(n);
  if (Array.isArray(n)) return n.map((c: ReactNode) => nodeText(c)).join("");
  if (isValidElement(n)) return nodeText((n.props as { children?: ReactNode }).children);
  return "";
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function termsRegExp(terms: string[]): RegExp | null {
  if (terms.length === 0) return null;
  const alts = [...terms].sort((a, b) => b.length - a.length).map(escapeRegExp);
  return new RegExp(`(${alts.join("|")})`, "gi");
}

function markText(text: string, re: RegExp): ReactNode {
  const parts = text.split(re);
  if (parts.length === 1) return text;
  // With a capture group, odd parts are the matches.
  return parts.map((p, i) =>
    i % 2 === 1 ? (
      <mark key={i} className="search-hit">
        {p}
      </mark>
    ) : (
      p
    ),
  );
}

/** Wraps matched words in `<mark>`; descends into host elements and fragments only. */
export function highlight(n: ReactNode, terms: string[]): ReactNode {
  const re = termsRegExp(terms);
  if (!re) return n;
  const walk = (node: ReactNode): ReactNode => {
    if (typeof node === "string") return markText(node, re);
    if (Array.isArray(node)) {
      return node.map((c: ReactNode, i) => <Fragment key={i}>{walk(c)}</Fragment>);
    }
    if (isValidElement(node) && (typeof node.type === "string" || node.type === Fragment)) {
      const children = (node.props as { children?: ReactNode }).children;
      if (children === undefined) return node;
      return cloneElement(node, undefined, walk(children));
    }
    return node;
  };
  return walk(n);
}

function useReport(counter: MatchCounter | null, on: boolean) {
  useLayoutEffect(() => (on && counter ? counter.add() : undefined), [on, counter]);
}

/** For a single row (`Field`): whether it is shown, and the terms to highlight. */
export function useSearchRow(text: string): { hidden: boolean; terms: string[] } {
  const ctx = useContext(SettingsSearchContext);
  const terms = ctx?.terms ?? [];
  const own = text.toLowerCase();
  const matched = ctx !== null && terms.every((t) => own.includes(t) || ctx.inherited.includes(t));
  const searching = terms.length > 0;
  useReport(ctx?.counter ?? null, searching && matched);
  return { hidden: searching && !matched, terms };
}

/**
 * For a container (`Section`, `SearchBlock`, a Settings group): shown when its own
 * text matches the whole query or any row inside matches. Provide `value` to the
 * children through `SettingsSearchContext`.
 */
export function useSearchScope(text: string): {
  searching: boolean;
  /** The container itself matches: everything inside is shown. */
  all: boolean;
  visible: boolean;
  terms: string[];
  value: SettingsSearch | null;
} {
  const ctx = useContext(SettingsSearchContext);
  const parent = ctx?.counter ?? null;
  const [count, setCount] = useState(0);
  const counter = useMemo<MatchCounter>(
    () => ({
      add: () => {
        setCount((c) => c + 1);
        const undo = parent?.add();
        return () => {
          setCount((c) => c - 1);
          undo?.();
        };
      },
    }),
    [parent],
  );
  const terms = ctx?.terms ?? [];
  const searching = terms.length > 0;
  const inherited = ctx ? `${ctx.inherited}\n${text.toLowerCase()}` : "";
  const all = searching && terms.every((t) => inherited.includes(t));
  useReport(parent, all);
  const value = useMemo(
    () => (ctx ? { terms: ctx.terms, inherited, counter } : null),
    [ctx, inherited, counter],
  );
  return { searching, all, visible: !searching || all || count > 0, terms, value };
}
