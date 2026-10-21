import { Search, X } from "lucide-react";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { Input } from "../../components/ui/misc";
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
      <h2
        id={`${groupDomId(id)}-title`}
        className="settings-group-title mb-4 text-2xl font-semibold tracking-tight"
      >
        {highlight(label, s.terms)}
      </h2>
      <div className="settings-group-body">
        <SettingsSearchContext value={s.value}>{children}</SettingsSearchContext>
      </div>
    </section>
  );
}

/**
 * Settings: one page at a time, picked in the sidebar (`target`; the parent remounts
 * this per request). Searching shows every matching option from all pages at once
 * (by title and description).
 */
export function SettingsPage({ target }: { target: SettingsTarget | null }) {
  const [query, setQuery] = useState("");
  const selected = target?.group ?? SETTINGS_GROUPS[0].id;
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
    <div className={`settings flex min-h-0 flex-1 flex-col${searching ? " searching" : ""}`}>
      <div className="settings-scroll min-h-0 flex-1 overflow-y-auto" ref={scrollRef}>
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-6 px-6 pb-12 pt-6">
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              ref={inputRef}
              className="pl-9 pr-9"
              value={query}
              placeholder="Search settings (Ctrl+F)"
              aria-label="Search settings"
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
                className="absolute right-2 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"
                title="Clear"
                aria-label="Clear search"
                onClick={clear}
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
          <SettingsSearchContext value={root}>
            {groups.map((g) => (
              <Group key={g.id} id={g.id} label={g.label} onVisible={onVisible}>
                {CONTENT[g.id]()}
              </Group>
            ))}
          </SettingsSearchContext>
          {searching && matches.length === 0 && (
            <div className="py-16 text-center">
              <h3 className="font-medium">No settings match “{query.trim()}”</h3>
              <p className="mt-1 text-sm text-muted-foreground">Try fewer or different words.</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
