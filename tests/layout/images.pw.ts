import { expect, test, type Page } from '@playwright/test';

async function expectAligned(page: Page) {
  await expect.poll(async () => page.evaluate(() => {
    const rows: { number: number; gutter: number; text: number }[] = (window as any).alignment();
    if (rows.length < 5 || !rows.some(row => row.number > 4)) return Infinity;
    const baseline = rows[0].gutter - rows[0].text;
    return Math.max(...rows.map(row => Math.abs(row.gutter - row.text - baseline)));
  })).toBeLessThan(1.5);
}

for (const orientation of ['landscape', 'portrait']) test(`${orientation}: line numbers align before and after asynchronous image loading, edits, and wrapping`, async ({ page }, testInfo) => {
  await page.goto('/tests/fixtures/editor-layout.html' + (orientation === 'portrait' ? '?portrait' : ''));
  await expect(page.locator('.note-image-status')).toHaveText('Loading image…');
  const before = await page.locator('.note-image-block').boundingBox();
  expect(before?.height).toBe(230);
  await expect(page.locator('.note-image-area')).toHaveCSS('height', '180px');
  await expect(page.locator('.note-image-caption')).toHaveCSS('height', '32px');
  await expectAligned(page);
  expect(await page.evaluate(() => (window as any).imageLoadCount())).toBe(1);
  await page.evaluate(() => (window as any).finishImage());
  await expect(page.locator('.note-image')).toBeVisible();
  await expectAligned(page);
  expect((await page.locator('.note-image-block').boundingBox())?.height).toBe(before?.height);
  await expect(page.locator('.note-image')).toHaveCSS('object-fit', 'contain');
  await expect(page.locator('.note-image-caption')).toHaveText('Preview caption — entire image visible');
  await page.getByRole('button', { name: 'Annotate image' }).click();
  expect(await page.evaluate(() => (window as any).openedImage.attachmentId)).toBe('./assets/example.png');
  await page.evaluate(() => (window as any).insertAbove());
  await expectAligned(page);
  await page.evaluate(() => (window as any).resizeEditor());
  await expectAligned(page);
  const preview = await page.locator('.note-image-button').boundingBox();
  const editor = await page.locator('#editor').boundingBox();
  expect(preview!.x + preview!.width).toBeLessThanOrEqual(editor!.x + editor!.width);
  await page.screenshot({ path: testInfo.outputPath('image-gutter.png') });
  await page.evaluate(() => (window as any).removeReference());
  await expect(page.locator('.note-image-block')).toHaveCount(0);
  await expectAligned(page);
});

test('line numbers stay aligned when an image cannot be loaded', async ({ page }, testInfo) => {
  await page.goto('/tests/fixtures/editor-layout.html');
  await expect(page.locator('.note-image-status')).toHaveText('Loading image…');
  const before = await page.locator('.note-image-block').boundingBox();
  await expectAligned(page);
  await page.evaluate(() => (window as any).failImage());
  await expect(page.locator('.note-image-status')).toContainText('Image unavailable');
  await page.screenshot({ path: testInfo.outputPath('missing-image.png') });
  expect((await page.locator('.note-image-block').boundingBox())?.height).toBe(before?.height);
  await page.evaluate(() => (window as any).insertAbove());
  await expectAligned(page);
  await page.evaluate(() => (window as any).resizeEditor());
  await expectAligned(page);
});


for (const wrap of ['', '&nowrap', '&nowrap&compact']) test(`short note ${wrap} keeps the blank line below the preview aligned`, async ({ page }) => {
  await page.goto('/tests/fixtures/editor-layout.html?short' + wrap);
  await expect(page.locator('.note-image-status')).toHaveText('Loading image…');
  await expectAligned(page);
  await page.evaluate(() => (window as any).finishImage());
  await expect(page.locator('.note-image')).toBeVisible();
  await expectAligned(page);
});


test('tab activation measures a previously hidden editor', async ({ page }) => {
  await page.goto('/tests/fixtures/editor-layout.html?short&hidden');
  await page.evaluate(() => (window as any).finishImage());
  await page.evaluate(() => (window as any).revealEditor());
  await expect(page.locator('.note-image')).toBeVisible();
  await expectAligned(page);
});

test('CRLF line endings keep lines distinct without overlapping line numbers', async ({ page }) => {
  await page.goto('/tests/fixtures/editor-layout.html?short');
  await page.evaluate(() => {
    const view = (window as any).view;
    const crlfText = '# File editor smoke test\r\n\r\n![image](./assets/example.png)\r\n\r\nExternal clean-buffer reload verified';
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: crlfText } });
  });
  await page.evaluate(() => (window as any).finishImage());
  await expect(page.locator('.note-image')).toBeVisible();
  await expectAligned(page);
  const tops = await page.evaluate(() => {
    return [...document.querySelectorAll('.cm-lineNumbers .cm-gutterElement')]
      .filter(el => /^\d+$/.test(el.textContent || ''))
      .map(el => el.getBoundingClientRect().top);
  });
  for (let i = 1; i < tops.length; i++) {
    expect(tops[i]).toBeGreaterThan(tops[i - 1]);
  }
});
