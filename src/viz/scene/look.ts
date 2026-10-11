/** Three-free rendering context: reflections never participate in interaction. */
import { createContext, useContext } from 'react';
export type Look = 'holo' | 'clay';
export const LookCtx = createContext<Look>('holo');
export const MirrorCtx = createContext(false);
export const useMirror = () => useContext(MirrorCtx);
export function assertSubject(look: Look, filament: boolean): void {
 if (look === 'clay' && filament) throw new Error('Clay studio cards accept solids and measurement guides, not holographic subjects.');
}
export function hotAlpha(seconds: number, reduced: boolean): number {
 return reduced ? 0.36 : 0.36 - 0.06 * Math.cos(seconds * Math.PI / 0.6);
}
export function lightened(hex: string): string {
 const n = parseInt(hex.slice(1), 16);
 return '#' + [16,8,0].map(s => Math.round(((n >> s) & 255) * .65 + 255 * .35).toString(16).padStart(2,'0')).join('');
}
