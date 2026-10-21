import { useApp } from "../stores/app";
import { Icon } from "./Icon";
import { Button as UiButton } from "./ui/button";
import { Button, SeverityIcon } from "./controls";

export function Notices() {
  const notices = useApp((s) => s.notices);
  const dismiss = useApp((s) => s.dismiss);
  return (
    <div className="notices" aria-live="polite">
      {notices.map((n) => (
        <div key={n.id} className={`notice ${n.severity}`}>
          <SeverityIcon severity={n.severity} />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div className="title">{n.title}</div>
            {n.hint && <div className="hint">{n.hint}</div>}
            {n.actions && (
              <div className="row" style={{ gap: 4, marginTop: 6 }}>
                {n.actions.map((a) => (
                  <Button
                    key={a.label}
                    small
                    onClick={() => {
                      a.run();
                      dismiss(n.id);
                    }}
                  >
                    {a.label}
                  </Button>
                ))}
              </div>
            )}
          </div>
          <UiButton
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Dismiss"
            onClick={() => {
              dismiss(n.id);
            }}
          >
            <Icon name="x" size={14} />
          </UiButton>
        </div>
      ))}
    </div>
  );
}
