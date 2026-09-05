import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import postcss from "postcss";
import ts from "typescript";

const mainUrl = new URL("../src/main.ts", import.meta.url);
const source = ts.createSourceFile("main.ts", readFileSync(mainUrl, "utf8"), ts.ScriptTarget.Latest, true);
const imports = source.statements.filter(ts.isImportDeclaration).map((node) => node.moduleSpecifier.text);
assert.ok(!imports.some((specifier) => specifier.startsWith("@fontsource/inter")), "Static Inter duplicates the active variable family");
assert.ok(imports.includes("@fontsource/jetbrains-mono/400.css"), "Keep the existing monospace font");
const css = postcss.parse(readFileSync(new URL("../src/app.css", import.meta.url), "utf8"));
const fontImports = [];
css.walkAtRules("import", (rule) => { if (rule.params.includes("@fontsource")) fontImports.push(rule.params); });
assert.deepEqual(fontImports, ['"@fontsource-variable/inter"']);
css.walkDecls("--font-sans", (declaration) => assert.match(declaration.value, /["']Inter Variable["']/));

const variableCss = postcss.parse(readFileSync(fileURLToPath(import.meta.resolve("@fontsource-variable/inter")), "utf8"));
const subsets = new Set();
variableCss.walkAtRules("font-face", (rule) => {
  const declarations = Object.fromEntries(rule.nodes.filter((node) => node.type === "decl").map((node) => [node.prop, node.value]));
  assert.equal(declarations["font-weight"], "100 900");
  assert.ok(declarations["unicode-range"]);
  const subset = declarations.src.match(/inter-([a-z-]+)-wght-normal\.woff2/);
  assert.ok(subset, "Keep the existing variable WOFF2 faces");
  subsets.add(subset[1]);
});
assert.deepEqual([...subsets].sort(), ["cyrillic", "cyrillic-ext", "greek", "greek-ext", "latin", "latin-ext", "vietnamese"], "Preserve all seven language subsets");
console.log("Font import fixtures passed: single Inter family, weights 100-900 and all seven language subsets preserved, monospace retained. No static Inter package required.");
