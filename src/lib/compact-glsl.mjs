/** Remove only GLSL whitespace; preprocessor lines retain their exact internal spacing. */
export function compactGLSL(shader) {
 const packed = shader.split('\n').map(line => {
  line = line.trim();
  if (line.startsWith('#') || line.includes('//') || line.includes('/*') || line.includes('*/')) return line;
  const tokens = line.match(/\+\+|--|==|!=|<=|>=|&&|\|\||\+=|-=|\*=|\/=|[A-Za-z_]\w*|(?:\d*\.\d+|\d+\.?\d*)(?:e[+-]?\d+)?|[^\s]/gi) ?? [];
  return tokens.reduce((out, token, i) => {
   const prev = tokens[i - 1] ?? '';
   // Preserve identifier/number boundaries and prevent two operator tokens merging.
   const gap = /[\w.]$/.test(prev) && /^[\w.]/.test(token) ||
    /^[+\-*\/<>=!&|%^]+$/.test(prev) && /^[+\-*\/<>=!&|%^]+$/.test(token);
   return out + (gap ? ' ' : '') + token;
  }, '');
 }).filter(Boolean);
 return packed.reduce((out, line) => {
  const previous = out.slice(out.lastIndexOf('\n') + 1);
  const join = /[;{},(]$/.test(previous) && !previous.includes('//') && !previous.startsWith('#') && !line.startsWith('#');
  return out + (out && !join ? '\n' : '') + line;
 }, '') + '\n';
}
export function compactThreeShaders(code) {
 return code.replace(/\bvar (\w+) = ("(?:\\.|[^"\\])*");/g, (original, name, literal) => {
  const shader = JSON.parse(literal);
  if (!shader.includes('\n') || !/\b(vec[234]|float|uniform|varying)\b|#include/.test(shader)) return original;
  return 'var ' + name + ' = ' + JSON.stringify(compactGLSL(shader)) + ';';
 });
}
