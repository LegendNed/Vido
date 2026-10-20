import {
  Fragment,
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
import { AudioSettings } from "../audio/AudioPage";
import { CaptureSettings } from "../capture/CapturePage";
import { ExportDefaultsPage } from "../export/ExportDefaultsPage";
import { HotkeysPage } from "../hotkeys/HotkeysPage";
import { OverlayPage } from "../overlay/OverlayPage";
import { AdvancedPage } from "./AdvancedPage";
import { GeneralPage } from "./GeneralPage";
import { SETTINGS_GROUPS, type SettingsGroupId, type SettingsTarget } from "./groups";
import { SavingPage } from "./SavingPage";

const CONTENT: Record<SettingsGroupId, () => ReactNode> = {
  video: () => <CaptureSettings />,
  audio: () => <AudioSettings />,
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
  onVisible,
  children,
}: {
  id: SettingsGroupId;
  label: string;
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
      <h2 id={`${groupDomId(id)}-title`} className="settings-group-title">
        {highlight(label, s.terms)}
      </h2>
      <div className="settings-group-body">
        <SettingsSearchContext value={s.value}>{children}</SettingsSearchContext>
      </div>
    </section>
  );
}

/**
 * Settings: a categorized list on the left shows one page at a time (`target`, from
 * links elsewhere, picks the first one; the parent remounts the page per request). Searching
 * shows every matching option from all pages at once (by title and description).
 */
export function SettingsPage({ target }: { target: SettingsTarget | null }) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<SettingsGroupId>(
    () => target?.group ?? SETTINGS_GROUPS[0].id,
  );
  const [hidden, setHidden] = useState<ReadonlySet<SettingsGroupId>>(() => new Set());
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const terms = useMemo(() => searchTerms(query), [query]);
  const searching = terms.length > 0;
  const root = useMemo<SettingsSearch>(() => ({ terms, inherited: "", counter: null }), [terms]);
  const groups = searching ? SETTINGS_GROUPS : SETTINGS_GROUPS.filter((g) => g.id === selected);
  const matches = SETTINGS_GROUPS.filter((g) => !hidden.has(g.id));

  const onVisible = useCallback((id: SettingsGroupId, visible: boolean) => {
    setHidden((prev) => {
      if (prev.has(id) === !visible) return prev;
      const next = new Set(prev);
      if (visible) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const open = useCallback((id: SettingsGroupId) => {
    setQuery("");
    setSelected(id);
    scrollRef.current?.scrollTo({ top: 0 });
  }, []);

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
        <nav className="settings-nav" aria-label="Settings pages">
          {SETTINGS_GROUPS.map((g, i) => {
            const heading = i === 0 || SETTINGS_GROUPS[i - 1]?.category !== g.category;
            const dim = searching && hidden.has(g.id);
            const active = !searching && selected === g.id;
            return (
              <Fragment key={g.id}>
                {heading && <div className="settings-nav-heading">{g.category}</div>}
                <button
                  type="button"
                  className={`settings-nav-item${active ? " active" : ""}${dim ? " dim" : ""}`}
                  aria-current={active ? "page" : undefined}
                  onClick={() => {
                    open(g.id);
                  }}
                >
                  {g.label}
                </button>
              </Fragment>
            );
          })}
        </nav>

        <div className="settings-scroll" ref={scrollRef}>
          <SettingsSearchContext value={root}>
            {groups.map((g) => (
              <Group key={g.id} id={g.id} label={g.label} onVisible={onVisible}>
                {CONTENT[g.id]()}
              </Group>
            ))}
          </SettingsSearchContext>
          {searching && matches.length === 0 && (
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
