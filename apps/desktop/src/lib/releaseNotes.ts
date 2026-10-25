/**
 * The bullets of a release description written for the app: each language
 * sits between `<!-- arcvox:notes lang=xx -->` and `<!-- /arcvox:notes -->`,
 * one `- **Lead**: detail` bullet per line. Anything outside the blocks is
 * for the release page only.
 */
export interface Note { lead: string; detail: string }

const BLOCK = /<!--\s*arcvox:notes\s+lang=([a-zA-Z-]+)\s*-->([\s\S]*?)<!--\s*\/arcvox:notes\s*-->/g;
const BULLET = /^\s*[-*]\s+(?:\*\*(.+?)\*\*\s*[:：]?\s*)?(.*)$/;

function bullets(block: string): Note[] {
  const out: Note[] = [];
  for (const line of block.split("\n")) {
    const m = BULLET.exec(line);
    if (!m) continue;
    const lead = (m[1] ?? "").trim();
    const detail = m[2].trim();
    if (lead || detail) out.push({ lead, detail });
  }
  return out;
}

/** The notes in `lang` (`zh-TW` reads the `zh` block), else in English; none without a block. */
export function releaseNotes(body: string | null | undefined, lang: string): Note[] {
  if (!body) return [];
  const blocks = new Map<string, Note[]>();
  for (const m of body.matchAll(BLOCK)) blocks.set(m[1].toLowerCase(), bullets(m[2]));
  const want = lang.toLowerCase().split("-")[0];
  return blocks.get(want) ?? blocks.get("en") ?? [];
}
