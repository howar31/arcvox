import { expect, test } from "vitest";
import { copyrightLines, filterPackages, findPackage, licenseIds, packageKey, reflowed, sortPackages, withoutPlaceholders, type ThirdPartyPackage } from "./thirdParty";

const pkg = (name: string, source: "rust" | "npm", license = "MIT", version = "1.0.0"): ThirdPartyPackage => ({ name, version, source, license, copyright: [] });

test("the identifiers of a licence expression, in order and once each", () => {
  expect(licenseIds("MIT OR Apache-2.0")).toEqual(["MIT", "Apache-2.0"]);
  expect(licenseIds("MIT/Apache-2.0")).toEqual(["MIT", "Apache-2.0"]);
  expect(licenseIds("(MIT OR Apache-2.0) AND Unicode-3.0")).toEqual(["MIT", "Apache-2.0", "Unicode-3.0"]);
  expect(licenseIds("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT")).toEqual(["Apache-2.0", "LLVM-exception", "MIT"]);
  expect(licenseIds("Apache-2.0 / MIT")).toEqual(["Apache-2.0", "MIT"]);
  expect(licenseIds("")).toEqual([]);
});

test("the copyright lines of a licence file", () => {
  const text = "MIT License\n\nCopyright (c) 2019 Tokio Contributors\nCopyright   (c) 2019  Tokio Contributors\n\nPermission is hereby granted...\n© 2020 Someone\n";
  expect(copyrightLines(text)).toEqual(["Copyright (c) 2019 Tokio Contributors", "© 2020 Someone"]);
  expect(copyrightLines("no notice here")).toEqual([]);
  expect(copyrightLines("Copyright (c) 2016 [Svelte Contributors](https://example.test/x)")).toEqual(["Copyright (c) 2016 Svelte Contributors"]);
  const many = Array.from({ length: 6 }, (_, i) => `Copyright ${i}`).join("\n");
  expect(copyrightLines(many)).toHaveLength(4);
});

test("a template keeps its terms and loses its placeholder notice", () => {
  const mit = "MIT License\n\nCopyright (c) <year> <copyright holders>\n\nPermission is hereby granted";
  expect(withoutPlaceholders(mit)).toBe("MIT License\n\nPermission is hereby granted");
  const bsd = "Copyright (c) <year> <owner>. \n\nRedistribution and use";
  expect(withoutPlaceholders(bsd)).toBe("Redistribution and use");
  expect(withoutPlaceholders("Copyright (c) 2020 Real Person\nterms")).toBe("Copyright (c) 2020 Real Person\nterms");
});

test("wrapped paragraphs are joined; headings, blank lines and list items are kept", () => {
  const long = "Permission is hereby granted, free of charge, to any person obtaining a copy of this";
  expect(reflowed(`${long}\nsoftware and associated documentation files.\n\nThe above notice`)).toBe(`${long} software and associated documentation files.\n\nThe above notice`);
  expect(reflowed("Apache License\nVersion 2.0, January 2004")).toBe("Apache License\nVersion 2.0, January 2004");
  expect(reflowed(`${long}\n1. Definitions.\n${long}\n   indented`)).toBe(`${long}\n1. Definitions.\n${long}\n   indented`);
  expect(reflowed(`${long}\n(a) item`)).toBe(`${long}\n(a) item`);
});

test("the filter matches names and licences, case-insensitively", () => {
  const all = [pkg("tokio", "rust"), pkg("svelte", "npm"), pkg("serde", "rust", "MIT OR Apache-2.0")];
  expect(filterPackages(all, "")).toBe(all);
  expect(filterPackages(all, "TOK").map((p) => p.name)).toEqual(["tokio"]);
  expect(filterPackages(all, "apache").map((p) => p.name)).toEqual(["serde"]);
  expect(filterPackages(all, "  mit ").map((p) => p.name)).toEqual(["tokio", "svelte", "serde"]);
});

test("packages sort by name, then source", () => {
  const all = [pkg("zlib", "rust"), pkg("Alpha", "npm"), pkg("beta", "rust"), pkg("beta", "npm")];
  expect(sortPackages(all).map((p) => `${p.name}/${p.source}`)).toEqual(["Alpha/npm", "beta/npm", "beta/rust", "zlib/rust"]);
});

test("a package's key carries the source only when the name is shared", () => {
  const all = [pkg("tokio", "rust"), pkg("beta", "rust"), pkg("beta", "npm"), pkg("base64", "rust", "MIT", "0.21.7"), pkg("base64", "rust", "MIT", "0.22.1")];
  expect(packageKey(all[0], all)).toBe("tokio");
  expect(packageKey(all[1], all)).toBe("beta@rust");
  expect(findPackage(all, "beta@npm")).toBe(all[2]);
  // The same crate at two versions.
  expect(packageKey(all[3], all)).toBe("base64@rust@0.21.7");
  expect(findPackage(all, "base64@rust@0.22.1")).toBe(all[4]);
  expect(findPackage(all, "tokio")).toBe(all[0]);
  expect(findPackage(all, "nope")).toBeUndefined();
});
