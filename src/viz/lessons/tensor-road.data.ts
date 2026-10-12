/**
 * Real data of lesson 3.1 (the tensor road), as plain numbers. Three-free and DOM-free (the pure tests import it).
 *
 * Provenance:
 * - `ICON`: an 8 x 8 RGB pixel-art picture (sky, sun, a house, grass) authored for this course as uint8 values, the
 *   storage of every PNG/JPEG decoder and of a texture in `RGBA8`/`RGB8` formats. All image examples (one pixel, the
 *   grayscale image, the batch, the video clips) are derived from it by the functions below, so they agree cell by cell.
 * - `GLOVE8`: the first 8 of the 300 numbers of five GloVe 6B 300d word vectors, copied from
 *   `src/data/embeddings-small.json` (lesson 1.7's file; its `provenance` block names the model, file and licence).
 *   Copied rather than imported so the scene bundle does not carry the 187 kB file; a test checks the copy.
 */

/** Pixel colours of the icon, uint8 sRGB. */
const PAL: Readonly<Record<string, readonly [number, number, number]>> = {
	s: [92, 156, 240], // sky
	y: [255, 217, 51], // sun
	r: [214, 64, 52], // roof
	w: [242, 230, 204], // wall
	k: [96, 56, 30], // door
	g: [70, 178, 80], // grass
	d: [34, 112, 52], // dark grass
};

/** The icon row by row (row 0 = top, as images are stored), one letter per pixel. */
const ROWS = [
	'ssssssyy',
	'ssssssyy',
	'sssrssss',
	'ssrrrsss',
	'srrrrrss',
	'gwwkwggg',
	'gwwkwgdg',
	'dddddddd',
] as const;

export const ICON_H = ROWS.length;
export const ICON_W = ROWS[0].length;

/** icon[h][w][c]: H x W x C = 8 x 8 x 3, uint8. */
export const ICON: readonly (readonly (readonly number[])[])[] = ROWS.map((row) => [...row].map((ch) => {
	const p = PAL[ch];
	if (!p) throw new Error(`tensor-road icon: unknown pixel "${ch}"`);
	return [...p];
}));

/** ITU-R BT.601 luma, rounded to uint8: the usual RGB -> grayscale conversion of image libraries. */
export const luma = (p: readonly number[]): number => Math.round(0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]);

/** The icon mirrored left-right (a data-augmentation flip). */
export const mirror = (img: readonly (readonly (readonly number[])[])[]) => img.map((row) => [...row].reverse());

/** The icon at dusk: every channel halved (integer division, as uint8 arithmetic does). */
export const dusk = (img: readonly (readonly (readonly number[])[])[]) => img.map((row) => row.map((p) => p.map((c) => Math.floor(c / 2))));

/** A camera pan: frame t is the picture rolled t pixels to the left (np.roll(img, -t, axis=1)). */
export const pan = (img: readonly (readonly (readonly number[])[])[], t: number) =>
	img.map((row) => row.map((_, w) => row[(w + t) % row.length]));

/** The sentence of lesson 1.7's attention preview, token by token. */
export const SENTENCE = ['the', 'cat', 'sat', 'on', 'the', 'mat'] as const;

/** First 8 numbers of each GloVe vector (rounded to 3 decimals in the source file). */
export const GLOVE8: Readonly<Record<(typeof SENTENCE)[number], readonly number[]>> = {
	the: [0.047, 0.213, -0.007, -0.459, -0.036, 0.236, -0.288, 0.215],
	cat: [-0.294, 0.332, -0.047, -0.122, 0.072, -0.234, -0.062, -0.004],
	sat: [-0.6, 0.04, 0.566, -0.662, -0.128, -0.117, 0.263, -0.822],
	on: [0.127, 0.211, -0.19, 0.177, -0.118, 0.243, -0.347, -0.323],
	mat: [0.155, -0.486, -0.181, -0.903, -0.114, -0.761, -0.358, -0.141],
};
/** Width of a full GloVe vector (the drawn tensors are its first 8). */
export const GLOVE_DIMS = 300;

/** Two joints (shoulder, elbow) over four frames: rotation x, y, z in degrees, the channels of an animation curve. */
export const ANIM: readonly (readonly (readonly number[])[])[] = [
	[[0, 0, 0], [0, 0, 0]],
	[[0, 10, 25], [-30, 0, 0]],
	[[0, 15, 40], [-65, 0, 0]],
	[[0, 10, 25], [-30, 0, 0]],
];

/** The four corners of a unit quad lying on the floor (x, y, z per vertex): a vertex buffer N x 3. */
export const QUAD: readonly (readonly number[])[] = [
	[-1, 0, -1],
	[1, 0, -1],
	[1, 0, 1],
	[-1, 0, 1],
];

/** One loss value (the number a training step makes smaller). */
export const LOSS = 0.37;
