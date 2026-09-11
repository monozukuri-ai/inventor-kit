import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests', timeout: 150000, workers: 1, fullyParallel: false,
  use: { browserName: 'chromium', viewport: { width: 1440, height: 950 },
    launchOptions: { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] },
    screenshot: 'only-on-failure' },
});
