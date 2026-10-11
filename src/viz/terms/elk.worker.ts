/**
 * Web Worker that runs ELK (the Eclipse Layout Kernel, compiled to JS) off the main thread for the term maps.
 *
 * Importing ELK's worker build inside a worker scope is all it takes: its tail sees `self` without `document` and
 * installs `self.onmessage` = ELK's dispatcher, the protocol `elkjs/lib/elk-api` speaks from the page (layout.ts).
 * `elk.bundled.js` cannot be used here: its embedded copy of this same file takes over `self.onmessage` in a worker
 * scope instead of exporting its in-thread FakeWorker, and its constructor then fails ("undefined is not a constructor").
 */
import 'elkjs/lib/elk-worker.min.js';
