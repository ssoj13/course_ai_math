/** Links to the course notebooks (`notebooks/<name>.ipynb` of the course repo), shared by ColabBadge and PyCell. */

const REPO = 'ssoj13/course_ai_math';

/** `owner/repo/blob/main/notebooks/<name>.ipynb`; throws unless `notebook` is a bare file name without extension. */
export function notebookPath(notebook: string): string {
	if (!/^[\w.-]+$/.test(notebook)) {
		throw new Error(`notebook must be a bare file name, got ${JSON.stringify(notebook)}`);
	}
	return `${REPO}/blob/main/notebooks/${notebook}.ipynb`;
}

export const colabUrl = (notebook: string) => `https://colab.research.google.com/github/${notebookPath(notebook)}`;
export const githubUrl = (notebook: string) => `https://github.com/${notebookPath(notebook)}`;
