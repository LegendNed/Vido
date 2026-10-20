import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { Icon } from "../../components/Icon";
import {
  highlight,
  searchTerms,
  SettingsSearchContext,
  useSearchScope,
  type SettingsSearch,
} from "../../lib/settingsSearch";
import { ExportDefaultsPage } from "../export/ExportDefaultsPage";
import { HotkeysPage } from "../hotkeys/HotkeysPage";
import { OverlayPage } from "../overlay/OverlayPage";
import { RecordingPage } from "../recording/RecordingPage";
import { AdvancedPage } from "./AdvancedPage";
import { GeneralPage } from "./GeneralPage";
import { SETTINGS_GROUPS, type SettingsGroupId, type SettingsTarget } from "./groups";
import { SavingPage } from "./SavingPage";

const CONTENT: Record<SettingsGroupId, () => ReactNode> = {
  recording: () => <RecordingPage />,
  hotkeys: () => <HotkeysPage />,
  overlay: () => <OverlayPage />,
  saving: () => <SavingPage />,
  export: () => <ExportDefaultsPage />,
  general: () => <GeneralPage />,
  advanced: () => <AdvancedPage />,
};

const groupDomId = (id: SettingsGroupId) => `settings-${id}`;

/**
 * One group: a heading plus an existing page rendered as-is (its own page heading is
 * hidden by CSS). Hidden while searching unless something inside matches.
 */
function Group({
  id,
  label,
  flash,
  onVisible,
  children,
}: {
  id: SettingsGroupId;
  label: string;
  /** Changes each time the group is jumped to (0 = never). */
  flash: number;
  onVisible: (id: SettingsGroupId, visible: boolean) => void;
  children: ReactNode;
}) {
  const s = useSearchScope(label);
  useLayoutEffect(() => {
    onVisible(id, s.visible);
  }, [id, s.visible, onVisible]);
  return (
    <section
      id={groupDomId(id)}
      className="settings-group"
      hidden={!s.visible}
      data-all={s.all || undefined}
      aria-labelledby={`${groupDomId(id)}-title`}
    >
      <h2
        key={flash}
        id={`${groupDomId(id)}-title`}
        className={`settings-group-title${flash ? " flash" : ""}`}
      >
        {highlight(label, s.terms)}
      </h2>
      <SettingsSearchContext value={s.value}>{children}</SettingsSearchContext>
    </section>
  );
}

/**
 * Every setting on one scrollable page, grouped, with a group list to jump around
 * and a search box that filters rows by their title and description.
 */
export function SettingsPage({ target }: { target: SettingsTarget | null }) {
  const [query, setQuery] = useState("");
  const [hidden, setHidden] = useState<ReadonlySet<SettingsGroupId>>(() => new Set());
  const [active, setActive] = useState<SettingsGroupId>(SETTINGS_GROUPS[0].id);
  const [flash, setFlash] = useState<{ id: SettingsGroupId; n: number } | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const terms = useMemo(() => searchTerms(query), [query]);
  const searching = terms.length > 0;
  const root = useMemo<SettingsSearch>(() => ({ terms, inherited: "", counter: null }), [terms]);
  const shown = SETTINGS_GROUPS.filter((g) => !hidden.has(g.id));
  // While searching the scrolled-to group may be filtered out: mark the first one shown.
  const current = shown.some((g) => g.id === active) ? active : shown[0]?.id;

  const onVisible = useCallback((id: SettingsGroupId, visible: boolean) => {
    setHidden((prev) => {
      if (prev.has(id) === !visible) return prev;
      const next = new Set(prev);
      if (visible) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const jump = useCallback((id: SettingsGroupId, smooth = true) => {
    const scroller = scrollRef.current;
    const el = document.getElementById(groupDomId(id));
    if (!scroller || !el) return;
    const top =
      el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop;
    scroller.scrollTo({ top: Math.max(0, top - 4), behavior: smooth ? "smooth" : "instant" });
    setActive(id);
    setFlash((f) => ({ id, n: (f?.n ?? 0) + 1 }));
  }, []);

  // Navigation from elsewhere (tray, old page ids): clear the search, then scroll.
  useEffect(() => {
    if (!target) return;
    let frame = requestAnimationFrame(() => {
      setQuery("");
      frame = requestAnimationFrame(() => {
        jump(target.group, false);
      });
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [target, jump]);

  // Ctrl+F focuses the search box while this page is open.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  // The group list follows the scroll position.
  const onScroll = () => {
    const scroller = scrollRef.current;
    if (!scroller) return;
    const edge = scroller.getBoundingClientRect().top + 48;
    let current: SettingsGroupId | null = null;
    for (const g of SETTINGS_GROUPS) {
      const el = document.getElementById(groupDomId(g.id));
      if (!el || el.hidden) continue;
      if (el.getBoundingClientRect().top <= edge || current === null) current = g.id;
    }
    // At the very bottom the last group counts as current even if it is short.
    if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2) {
      const last = [...SETTINGS_GROUPS].reverse().find((g) => !hidden.has(g.id));
      if (last) current = last.id;
    }
    if (current) setActive(current);
  };

  const clear = () => {
    setQuery("");
    inputRef.current?.focus();
  };

  return (
    <div className={`settings${searching ? " searching" : ""}`}>
      <header className="settings-head">
        <h1>Settings</h1>
        <div className={`search-box${query ? " has-text" : ""}`}>
          <input
            ref={inputRef}
            className="input"
            value={query}
            placeholder="Find a setting"
            aria-label="Find a setting"
            spellCheck={false}
            onChange={(e) => {
              setQuery(e.target.value);
              scrollRef.current?.scrollTo({ top: 0 });
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                if (query) {
                  e.preventDefault();
                  setQuery("");
                } else inputRef.current?.blur();
              }
            }}
          />
          {query && (
            <button
              type="button"
              className="search-clear"
              title="Clear"
              aria-label="Clear search"
              onClick={clear}
            >
              <Icon name="x" size={12} />
            </button>
          )}
          <span className="search-glyph" aria-hidden="true">
            <Icon name="search" size={14} />
          </span>
        </div>
      </header>

      <div className="settings-main">
        <nav className="settings-nav" aria-label="Settings groups">
          {shown.map((g) => (
            <button
              key={g.id}
              type="button"
              className={`settings-nav-item${current === g.id ? " active" : ""}`}
              aria-current={current === g.id ? "true" : undefined}
              onClick={() => {
                jump(g.id);
              }}
            >
              {g.label}
            </button>
          ))}
        </nav>

        <div className="settings-scroll" ref={scrollRef} onScroll={onScroll}>
          <SettingsSearchContext value={root}>
            {SETTINGS_GROUPS.map((g) => (
              <Group
                key={g.id}
                id={g.id}
                label={g.label}
                flash={flash?.id === g.id ? flash.n : 0}
                onVisible={onVisible}
              >
                {CONTENT[g.id]()}
              </Group>
            ))}
          </SettingsSearchContext>
          {searching && shown.length === 0 && (
            <div className="empty settings-empty">
              <h3>No settings match “{query.trim()}”</h3>
              <p>Try fewer or different words.</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
