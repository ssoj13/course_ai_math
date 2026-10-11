import { createContext, useContext, type ReactNode, type ComponentType } from 'react';
import type { Locale } from './core';

/** Locale is explicit per island; it never depends on a server-global value or a hydration effect. */
export const LocaleContext = createContext<Locale>('en');
export const useLocale = (): Locale => useContext(LocaleContext);
export interface LocaleProps { locale: Locale }
export function LocaleRoot({ locale, children }: LocaleProps & { children: ReactNode }) {
 return <LocaleContext.Provider value={locale}>{children}</LocaleContext.Provider>;
}

/** Wrap an SSR-safe entry, including a lazy scene entry, without serializing row callbacks. */
export function localized<P extends object>(Component: ComponentType<P>): ComponentType<P & { locale?: Locale }> {
 return function Localized({ locale = 'en', ...props }: P & { locale?: Locale }) {
  return <LocaleRoot locale={locale}><Component {...props as P} /></LocaleRoot>;
 };
}
