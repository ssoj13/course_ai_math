/** SSR-safe demonstration entries; locale arrives from the Astro wrapper. */
import { localized } from '../../i18n/react';
import type { Locale } from '../../i18n/core';
import { lazyIsland } from '../island';
import { ROWSPOLAR } from './engine.rows';
export const ROWSHOLO = (locale: Locale) => ROWSPOLAR(locale).slice(0, 4);
export const DemoHolo = localized(lazyIsland(() => import('./holo.impl').then(m => m.DemoHolo)));
export const DemoFloor = localized(lazyIsland(() => import('./holo.impl').then(m => m.DemoFloor)));
export const DemoClay = localized(lazyIsland(() => import('./holo.impl').then(m => m.DemoClay)));
