import { expect, test, beforeEach, afterEach } from 'bun:test';
import '../../src/i18n/ui';
import { lessonByPath, lessons, loc } from '../../src/lib/course';
import { localizedUrl, asLocale } from '../../src/lib/course-rules.mjs';
import { resolveCourseLink, fileLocale } from '../../src/lib/remark-lesson-links.mjs';
import { ROWS2, ROWS3, ROWSLC, ROWSPOLAR, ROWSSIGN, ROWSLAMP } from '../../src/viz/demo/engine.rows';
import { chipLines } from '../../src/viz/scene/chips';

let oldBase: string | undefined;
beforeEach(()=>{oldBase=process.env.BASE_URL;process.env.BASE_URL='/math-course/';});
afterEach(()=>{if(oldBase===undefined)delete process.env.BASE_URL;else process.env.BASE_URL=oldBase;});

test('locale URLs preserve deployment base and semantic course identity', () => {
 for (const l of lessons) {
  expect(localizedUrl('/math-course/', l.path, 'ru')).toBe('/math-course/ru/' + l.path + '/');
  expect(lessonByPath('ru/' + l.path)?.slug).toBe(l.slug);
  expect(loc(l, 'ru').title).not.toBe(loc(l, 'en').title);
 }
 expect(fileLocale('C:\\course\\src\\content\\docs\\ru\\vectors\\dot-product.mdx')).toBe('ru');
 expect(fileLocale('/course/src/content/docs/vectors/dot-product.mdx')).toBe('en');
 expect(lessonByPath('/math-course/ru/vectors/dot-product/')?.slug).toBe('dot-product');
 expect(localizedUrl('/math-course/', '', 'ru')).toBe('/math-course/ru/');
 expect(() => asLocale('fr')).toThrow();
 expect(resolveCourseLink('lesson:dot-product#formula', '/math-course/', 'ru', 'test')).toBe('/math-course/ru/vectors/dot-product/#formula');
});

test('localized row factories preserve semantic IDs, store keys, ranges and mathematical markup', () => {
 const ids = [
  ['a','b','p','th','r','dot','sqa','par'], ['u','v','phi','dot3','par3'],
  ['a','b','sqa','sqb','corr','sqc'], ['la','lb','lth','lc','a2','b2','corr','c2'],
  ['sa','sb','sa:x','sa:y','sdot'], ['light','nl_raw','N','L','NdotL','bright'],
 ];
 [ROWS2,ROWS3,ROWSLC,ROWSPOLAR,ROWSSIGN,ROWSLAMP].forEach((factory,i)=>{
  const en=factory('en'),ru=factory('ru');
  expect(en.map(r=>r.id)).toEqual(ids[i]);
  expect(ru.map(r=>r.id)).toEqual(ids[i]);
  const contracts=(rows: ReturnType<typeof factory>)=>rows.map(({label,tip,group,value,fmt,visible,...row})=>({...row,tex:tip?.tex,value: value?.toString(),fmt:fmt?.toString(),visible:visible?.toString()}));
  expect(contracts(ru)).toEqual(contracts(en));
  expect(chipLines(ru)).toBeGreaterThan(0);
 });
});
