import '../../src/i18n/ui';
/**
 * Opt-in DOM for component tests. Import it first in a test file that mounts React (react-dom/client, act).
 * It is deliberately not preloaded, so pure tests stay DOM-free. bun runs test files in one process, so once any
 * DOM test loads this, later files also see `window`/`document`; pure modules must not depend on their absence.
 */
import { GlobalRegistrator } from '@happy-dom/global-registrator';

// Tell React that updates come from a test harness, so `act` is required and warnings are reported.
declare global {
	var IS_REACT_ACT_ENVIRONMENT: boolean;
}

if (!globalThis.document) GlobalRegistrator.register();
// KaTeX checks compatMode when it is first imported and warns in quirks mode; happy-dom documents have no doctype.
Object.defineProperty(document, 'compatMode', { value: 'CSS1Compat', configurable: true });
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

// Frame-clock stubs that always restore the originals (one process for all files; see frames.ts).
export { captureFrames, stubFrames, timerFrames, useFrameStub, type FramePair } from './frames';
