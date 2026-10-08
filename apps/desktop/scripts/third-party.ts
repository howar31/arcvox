// Writes src/generated/third-party.json: the packages a build ships, with their licences,
// for the About page. Runs before `vite` (see package.json); needs the Rust sources fetched.
//
// Rust: the crates of the desktop app on the build's target (TAURI_ENV_TARGET_TRIPLE, else
// the host), following normal dependencies only, so build-time and test-only crates stay out.
// npm: the runtime dependencies of package.json plus the packages whose code the bundle
// carries although they are listed for development.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { copyrightLines, licenseIds, reflowed, sortPackages, withoutPlaceholders, type ThirdPartyData, type ThirdPartyPackage } from "../src/lib/thirdParty.ts";

const here = dirname(fileURLToPath(import.meta.url));
const app = resolve(here, "..");
const out = join(app, "src", "generated", "third-party.json");

/** Development packages whose code ships in the bundle all the same. */
const BUNDLED_DEV_PACKAGES = ["svelte"];
const ROOT_CRATE = "arcvox-desktop";

function licenseFileText(dir: string): string {
  const names = existsSync(dir) ? readdirSync(dir) : [];
  const file = names.filter((n) => /^(licen[cs]e|copying)/i.test(n)).sort()[0];
  return file ? readFileSync(join(dir, file), "utf8") : "";
}

function hostTriple(): string {
  const info = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  return /^host:\s*(\S+)/m.exec(info)?.[1] ?? "";
}

interface CargoPackage { id: string; name: string; version: string; license: string | null; manifest_path: string }
interface CargoNode { id: string; deps: { pkg: string; dep_kinds: { kind: string | null }[] }[] }
interface CargoMetadata { packages: CargoPackage[]; workspace_members: string[]; resolve: { nodes: CargoNode[] } }

function rustPackages(): ThirdPartyPackage[] {
  const target = process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple();
  const args = ["metadata", "--format-version", "1", "--filter-platform", target];
  const meta = JSON.parse(execFileSync("cargo", args, { cwd: app, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 })) as CargoMetadata;
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const members = new Set(meta.workspace_members);
  const root = meta.packages.find((p) => p.name === ROOT_CRATE && members.has(p.id));
  if (!root) throw new Error(`crate ${ROOT_CRATE} is not a workspace member`);
  const seen = new Set<string>();
  const stack = [root.id];
  while (stack.length) {
    const id = stack.pop()!;
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((k) => k.kind === null)) stack.push(dep.pkg);
    }
  }
  const list: ThirdPartyPackage[] = [];
  for (const id of seen) {
    if (members.has(id)) continue;
    const p = byId.get(id)!;
    if (!p.license) console.warn(`third-party: ${p.name} declares no licence`);
    list.push({ name: p.name, version: p.version, source: "rust", license: p.license ?? "", copyright: copyrightLines(licenseFileText(dirname(p.manifest_path))) });
  }
  return list;
}

function npmPackages(): ThirdPartyPackage[] {
  const manifest = JSON.parse(readFileSync(join(app, "package.json"), "utf8")) as { dependencies?: Record<string, string> };
  const names = [...Object.keys(manifest.dependencies ?? {}), ...BUNDLED_DEV_PACKAGES];
  return names.map((name) => {
    const dir = join(app, "node_modules", name);
    const p = JSON.parse(readFileSync(join(dir, "package.json"), "utf8")) as { version: string; license?: string };
    return { name, version: p.version, source: "npm", license: p.license ?? "", copyright: copyrightLines(licenseFileText(dir)) };
  });
}

function texts(packages: ThirdPartyPackage[]): Record<string, string> {
  const dir = join(app, "licenses");
  const result: Record<string, string> = {};
  const ids = new Set(packages.flatMap((p) => licenseIds(p.license)));
  for (const id of [...ids].sort()) {
    const file = join(dir, `${id}.txt`);
    if (existsSync(file)) result[id] = reflowed(withoutPlaceholders(readFileSync(file, "utf8")));
    else console.warn(`third-party: no text for licence ${id} (${packages.filter((p) => licenseIds(p.license).includes(id)).map((p) => p.name).join(", ")})`);
  }
  return result;
}

const packages = sortPackages([...rustPackages(), ...npmPackages()]);
const data: ThirdPartyData = { packages, texts: texts(packages) };
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(data));
console.log(`third-party: ${packages.length} packages, ${Object.keys(data.texts).length} licence texts`);
