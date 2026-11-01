// Writes a small PDF for the viewer's test: three pages (the last one
// landscape), standard fonts that are not embedded (drawn in the substitute
// faces), a filled box, and bookmarks. usage: node sample-pdf.mjs <out.pdf>
import fs from 'node:fs';

const pages = [
  { w: 595, h: 842, title: 'Non-Disclosure Agreement', lines: [
    'between Acme Oy and us, signed 2026-10-01.',
    '1. Confidential information means all information disclosed.',
    '2. The receiving party keeps it confidential for five years.'] },
  { w: 595, h: 842, title: 'Appendix A', lines: ['Contact persons and the confidential project names.'] },
  { w: 842, h: 595, title: 'Appendix B: schedule', lines: ['A landscape page with a table of dates.'] },
];

const objs = [];
const add = (s) => { objs.push(s); return objs.length; };
const esc = (s) => s.replace(/[\\()]/g, (c) => '\\' + c);

const catalog = add('');
const pagesObj = add('');
const fontH = add('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>');
const fontT = add('<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>');
const kids = [];
for (const p of pages) {
  let c = `0.92 0.95 1 rg 50 ${p.h - 190} ${p.w - 100} 120 re f\n`;
  c += `0.1 0.3 0.7 RG 3 w 50 ${p.h - 205} m ${p.w - 50} ${p.h - 205} l S\n`;
  c += `0 g BT /F1 26 Tf 70 ${p.h - 120} Td (${esc(p.title)}) Tj ET\n`;
  let y = p.h - 160;
  for (const l of p.lines) {
    c += `BT /F2 14 Tf 70 ${y} Td (${esc(l)}) Tj ET\n`;
    y -= 60;
  }
  const content = add(`<< /Length ${Buffer.byteLength(c)} >>\nstream\n${c}endstream`);
  kids.push(add(`<< /Type /Page /Parent ${pagesObj} 0 R /MediaBox [0 0 ${p.w} ${p.h}] ` +
    `/Resources << /Font << /F1 ${fontH} 0 R /F2 ${fontT} 0 R >> >> /Contents ${content} 0 R >>`));
}
// bookmarks: one per page
const outlines = add('');
const items = pages.map(() => add(''));
items.forEach((n, i) => {
  let d = `<< /Title (${esc(pages[i].title)}) /Parent ${outlines} 0 R /Dest [${kids[i]} 0 R /Fit]`;
  if (i > 0) d += ` /Prev ${items[i - 1]} 0 R`;
  if (i + 1 < items.length) d += ` /Next ${items[i + 1]} 0 R`;
  objs[n - 1] = d + ' >>';
});
objs[outlines - 1] = `<< /Type /Outlines /First ${items[0]} 0 R /Last ${items[items.length - 1]} 0 R /Count ${items.length} >>`;
objs[catalog - 1] = `<< /Type /Catalog /Pages ${pagesObj} 0 R /Outlines ${outlines} 0 R >>`;
objs[pagesObj - 1] = `<< /Type /Pages /Kids [${kids.map((k) => k + ' 0 R').join(' ')}] /Count ${kids.length} >>`;
const info = add('<< /Title (Acme NDA signed) >>');

let pdf = '%PDF-1.4\n';
const offs = [];
objs.forEach((o, i) => { offs.push(Buffer.byteLength(pdf)); pdf += `${i + 1} 0 obj\n${o}\nendobj\n`; });
const xref = Buffer.byteLength(pdf);
pdf += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`;
for (const o of offs) pdf += String(o).padStart(10, '0') + ' 00000 n \n';
pdf += `trailer\n<< /Size ${objs.length + 1} /Root ${catalog} 0 R /Info ${info} 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
fs.writeFileSync(process.argv[2], pdf);
