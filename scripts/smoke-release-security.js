async (page, { version, beta }) => {
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => {
    const label = document.querySelector('nav[aria-label="Workspace navigation"] button[title="Home"] > span:last-child');
    return label && getComputedStyle(label).opacity === "1";
  });
  if (beta) {
    await page.getByText(`${version} Beta`, { exact: true }).waitFor();
    await page.getByText("This version is not fully tested on live servers or databases.", { exact: false }).waitFor();
    await page.screenshot({ path: "output/playwright/release-beta-warning.png", fullPage: true, animations: "disabled" });
    await page.getByRole("button", { name: `Dismiss ${version} Beta`, exact: true }).click();
    if (await page.getByText(`${version} Beta`, { exact: true }).count()) throw new Error("Beta notice could not be dismissed");
    await page.getByText("Beta", { exact: true }).waitFor();
  } else if (await page.getByText("Beta", { exact: true }).count()) {
    throw new Error("Stable release incorrectly marked as beta");
  }
  await page.evaluate(() => {
    window.policyViolations = [];
    document.addEventListener("securitypolicyviolation", (event) => {
      window.policyViolations.push({ directive: event.effectiveDirective, uri: event.blockedURI });
    });
    const script = document.createElement("script");
    script.textContent = "window.untrustedScriptRan = true";
    document.head.append(script);
    const external = document.createElement("script");
    external.src = "https://example.invalid/untrusted.js";
    document.head.append(external);
  });
  await page.waitForFunction(() => {
    const scripts = window.policyViolations.filter((event) => event.directive === "script-src-elem");
    return scripts.some((event) => event.uri === "inline") && scripts.some((event) => event.uri.startsWith("https://example.invalid"));
  });
  if (await page.evaluate(() => window.untrustedScriptRan)) throw new Error("CSP allowed an injected inline script");
  await page.screenshot({ path: "output/playwright/release-beta.png", fullPage: true, animations: "disabled" });
  console.log("PASS: version-specific beta notice, dismissal with persistent badge, inline and remote script restrictions.");
}
