import { defineCollection } from 'astro:content';
import { docsLoader, i18nLoader } from '@astrojs/starlight/loaders';
import { docsSchema, i18nSchema } from '@astrojs/starlight/schema';

export const collections = {
	docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
	/*
	 * Starlight reads UI-string overrides with getCollection('i18n') on every page. An undeclared or empty collection makes
	 * Astro 7 log "The collection "i18n" does not exist or is empty" on each build: Starlight mutes console.warn around the
	 * query (utils/translations.js), but Astro 7's node logger writes to process.stderr. So the collection is declared the
	 * documented way, with the default-locale file src/content/i18n/en.json ({} = Starlight's built-in English strings;
	 * English UI-string overrides go there).
	 */
	i18n: defineCollection({ loader: i18nLoader(), schema: i18nSchema() }),
};
