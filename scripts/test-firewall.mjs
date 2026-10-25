import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/modules/firewall.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
const calls = [];
const module = { exports: {} };
new Function("require", "module", "exports", compiled)(() => ({ taskInvoke: async (...args) => { calls.push(args); return {}; } }), module, module.exports);
const firewall = module.exports;

test("firewall input rejects non-integer and out-of-range ports", () => {
  for (const port of [1, 30120, 65535]) assert.equal(firewall.validFirewallPort(port), true);
  for (const port of [undefined, null, 0, -1, 65536, 1.5, NaN, Infinity, "30120"]) assert.equal(firewall.validFirewallPort(port), false);
});

test("firewall commands retain typed ports and never request elevation implicitly", async () => {
  const request = { backend: "auto", tcpPort: 30120, udpPort: 30121 };
  await firewall.inspectFirewall(request);
  await firewall.inspectFirewall(request, true);
  await firewall.configureFirewall(request);
  await firewall.removeFirewall(request);
  await firewall.getFirewallPorts("fixture/txData", "default");
  assert.deepEqual(calls, [
    ["inspect_fivem_firewall", { request, elevated: false }],
    ["inspect_fivem_firewall", { request, elevated: true }],
    ["configure_fivem_firewall", { request }],
    ["remove_fivem_firewall", { request }],
    ["get_fivem_firewall_ports", { request: { txDataPath: "fixture/txData", profile: "default" } }],
  ]);
});
