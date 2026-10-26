import { useState } from "react";

import { Icon } from "../../components/Icon";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Button } from "../../components/controls";
import { Button as UiButton } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "../../components/ui/dropdown-menu";
import { Input } from "../../components/ui/misc";
import { commands } from "../../lib/tauri/commands";
import { errorMessage, type ClipInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

const stemOf = (name: string) => name.replace(/\.[^.]+$/, "");

/**
 * A clip's star, copy button and "⋯" menu (open in player, show in folder,
 * rename, export audio, delete), on cards and the latest-clip tile.
 */
export function ClipActions({ clip }: { clip: ClipInfo }) {
  const notify = useApp((s) => s.notify);
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      notify("error", "Action failed", errorMessage(e));
    }
  };
  return (
    <>
      <UiButton
        variant="ghost"
        size="icon-sm"
        title={clip.favorite ? "Remove from favorites" : "Add to favorites"}
        aria-label={clip.favorite ? "Remove from favorites" : "Add to favorites"}
        aria-pressed={clip.favorite}
        onClick={() => void act(() => commands.setFavorite(clip.path, !clip.favorite))}
      >
        <Icon
          name="star"
          size={14}
          filled={clip.favorite}
          className={clip.favorite ? "text-warning" : undefined}
        />
      </UiButton>
      <UiButton
        variant="ghost"
        size="icon-sm"
        title="Copy clip"
        aria-label="Copy clip"
        onClick={() =>
          void act(async () => {
            await commands.copyClip(clip.path);
            notify("info", "Clip copied", "Paste it anywhere with Ctrl+V.");
          })
        }
      >
        <Icon name="copy" size={14} />
      </UiButton>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <UiButton variant="ghost" size="icon-sm" title="More" aria-label="More actions">
            <Icon name="more" size={14} />
          </UiButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={() => void act(() => commands.openClipExternal(clip.path))}>
            <Icon name="external" /> Open in default player
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void act(() => commands.revealClip(clip.path))}>
            <Icon name="folder" /> Show in folder
          </DropdownMenuItem>
          <DropdownMenuItem
            onSelect={() => {
              setRenaming(true);
            }}
          >
            <Icon name="edit" /> Rename…
          </DropdownMenuItem>
          {clip.audioTracks.length > 0 && (
            <DropdownMenuItem
              onSelect={() =>
                void act(async () => {
                  await commands.exportAudioTracks(clip.path, true);
                  notify("info", "Audio tracks exported", "One WAV file per track.");
                })
              }
            >
              <Icon name="audio" /> Export audio tracks (WAV)
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem
            variant="destructive"
            onSelect={() => {
              setDeleting(true);
            }}
          >
            <Icon name="trash" /> Delete…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {renaming && (
        <RenameDialog
          clip={clip}
          onClose={() => {
            setRenaming(false);
          }}
        />
      )}
      {deleting && (
        <ConfirmDialog
          title={`Delete ${clip.fileName}?`}
          confirmLabel="Delete"
          onConfirm={() => {
            setDeleting(false);
            void act(() => commands.deleteClip(clip.path));
          }}
          onCancel={() => {
            setDeleting(false);
          }}
        >
          The file is deleted from disk.
        </ConfirmDialog>
      )}
    </>
  );
}

function RenameDialog({ clip, onClose }: { clip: ClipInfo; onClose: () => void }) {
  const notify = useApp((s) => s.notify);
  const [name, setName] = useState(stemOf(clip.fileName));
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    if (!name.trim() || name.trim() === stemOf(clip.fileName)) {
      onClose();
      return;
    }
    try {
      await commands.renameClip(clip.path, name);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      notify("error", "Couldn't rename the clip", errorMessage(e));
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Rename clip</DialogTitle>
        </DialogHeader>
        <Input
          autoFocus
          value={name}
          aria-invalid={error !== null || undefined}
          onChange={(e) => {
            setName(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") void submit();
          }}
        />
        {error && <p className="text-sm text-destructive">{error}</p>}
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => void submit()}>
            Rename
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
