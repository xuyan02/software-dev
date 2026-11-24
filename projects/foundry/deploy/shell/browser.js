#!/usr/bin/env node
// Fixed Playwright harness used by the Foundry `browser` tool.
// Usage: foundry-browser '<json array of actions>'
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const ARTIFACT_DIR = '/tmp/foundry-artifacts';

function safeName(name, fallback) {
  const value = (name || fallback).replace(/[^A-Za-z0-9._-]/g, '-');
  return value || fallback;
}

(async () => {
  const actions = JSON.parse(process.argv[2] || '[]');
  if (!Array.isArray(actions) || actions.length === 0) {
    throw new Error('actions must be a non-empty array');
  }
  fs.mkdirSync(ARTIFACT_DIR, { recursive: true });
  const browser = await chromium.launch({
    headless: true,
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  const output = [];
  const screenshots = [];
  let shotIndex = 0;
  try {
    for (const action of actions) {
      switch (action.type) {
        case 'goto': {
          await page.goto(action.url, { waitUntil: 'domcontentloaded', timeout: 30000 });
          await page.waitForTimeout(200);
          output.push(`goto ${action.url}`);
          break;
        }
        case 'click': {
          await page.click(action.selector, { timeout: 15000 });
          output.push(`click ${action.selector}`);
          break;
        }
        case 'fill': {
          await page.fill(action.selector, action.value || '', { timeout: 15000 });
          output.push(`fill ${action.selector}`);
          break;
        }
        case 'press': {
          if (action.selector) {
            await page.press(action.selector, action.key, { timeout: 15000 });
            output.push(`press ${action.key} on ${action.selector}`);
          } else {
            await page.keyboard.press(action.key);
            output.push(`press ${action.key}`);
          }
          break;
        }
        case 'wait': {
          const ms = Math.min(Math.max(Number(action.ms || 500), 0), 60000);
          await page.waitForTimeout(ms);
          output.push(`wait ${ms}ms`);
          break;
        }
        case 'text': {
          const selector = action.selector || 'body';
          const text = (await page.textContent(selector, { timeout: 15000 })) || '';
          output.push(`text ${selector}:\n${text.slice(0, 8000)}`);
          break;
        }
        case 'screenshot': {
          shotIndex += 1;
          const name = safeName(action.name, `screenshot-${shotIndex}`) + '.png';
          const file = path.join(ARTIFACT_DIR, name);
          await page.screenshot({ path: file, fullPage: false });
          screenshots.push(file);
          output.push(`screenshot ${file}`);
          break;
        }
        default:
          throw new Error(`unknown action type: ${action.type}`);
      }
    }
  } finally {
    await browser.close();
  }
  process.stdout.write(JSON.stringify({ output, screenshots }));
})().catch((error) => {
  process.stderr.write(String(error && error.stack ? error.stack : error));
  process.exit(1);
});
