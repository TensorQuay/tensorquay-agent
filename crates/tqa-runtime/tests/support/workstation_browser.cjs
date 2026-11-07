const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const { chromium } = require('playwright');

async function checkBrowser() {
  assert.equal(process.getuid(), 1000);
  assert.equal(process.getgid(), 1000);
  const home = process.env.HOME;
  assert.equal(home, '/workspace/.tqa/home');
  await fs.writeFile('/workspace/browser.html', `<!doctype html>
    <title>Offline workstation</title><h1>Local browser check</h1>
    <button onclick="document.querySelector('output').textContent='42'">Calculate</button><output></output>`);
  const context = await chromium.launchPersistentContext(`${home}/chromium-profile`, {
    executablePath: '/usr/bin/chromium',
    chromiumSandbox: false,
    headless: true,
    viewport: { width: 640, height: 480 },
    downloadsPath: `${home}/browser-downloads`,
    args: [`--disk-cache-dir=${home}/.cache/chromium`],
  });
  try {
    const page = await context.newPage();
    await page.goto('file:///workspace/browser.html');
    assert.equal(await page.title(), 'Offline workstation');
    await page.getByRole('button', { name: 'Calculate' }).click();
    assert.equal(await page.locator('output').textContent(), '42');
    await page.screenshot({ path: '/workspace/browser.png' });
    const png = await fs.readFile('/workspace/browser.png');
    assert.equal(png.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
    assert.equal(png.readUInt32BE(16), 640);
    assert.equal(png.readUInt32BE(20), 480);
    assert.ok(png.length > 1000);
    assert.equal((await fs.stat('/workspace/browser.png')).uid, 1000);
    assert.ok((await fs.stat(`${home}/chromium-profile`)).isDirectory());
    console.log(`Chromium ${context.browser().version()}: local HTML, JavaScript and 640x480 PNG passed`);
  } finally {
    await context.close();
  }
}

checkBrowser().catch(error => { console.error(error); process.exitCode = 1; });
