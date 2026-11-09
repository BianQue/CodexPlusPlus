import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { agentPresentation, manualPresentation } from "./gateway-agent-safety.ts";
import { EN_BACKEND, EN_BACKEND_PATTERNS, EN_PLAIN, EN_TEMPLATE } from "./i18n-en.ts";
import { maskApiKey } from "./gateway-model.ts";
import type { GatewayAgentDiagnostics, GatewayAgentView } from "./gateway-model.ts";

const trae = (seen: boolean, state?: GatewayAgentDiagnostics["state"], enabled = true): GatewayAgentView => ({
  agent: "trae-cn", name: "Trae CN", enabled, baseUrl: "http://127.0.0.1:57321/gw/trae-cn", defaultModel: "", models: [],
  manual: { menuPath: "AI 设置 → 模型 → 添加模型 → 自定义", notes: [], endpoints: [], models: [], seen },
  ...(state ? { diagnostics: {
    state, endpoint: null, expectedEndpoint: "", configEvidence: [], requests: 0, errors: 0, inFlight: 0,
    lastSeenMs: null, recentError: null,
  } } : {}),
});

test("手动接入的状态只看开关和用量：未接入 / 网关未运行 / 等待首个请求 / 已接入", () => {
  assert.deepEqual(manualPresentation(trae(true, undefined, false), true), { badge: "未接入", tone: "off", reconnect: false });
  assert.equal(manualPresentation(trae(true), false).badge, "网关未运行");
  assert.equal(manualPresentation(trae(true, "gatewayStopped"), true).badge, "网关未运行");
  assert.deepEqual(manualPresentation(trae(false, "awaitingTraffic"), true), { badge: "等待首个请求", tone: "pending", reconnect: false });
  assert.deepEqual(manualPresentation(trae(true, "awaitingTraffic"), true), { badge: "已接入", tone: "on", reconnect: false });
});

test("手动接入的徽标不被配置诊断或上游失败覆盖，也不出「重新接入」", () => {
  assert.equal(manualPresentation(trae(false, "configChanged"), true).badge, "等待首个请求");
  assert.equal(manualPresentation(trae(true, "upstreamError"), true).badge, "已接入");
  assert.equal(manualPresentation(trae(false, "configMissing"), true).reconnect, false);
  // 没有 manual 的 Agent 照旧走联合诊断。
  const { manual: _manual, ...plain } = trae(false, "configMissing");
  assert.equal(agentPresentation(plain, true).badge, "配置缺失");
  assert.equal(agentPresentation(trae(false, "configMissing"), true).badge, "等待首个请求");
});

test("API Key 打码显示头尾，短的原样", () => {
  assert.equal(maskApiKey("cpx-gw-trae-cn-0123456789abcdef"), "cpx-gw-tra…cdef");
  assert.equal(maskApiKey("short"), "short");
});

const read = (name: string) => readFileSync(new URL(`./${name}`, import.meta.url), "utf8");

test("手动接入的行用「接入指引」代替模型下拉，关开关不走断开预览，详情不显示改动的配置文件", () => {
  const agents = read("GatewayAgents.tsx");
  const controls = agents.slice(agents.indexOf('<div className="cx-row-controls">'));
  assert.match(controls, /\{manual \? \(\s*<button[^>]*gw-manual-guide-button[\s\S]*?\) : \(\s*<ModelPicker/);
  assert.match(agents, /if \(connected && manual\) \{[\s\S]*?"disable_gateway_binding", \{ agent: agent\.agent \}/, "手动接入关开关不带 previewToken");
  assert.match(agents, /\{agent\.manual \? null : \(\s*<div className="gw-detail-row gw-detail-foot">/);
});

test("重新生成 API Key 先确认；托盘对手动接入只显示状态", () => {
  const guide = read("GatewayManualGuide.tsx");
  assert.match(guide, /if \(await confirmRegenerate\(agent\.name\)\) \{\s*await run\([^)]*"rotate_gateway_token"/);
  const page = read("GatewayPage.tsx");
  assert.match(page, /if \(agent\.manual && !\(await confirmRegenerate\(agent\.name\)\)\) return;/);
  const tray = read("TrayPanel.tsx");
  assert.match(tray, /agent\.manual\s*\? <PanelManualAgentRow/);
});

/** 收集一个文件里 t("…") / tf("…", …) 的中文字面量。 */
function literals(source: string, fn: "t" | "tf"): string[] {
  const re = fn === "t" ? /\bt\("((?:[^"\\]|\\.)*)"/g : /\btf\("((?:[^"\\]|\\.)*)"/g;
  return [...source.matchAll(re)].map((match) => JSON.parse(`"${match[1]}"`) as string);
}

test("接入指引与 Trae 规格里的文案都有英文", () => {
  const translated = (zh: string) => Boolean(EN_PLAIN[zh] ?? EN_BACKEND[zh] ?? EN_TEMPLATE[zh])
    || EN_BACKEND_PATTERNS.some(([re]) => re.test(zh));
  const guide = read("GatewayManualGuide.tsx");
  const missing = [...literals(guide, "t"), ...literals(guide, "tf")].filter((zh) => /[一-鿿]/.test(zh) && !translated(zh));
  assert.deepEqual(missing, []);
  const spec = readFileSync(new URL("../../../crates/codex-plus-core/src/gateway/agents/catalog_manual.rs", import.meta.url), "utf8");
  const specText = [...spec.matchAll(/^\s*(?:menu_path: )?"([^"]*[一-鿿][^"]*)",$/gm)].map((match) => match[1]);
  assert.ok(specText.length >= 4, "菜单路径和三条提醒");
  assert.deepEqual(specText.filter((zh) => !translated(zh)), []);
  for (const message of [
    "Trae CN 已暂停接入：网关对它的请求返回 503，API Key 保留，重新打开即可继续用。",
    "Trae CN 已开启接入，按「接入指引」在 Trae CN 里添加模型。",
    "已更换网关 token。请到 Trae CN 里把每个自定义模型的 API Key 换成新的。",
    "Trae CN 由用户在应用里手动配置，Codex++ 没有改动它的文件",
  ]) assert.ok(translated(message), message);
});
