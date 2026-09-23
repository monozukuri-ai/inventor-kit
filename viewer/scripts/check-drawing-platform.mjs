// Installed-wheel SVG checks in Chromium. This does not qualify native fonts
// or paper units, and deliberately does not depend on 3D/WebGL screenshots.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '@playwright/test';

const [python, corpus, directory] = process.argv.slice(2);
assert(python && corpus && directory, 'python, corpus and output directory required');
execFileSync(python, ['-I', '-c', `
import hashlib, json, math, sys
from pathlib import Path
import inventor_kit as ik
from inventor_kit.drawing_output import render_svg
assert Path(ik.__file__).resolve().is_relative_to(Path(sys.prefix).resolve())
root=Path(sys.argv[2]); root.mkdir()
rows=[]
for filename in ('SampleBg.idw', '_Fishing Rod Assembly.idw'):
    doc=ik.read_drawing_file(Path(sys.argv[1])/filename)
    for sheet in doc.sheets:
        output=root/('sheet-'+str(len(rows))+'.svg')
        report=doc.export_svg(output,sheet_id=sheet.id,allow_partial=True)
        assert report['export']['sha256']==hashlib.sha256(output.read_bytes()).hexdigest()
        available={image.reference for image in doc.images if image.data is not None}
        used={i.geometry['reference'] for i in sheet.items if i.geometry['kind']=='image'}
        missing=sum(i.geometry['kind']=='image' and i.geometry['reference'] not in available for i in sheet.items)
        rows.append(dict(file=output.name,source_sha256=doc.source_sha256,sheet_id=sheet.id,
            items=len(sheet.items),rendered_items=len(sheet.items)-missing,images=len(used & available),size=list(sheet.size_in_source_units)))
style=dict(width=.025,rgba=[0,0,0,1],dash=[.5,.15,.05,.15],unresolved=[])
font=dict(family='Unavailable qualification font',height_candidate=.6,weight_candidate=400,flags=0,width_factor=1)
text=dict(kind='text',text='日本語 寸法 ±90°',position=[1,7,0],direction=[1,0,0],up=[0,1,0],raw_flags=9,font=font)
symbol=dict(text,text='n',position=[1,5,0],font=dict(font,family='AIGDT'))
curve=dict(kind='curve',center=[6,3,0],u=[2,1,0],v=[.5,1.5,0],start=0,end=2*math.pi)
sheet=dict(id='synthetic-platform',name='Synthetic font and dash control',status='experimental_partial',size_in_source_units=[10,10],
    items=[dict(id=str(i),geometry=g,style=style) for i,g in enumerate((text,symbol,curve))])
(root/'synthetic.svg').write_text(render_svg(sheet,'a'*64,{}),encoding='utf-8')
(root/'inputs.json').write_text(json.dumps(rows),encoding='utf-8')
`, corpus, directory], { stdio: 'inherit' });

const inputs = JSON.parse(await readFile(path.join(directory, 'inputs.json'), 'utf8'));
assert.equal(inputs.length, 5);
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
const results = [];
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on('pageerror', error => errors.push(String(error)));
  const remote = [];
  await page.route(/^https?:/, route => { remote.push(route.request().url()); return route.abort(); });
  for (const input of [...inputs, { file: 'synthetic.svg', size: [10, 10] }]) {
    // An HTML host mirrors the Viewer mounting path and avoids Chromium's
    // standalone SVG screenshot failures after font/layout inspection.
    const svgText = await readFile(path.join(directory, input.file), 'utf8');
    await page.setContent('<!doctype html><meta charset="utf-8"><style>body{margin:0}</style>' +
      svgText.replace(/^<\?xml[^?]*\?>\s*/, ''));
    await page.evaluate(() => document.fonts.ready);
    const result = await page.evaluate(() => {
      const svg = document.querySelector('svg');
      const items = [...document.querySelectorAll('[data-item-id]')];
      const boxes = items.map(item => { const b = item.getBBox(); return [b.x, b.y, b.width, b.height]; });
      return { source: svg.getAttribute('data-source-sha256'), sheet: svg.getAttribute('data-sheet-id'),
        viewBox: svg.getAttribute('viewBox'), items: items.length, boxes,
        imageCount: document.querySelectorAll('defs image').length,
        texts: [...document.querySelectorAll('text')].map(t => t.textContent),
        dash: document.querySelector('path')?.getAttribute('stroke-dasharray') };
    });
    assert.equal(result.viewBox, `0 0 ${input.size.join(' ')}`);
    assert(result.boxes.length > 0 && result.boxes.every(b => b.every(Number.isFinite)));
    assert(result.boxes.some(b => b[2] > 0 && b[3] > 0));
    // Reject the enormous browser-recovered arcs this renderer previously hit.
    assert(result.boxes.every(b => b.every(v => Math.abs(v) < 10000)));
    if (input.sheet_id) {
      assert.equal(result.source, input.source_sha256);
      assert.equal(result.sheet, input.sheet_id);
      // A missing saved image is reported as an omission, without a SVG node.
      assert.equal(result.items, input.rendered_items);
      assert.equal(result.imageCount, input.images);
    } else {
      assert.deepEqual(result.texts, ['日本語 寸法 ±90°', '⌀']);
      assert.equal(result.dash, '0.5 0.15 0.05 0.15');
      const cdp = await page.context().newCDPSession(page);
      await cdp.send('DOM.enable');
      await cdp.send('CSS.enable');
      const { root } = await cdp.send('DOM.getDocument');
      const { nodeIds } = await cdp.send('DOM.querySelectorAll', { nodeId: root.nodeId, selector: 'text' });
      result.fonts = [];
      for (const nodeId of nodeIds) {
        const { fonts } = await cdp.send('CSS.getPlatformFontsForNode', { nodeId });
        assert(fonts.length > 0 && fonts.some(f => f.glyphCount > 0));
        assert(fonts.every(f => !/lastresort/i.test(f.familyName)), 'missing platform glyph font');
        result.fonts.push(fonts);
      }
      assert(result.fonts[0].some(f => /NotoSansCJKjp|YuGothic|Meiryo|Hiragino|IPAP?Gothic/i.test(f.postScriptName)),
        'Japanese control needs an actual Japanese font, not a generic CJK/tofu fallback');
      await cdp.detach();
    }
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await page.screenshot({ path: path.join(directory, input.file + '.png'), fullPage: true });
    delete result.boxes;
    delete result.texts;
    results.push({ file: input.file, ...result });
  }
  assert.deepEqual(errors, []);
  assert.deepEqual(remote, []);
  await writeFile(path.join(directory, 'browser.json'), JSON.stringify({ status: 'passed',
    engine: 'chromium', version: browser.version(), platform: process.platform, architecture: process.arch,
    installed_wheel: true, real_sheets: inputs.length, synthetic_controls: 1, results,
    physical_scale_verified: false, native_font_fidelity_verified: false }, null, 2) + '\n');
} finally {
  await browser.close();
}
