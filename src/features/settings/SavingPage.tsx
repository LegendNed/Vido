import { Badge, Button, Field, Section, Segmented, TextInput, Toggle } from "../../components/ui";
import { commands } from "../../lib/tauri/commands";
import { errorMessage, type Container, type SaveMode } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

function previewName(template: string): string {
  const now = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`;
  const time = `${p(now.getHours())}-${p(now.getMinutes())}-${p(now.getSeconds())}`;
  return template
    .replaceAll("{date}", date)
    .replaceAll("{time}", time)
    .replaceAll("{source}", "Game")
    .replaceAll("{duration}", "30s");
}

export function SavingPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const info = useApp((s) => s.info);
  const notify = useApp((s) => s.notify);
  const fieldError = useApp((s) => s.fieldError);
  if (!settings) return null;
  const sv = settings.saving;
  const err = (f: string) => fieldError?.field?.startsWith(f) ?? false;

  const browse = async () => {
    try {
      const dir = await commands.pickFolder(info?.outputDir);
      if (dir) {
        update(
          (d) => {
            d.saving.outputDir = dir;
          },
          { immediate: true },
        );
      }
    } catch (e) {
      notify("error", "Could not open folder picker", errorMessage(e));
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Saving</h1>
      </div>
      <Section title="Clips folder">
        <Field
          label="Output folder"
          description={<span className="mono">{info?.outputDir}</span>}
          error={err("saving.outputDir")}
        >
          <Button small icon="folder" onClick={() => void browse()}>
            Change…
          </Button>
          {sv.outputDir !== null && (
            <Button
              small
              variant="ghost"
              onClick={() => {
                update(
                  (d) => {
                    d.saving.outputDir = null;
                  },
                  { immediate: true },
                );
              }}
            >
              Use default
            </Button>
          )}
          <Button
            small
            variant="ghost"
            icon="external"
            title="Open folder"
            onClick={() => void commands.openFolder("output")}
          />
        </Field>
        <Field
          label="File name"
          description={
            <>
              Tokens: {"{date} {time} {source} {duration}"} · Preview:{" "}
              <span className="mono">
                {previewName(sv.filenameTemplate)}.{sv.container}
              </span>
            </>
          }
          error={err("saving.filenameTemplate")}
        >
          <TextInput
            value={sv.filenameTemplate}
            mono
            onCommit={(v) => {
              update(
                (d) => {
                  d.saving.filenameTemplate = v;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <Field
          label="Container"
          description="MP4 plays everywhere. MKV survives crashes mid-write."
        >
          <Segmented<Container>
            value={sv.container}
            options={[
              { value: "mp4", label: "MP4" },
              { value: "mkv", label: "MKV" },
            ]}
            onChange={(v) => {
              update((d) => {
                d.saving.container = v;
              });
            }}
          />
        </Field>
        <Field
          label="When saving"
          description={
            sv.saveMode === "remux"
              ? "Instant: buffered video is copied as-is (no quality loss, no CPU/GPU cost)."
              : "Saves instantly, then re-encodes in the background with your Export defaults (smaller files)."
          }
        >
          <Segmented<SaveMode>
            value={sv.saveMode}
            options={[
              { value: "remux", label: "Instant (copy)" },
              { value: "reencode", label: "Re-encode" },
            ]}
            onChange={(v) => {
              update((d) => {
                d.saving.saveMode = v;
              });
            }}
          />
        </Field>
      </Section>

      <Section title="After saving" description="Optional actions that run after every saved clip.">
        <Field label="Open in editor">
          <Toggle
            value={sv.postSave.openEditor}
            onChange={(v) => {
              update((d) => {
                d.saving.postSave.openEditor = v;
              });
            }}
          />
        </Field>
        <Field label="Open containing folder">
          <Toggle
            value={sv.postSave.openFolder}
            onChange={(v) => {
              update((d) => {
                d.saving.postSave.openFolder = v;
              });
            }}
          />
        </Field>
        <Field label="Copy file path to clipboard">
          <Toggle
            value={sv.postSave.copyPath}
            onChange={(v) => {
              update((d) => {
                d.saving.postSave.copyPath = v;
              });
            }}
          />
        </Field>
        <Field
          label="Run command"
          description={
            <>
              Runs via <span className="mono">cmd /C</span>;{" "}
              <span className="mono">{"{path}"}</span> is replaced with the quoted clip path.
            </>
          }
          error={err("saving.postSave.runCommand")}
        >
          {sv.postSave.runCommand && <Badge tone="warn">Runs on every save</Badge>}
          <TextInput
            value={sv.postSave.runCommand ?? ""}
            placeholder="e.g. copy {path} D:\Backup"
            mono
            onCommit={(v) => {
              update(
                (d) => {
                  d.saving.postSave.runCommand = v.trim() ? v.trim() : null;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <Field label="Play a sound when saved">
          <Toggle
            value={settings.general.soundOnSave}
            onChange={(v) => {
              update((d) => {
                d.general.soundOnSave = v;
              });
            }}
          />
        </Field>
      </Section>
    </div>
  );
}
