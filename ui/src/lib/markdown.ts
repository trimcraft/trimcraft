// A small Markdown reader for CREDITS.md: headings, paragraphs, bullet lists, **bold**,
// `code` and [links](url). Produces plain data, rendered without {@html}; links come out as
// their text (the app opens only fixed URLs).

export type Inline = { text: string; bold?: boolean; code?: boolean };
export type Block =
  | { kind: "heading"; level: number; text: Inline[] }
  | { kind: "paragraph"; text: Inline[] }
  | { kind: "list"; items: Inline[][] };

export function inline(source: string): Inline[] {
  const out: Inline[] = [];
  const re = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\([^)]*\)/g;
  let last = 0;
  for (const m of source.matchAll(re)) {
    if (m.index > last) out.push({ text: source.slice(last, m.index) });
    if (m[1] !== undefined) out.push({ text: m[1], bold: true });
    else if (m[2] !== undefined) out.push({ text: m[2], code: true });
    else out.push({ text: m[3] ?? "" });
    last = m.index + m[0].length;
  }
  if (last < source.length) out.push({ text: source.slice(last) });
  return out;
}

export function parse(source: string): Block[] {
  const blocks: Block[] = [];
  let para: string[] = [];
  let items: string[] | null = null;
  const flush = () => {
    if (para.length) blocks.push({ kind: "paragraph", text: inline(para.join(" ")) });
    if (items) blocks.push({ kind: "list", items: items.map(inline) });
    para = [];
    items = null;
  };
  for (const raw of source.split(/\r?\n/)) {
    const line = raw.trimEnd();
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    const bullet = /^[-*]\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      blocks.push({ kind: "heading", level: heading[1]!.length, text: inline(heading[2]!) });
    } else if (bullet) {
      if (para.length) flush();
      items ??= [];
      items.push(bullet[1]!);
    } else if (line.trim() === "") {
      flush();
    } else if (items && /^\s+\S/.test(raw)) {
      // Continuation of the last list item.
      items[items.length - 1] += " " + line.trim();
    } else {
      if (items) flush();
      para.push(line.trim());
    }
  }
  flush();
  return blocks;
}
