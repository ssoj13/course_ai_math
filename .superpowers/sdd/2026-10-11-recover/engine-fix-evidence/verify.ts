// One browser pass over the production preview: console/hydration errors on every page, 1.9 epicycles screenshots,
// 0.4 unit-circle card screenshots. Usage: bun verify.ts <outDir>
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';

const OUT = process.argv[2];
mkdirSync(OUT, { recursive: true });
const BASE = 'http://localhost:4521/math-course';
const PAGES = ['', 'course/roadmap', 'dev/engine', 'dev/linked', 'dev/math', 'glossary', 'ground/law-of-cosines', 'ground/powers',
	'ground/pythagoras', 'ground/squares-and-plane', 'ground/unit-circle', 'start/how-it-connects', 'start/why-this-course',
	'vectors/basis', 'vectors/cosine-similarity', 'vectors/cross-product', 'vectors/dot-product', 'vectors/length',
	'vectors/projection', 'vectors/vector', 'vectors/vector-arithmetic', 'vectors/waves-dft'];

const chrome = spawn('C:/Program Files/Google/Chrome/Application/chrome.exe', ['--headless=new', '--remote-debugging-port=9333',
	`--user-data-dir=${process.env.TEMP}/mathcdp-profile`, '--disk-cache-size=1', '--window-size=1280,900', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', 'about:blank'], { stdio: 'ignore' });
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let ws: WebSocket | null = null;
for (let i = 0; i < 40 && !ws; i++) {
	await sleep(250);
	try {
		const list = (await (await fetch('http://127.0.0.1:9333/json/list')).json()) as { type: string; webSocketDebuggerUrl: string }[];
		const page = list.find((t) => t.type === 'page');
		if (page) ws = new WebSocket(page.webSocketDebuggerUrl);
	} catch { /* chrome still starting */ }
}
if (!ws) throw new Error('chrome did not start');
await new Promise((r) => ws!.addEventListener('open', r, { once: true }));
let id = 0;
const pending = new Map<number, (v: any) => void>();
const errors: string[] = [];
ws.addEventListener('message', (m) => {
	const msg = JSON.parse(String(m.data));
	if (msg.id && pending.has(msg.id)) { pending.get(msg.id)!(msg.result ?? msg.error); pending.delete(msg.id); }
	if (msg.method === 'Runtime.exceptionThrown') errors.push('EXC ' + (msg.params.exceptionDetails.exception?.description ?? msg.params.exceptionDetails.text));
	if (msg.method === 'Runtime.consoleAPICalled' && (msg.params.type === 'error' || msg.params.type === 'warning'))
		errors.push(msg.params.type.toUpperCase() + ' ' + msg.params.args.map((a: any) => a.value ?? a.description ?? '').join(' ').slice(0, 300));
	if (msg.method === 'Log.entryAdded' && msg.params.entry.level === 'error') errors.push('LOG ' + msg.params.entry.text + ' ' + (msg.params.entry.url ?? ''));
});
const send = (method: string, params: object = {}) => new Promise<any>((r) => { const i = ++id; pending.set(i, r); ws!.send(JSON.stringify({ id: i, method, params })); });
const ev = async (expr: string) => (await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true })).result?.value;
await send('Runtime.enable');
await send('Log.enable');
await send('Page.enable');

const report: Record<string, string[]> = {};
async function visit(path: string) {
	errors.length = 0;
	await send('Page.navigate', { url: `${BASE}/${path}${path ? '/' : ''}` });
	await sleep(3500);
	// Scroll through the page so every scene island mounts and draws.
	await ev(`(async()=>{for(let y=0;y<document.body.scrollHeight;y+=700){scrollTo(0,y);await new Promise(r=>setTimeout(r,120));}scrollTo(0,0);})()`);
	await sleep(500);
	const info = await ev(`document.title + ' | cards=' + document.querySelectorAll('.lab-vp-card').length + ' canvas=' + document.querySelectorAll('canvas').length`);
	report[path || 'index'] = [info, ...errors];
	console.log(path || 'index', info);
}

async function shotEl(sel: string, name: string) {
	const r = await ev(`(()=>{const e=document.querySelector(${JSON.stringify(sel)});if(!e)return null;e.scrollIntoView({block:'center'});const b=e.getBoundingClientRect();return {x:b.x,y:b.y,w:b.width,h:b.height};})()`);
	if (!r) return console.log('missing', sel);
	await sleep(700);
	const b = await ev(`(()=>{const b=document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect();return {x:b.x+scrollX,y:b.y+scrollY,w:b.width,h:b.height};})()`);
	const s = await send('Page.captureScreenshot', { format: 'png', clip: { x: b.x, y: b.y, width: b.w, height: b.h, scale: 1 }, captureBeyondViewport: false });
	writeFileSync(`${OUT}/${name}.png`, Buffer.from(s.data, 'base64'));
}

for (const loc of ['', 'ru/']) for (const p of PAGES) await visit(loc + p);

// 1.9 epicycles: S from 1 to 63, the trace at several times.
await visit('vectors/waves-dft');
const cards = await ev(`document.querySelectorAll('.lab-vp-card').length`);
console.log('waves cards', cards);
await ev(`window.__card=document.querySelectorAll('.lab-vp-card')[3];__card.id='wd4';1`);
async function setS(dir: 'PageUp' | 'PageDown', times: number) {
	await ev(`(()=>{const f=document.querySelector('#wd4 .lab-np-row[data-id="S"] [tabindex]')||document.querySelector('#wd4 .lab-np-row[data-id="S"] .lab-np-field');f.focus();return document.activeElement.className})()`);
	for (let i = 0; i < times; i++) {
		await send('Input.dispatchKeyEvent', { type: 'keyDown', key: dir, code: dir, windowsVirtualKeyCode: dir === 'PageUp' ? 33 : 34 });
		await send('Input.dispatchKeyEvent', { type: 'keyUp', key: dir, code: dir, windowsVirtualKeyCode: dir === 'PageUp' ? 33 : 34 });
	}
	await sleep(900);
	return ev(`document.querySelector('#wd4 .lab-np-row[data-id="S"] .lab-np-val')?.textContent`);
}
async function setT(t: number) {
	await ev(`(()=>{const i=[...document.querySelectorAll('input[type=range]')].find(e=>e.step==='0.001');const s=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;s.call(i,'${t}');i.dispatchEvent(new Event('input',{bubbles:true}));})()`);
	await sleep(600);
}
const sLow = await setS('PageDown', 10);
console.log('S low', sLow);
for (const t of [0.3, 0.95]) { await setT(t); await shotEl('#wd4', `wd4-S${sLow}-t${t}`); }
for (const step of [1, 1, 7]) {
	const s = await setS('PageUp', step);
	console.log('S', s);
	for (const t of [0.25, 0.6, 0.98]) { await setT(t); await shotEl('#wd4', `wd4-S${s}-t${t}`); }
}
report['waves-interaction'] = [...errors];

// 0.4 unit circle: every card.
await visit('ground/unit-circle');
const n = await ev(`(()=>{const c=document.querySelectorAll('.lab-vp-card');c.forEach((e,i)=>e.id='uc'+i);return c.length})()`);
for (let i = 0; i < n; i++) await shotEl('#uc' + i, 'uc-card' + i);

writeFileSync(`${OUT}/report.json`, JSON.stringify(report, null, 1));
const bad = Object.entries(report).filter(([, e]) => e.length > 1);
console.log('pages', Object.keys(report).length, 'with messages', bad.length);
for (const [p, e] of bad) console.log(p, '\n  ' + e.join('\n  '));
ws.close();
chrome.kill();
process.exit(0);
