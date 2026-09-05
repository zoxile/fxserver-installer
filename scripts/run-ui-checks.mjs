import { mkdir, readFile, readdir } from "node:fs/promises";
import { runInThisContext } from "node:vm";
import { chromium } from "playwright";
import { preview } from "vite";

await mkdir("output/playwright", { recursive: true });
const requested = process.argv.slice(2);
const files = requested.length ? requested : (await readdir("scripts")).filter((name) => /^smoke-.*\.js$/.test(name)).sort();
if (files.some((name) => !/^smoke-[a-z-]+\.js$/.test(name))) throw new Error("Use smoke script filenames from scripts/.");
const config = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
const { version } = JSON.parse(await readFile("package.json", "utf8"));
const { betaVersions } = JSON.parse(await readFile("release-policy.json", "utf8"));
const csp = Object.entries(config.app.security.csp).map(([directive, sources]) => `${directive} ${Array.isArray(sources) ? sources.join(" ") : sources}`).join("; ");
const server = await preview({ logLevel: "error", preview: { host: "127.0.0.1", port: 0, headers: { "Content-Security-Policy": csp } } });
let browser;
try {
  const url = server.resolvedUrls.local[0];
  browser = await chromium.launch({ headless: process.env.HEADED !== "1" });
  for (const name of files) {
    console.log(`Running ${name}`);
    const context = await browser.newContext();
    await context.route("**/*", (route) => new URL(route.request().url()).origin === new URL(url).origin ? route.continue() : route.abort());
    const page = await context.newPage();
    page.setDefaultTimeout(15_000);
    await page.addInitScript(() => { window.confirm = () => true; });
    try {
      await page.goto(url);
      const run = runInThisContext(await readFile(`scripts/${name}`, "utf8"), { filename: name });
      const result = await run(page, { version, beta: betaVersions.includes(version) });
      if (result) console.log(result);
    } catch (error) {
      await page.screenshot({ path: `output/playwright/${name}-failure.png`, fullPage: true }).catch(() => {});
      throw error;
    } finally { await context.close(); }
  }
} finally {
  await browser?.close();
  await new Promise((resolve, reject) => server.httpServer.close((error) => error ? reject(error) : resolve()));
}
