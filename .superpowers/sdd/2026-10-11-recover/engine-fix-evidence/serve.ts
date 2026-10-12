// Static server for dist/ under /math-course (astro preview exits 255 under headless Chrome load on this machine).
const ROOT = process.argv[2];
Bun.serve({
	port: 4521,
	async fetch(req) {
		let p = decodeURIComponent(new URL(req.url).pathname);
		if (!p.startsWith('/math-course')) return new Response('nf', { status: 404 });
		p = p.slice('/math-course'.length) || '/';
		if (p.endsWith('/')) p += 'index.html';
		const f = Bun.file(ROOT + p);
		return (await f.exists()) ? new Response(f) : new Response('nf', { status: 404 });
	},
});
console.log('serving', ROOT);
