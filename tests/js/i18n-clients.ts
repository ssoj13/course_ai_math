import ts from 'typescript';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';

/** Imports, reexports, workers and literal dynamic imports, including lazy React implementations. */
export function importsOf(text: string, file: string) {
 const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
 const out: { spec: string; names: string[]; worker: boolean }[] = [];
 const add = (spec: ts.Node | undefined, names: string[] = [], worker = false) => {
  if (spec && ts.isStringLiteralLike(spec)) out.push({ spec: spec.text, names, worker });
 };
 const visit = (node: ts.Node) => {
  if (ts.isImportDeclaration(node)) {
   const clause = node.importClause, bindings = clause?.namedBindings;
   if (clause?.phaseModifier !== ts.SyntaxKind.TypeKeyword) {
    const names = clause?.name ? [clause.name.text] : [];
    if (bindings && ts.isNamedImports(bindings)) names.push(...bindings.elements.filter(e => !e.isTypeOnly).map(e => e.name.text));
    if (bindings && ts.isNamespaceImport(bindings)) names.push(bindings.name.text);
    if (!clause || names.length) add(node.moduleSpecifier, names);
   }
  } else if (ts.isExportDeclaration(node) && !node.isTypeOnly) {
   if (!node.exportClause || !ts.isNamedExports(node.exportClause) || node.exportClause.elements.some(e => !e.isTypeOnly)) add(node.moduleSpecifier);
  } else if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) add(node.arguments[0]);
  else if (ts.isNewExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'URL') add(node.arguments?.[0], [], true);
  ts.forEachChild(node, visit);
 };
 visit(source);
 return out;
}
export interface Files {
 read(file: string): string;
 exists(file: string): boolean;
}
const disk: Files = { read: f => readFileSync(f, 'utf8'), exists: f => existsSync(f) && statSync(f).isFile() };
export function clientGraph(text: string, file: string, fs: Files = disk) {
 const front = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text)?.[1] ?? '';
 const scripts = [...text.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(m => m[1]).join('\n');
 const roots = new Set([...text.matchAll(/<([\w.]+)\b[^>]*\bclient:(?:load|idle|visible|media|only)\b/g)].map(m => m[1].split('.')[0]));
 const rootImports = importsOf(front, file).filter(e => e.names.some(n => roots.has(n)));
 const imports: { from: string; file: string; worker: boolean }[] = [];
 const texts: string[] = [scripts];
 const seen = new Set<string>();
 const resolve = (spec: string, from: string) => {
  if (!spec.startsWith('.')) return;
  const base = join(dirname(from), spec);
  return [base, ...['.ts','.tsx','.mjs','.js','/index.ts','/index.tsx'].map(ext => base + ext)].find(p => /\.(tsx?|m?js)$/.test(p) && fs.exists(p));
 };
 const walk = (spec: string, from: string, worker = false) => {
  const target = resolve(spec, from);
  if (!target) return;
  imports.push({ from, file: target, worker });
  if (seen.has(target)) return;
  seen.add(target);
  const next = fs.read(target);
  if (!/[\\/]i18n[\\/]/.test(target)) texts.push(next);
  for (const edge of importsOf(next, target)) walk(edge.spec, target, edge.worker);
 };
 for (const e of [...rootImports, ...importsOf(scripts, file)]) walk(e.spec, file, e.worker);
 const tags = [...text.matchAll(/<UiStrings\b[^>]*>/g)].map(m => m[0]);
 const declared = tags.flatMap(tag => [...(tag.match(/ns=\{\[([^\]]*)\]\}/)?.[1] ?? '').matchAll(/['"]([^'"]+)['"]/g)].map(m => m[1]));
 return { texts, imports, tags, declared, roots: [...roots] };
}
export function namespaces(texts: string[], known: string[]) {
 const re = new RegExp("['\"\x60](" + known.join('|') + ")\\.[A-Za-z0-9_.]+['\"\x60]", 'g');
 return [...new Set(texts.flatMap(text => [...text.matchAll(re)].map(m => m[1])))].sort();
}
