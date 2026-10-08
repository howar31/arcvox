/** The third-party packages shipped in a build, as the About page lists them. */

export type PackageSource = "rust" | "npm";

export interface ThirdPartyPackage {
  name: string;
  version: string;
  source: PackageSource;
  /** The licence expression the package declares, as written ("MIT OR Apache-2.0"). */
  license: string;
  /** The copyright lines of the package's own licence file, when it ships one. */
  copyright: string[];
}

export interface ThirdPartyData {
  packages: ThirdPartyPackage[];
  /** The text of each licence named by a package, by its identifier. */
  texts: Record<string, string>;
}

/** A row's key in the licence list: the name alone, with the source when two packages share a name, and the version when a crate ships twice. */
export function packageKey(pkg: ThirdPartyPackage, all: ThirdPartyPackage[]): string {
  const sameName = all.filter((p) => p !== pkg && p.name === pkg.name);
  if (sameName.length === 0) return pkg.name;
  if (!sameName.some((p) => p.source === pkg.source)) return `${pkg.name}@${pkg.source}`;
  return `${pkg.name}@${pkg.source}@${pkg.version}`;
}

export function findPackage(all: ThirdPartyPackage[], key: string): ThirdPartyPackage | undefined {
  return all.find((p) => packageKey(p, all) === key);
}

/** The licence identifiers named in an expression, in order; "/" between two names is read as OR. */
export function licenseIds(expression: string): string[] {
  const out: string[] = [];
  for (const token of expression.split(/[\s()/]+/)) {
    if (!token || /^(OR|AND|WITH)$/i.test(token)) continue;
    if (!out.includes(token)) out.push(token);
  }
  return out;
}

/** The copyright lines of a licence file: the lines that start with a copyright notice, at most four. */
export function copyrightLines(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split(/\r?\n/)) {
    // A notice may link its holder in Markdown; the text alone is kept.
    const line = raw.trim().replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").replace(/\s+/g, " ");
    if (!/^(copyright|©|\(c\))/i.test(line)) continue;
    if (!out.includes(line)) out.push(line);
    if (out.length === 4) break;
  }
  return out;
}

/** A licence text without the template's own placeholder notice ("Copyright (c) <year> <owner>"). */
export function withoutPlaceholders(text: string): string {
  return text
    .split("\n")
    .filter((line) => !(/^\s*copyright/i.test(line) && /<[a-z ]+>/i.test(line)))
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/^\n+/, "");
}

/** A hard-wrapped licence text with its paragraphs on one line each, so it fits any width; headings, short lines and list items keep their breaks. */
export function reflowed(text: string): string {
  const lines = text.split("\n");
  const out: string[] = [];
  for (const line of lines) {
    const prev = out[out.length - 1];
    const continues = prev !== undefined && prev.trim().length > 60 && line.length > 0 && !/^(\s|\d+\.|\([a-z0-9]+\)|[-*])/i.test(line);
    if (continues) out[out.length - 1] = `${prev.trimEnd()} ${line.trim()}`;
    else out.push(line);
  }
  return out.join("\n");
}

/** The packages whose name or licence contains the query, case-insensitively; all of them for an empty query. */
export function filterPackages(all: ThirdPartyPackage[], query: string): ThirdPartyPackage[] {
  const q = query.trim().toLowerCase();
  if (!q) return all;
  return all.filter((p) => p.name.toLowerCase().includes(q) || p.license.toLowerCase().includes(q));
}

/** Packages by name, then by source, so the list reads the same on every build. */
export function sortPackages(all: ThirdPartyPackage[]): ThirdPartyPackage[] {
  return [...all].sort((a, b) => a.name.localeCompare(b.name, "en") || a.source.localeCompare(b.source, "en"));
}
