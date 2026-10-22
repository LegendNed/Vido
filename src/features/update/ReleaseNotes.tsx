import type { ReactNode } from "react";

/**
 * GitHub release notes (Markdown) as plain, readable text: headings, bullet lists
 * and **bold**; links show their text (or the URL) but are not clickable. No HTML
 * is ever injected.
 */
export function ReleaseNotes({ markdown }: { markdown: string }) {
  const blocks: ReactNode[] = [];
  let list: string[] = [];
  const flush = () => {
    if (list.length > 0) {
      blocks.push(
        <ul key={`l${blocks.length}`} className="ml-4 list-disc space-y-0.5">
          {list.map((item, i) => (
            <li key={i}>{inline(item)}</li>
          ))}
        </ul>,
      );
      list = [];
    }
  };
  for (const raw of markdown.replace(/\r\n/g, "\n").split("\n")) {
    const line = raw.trim();
    const bullet = /^[-*+]\s+(.*)$/.exec(line);
    if (bullet) {
      list.push(bullet[1] ?? "");
      continue;
    }
    flush();
    if (!line) continue;
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push(
        <p key={`h${blocks.length}`} className="font-semibold">
          {inline(heading[1] ?? "")}
        </p>,
      );
    } else {
      blocks.push(<p key={`p${blocks.length}`}>{inline(line)}</p>);
    }
  }
  flush();
  return <div className="space-y-2">{blocks}</div>;
}

/** `**bold**`, `` `code` `` and `[text](url)` (the text only). */
function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\)/g;
  let last = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    if (m[1] !== undefined) out.push(<strong key={m.index}>{m[1]}</strong>);
    else if (m[2] !== undefined)
      out.push(
        <code key={m.index} className="font-mono text-xs">
          {m[2]}
        </code>,
      );
    else out.push(m[3]);
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}
