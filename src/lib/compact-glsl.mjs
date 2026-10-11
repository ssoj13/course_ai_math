/** Remove only GLSL whitespace; preprocessor lines retain their exact internal spacing. */
export function compactGLSL(shader) {
 return shader.split('\n').map(line => line.trim().startsWith('#') ? line.trim() : line.trim().replace(/[\t ]+/g, ' ')
  .replace(/\s*([(){}\[\],;:=*\/<>?])\s*/g, (_space, token) => token)).filter(Boolean).join('\n') + '\n';
}
export function compactThreeShaders(code) {
 return code.replace(/\bvar (\w+) = ("(?:\\.|[^"\\])*");/g, (original, name, literal) => {
  const shader = JSON.parse(literal);
  if (!shader.includes('\n') || !/\b(vec[234]|float|uniform|varying)\b|#include/.test(shader)) return original;
  return 'var ' + name + ' = ' + JSON.stringify(compactGLSL(shader)) + ';';
 });
}
