/** 手动接入的 Agent（Trae CN 等）：配置存在它的云端账号里，Codex++ 不改它的文件，
 * 这里给出要在它界面里填的地址、API Key 和模型 ID，每项都能复制。 */
import { useState } from "react";
import { Check, Copy, X } from "lucide-react";
import { t, tf } from "./i18n";
import { appConfirm } from "./app-dialog";
import { useEscapeLayer } from "./use-escape-layer";
import { MANUAL_WIRE_LABELS, maskApiKey, type GatewayAgentId, type GatewayAgentView, type GatewayPayload } from "./gateway-model";
import type { Run } from "./gateway-ui";

/** 旧 Key 立即失效，要回应用里逐个改：换之前先确认。网关页的「更换 token」也用它。 */
export async function confirmRegenerate(agentName: string): Promise<boolean> {
  return appConfirm(
    tf("旧 Key 会立即失效。之后要在 {0} 里把每个自定义模型的 API Key 都换成新的，否则请求会被拒绝（401）。", [agentName]),
    { title: t("重新生成 API Key？"), confirmText: t("重新生成"), danger: true },
  );
}

function CopyButton({ copied, label, onCopy }: { copied: boolean; label: string; onCopy: () => void }) {
  return (
    <button aria-label={label} className="gw-icon-button" onClick={onCopy} title={label} type="button">
      {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}

export function ManualGuideDialog({ agent, payload, busy, run, onClose }: {
  agent: GatewayAgentView;
  payload: GatewayPayload | null;
  busy: string | null;
  run: Run;
  onClose: () => void;
}) {
  useEscapeLayer(true, onClose);
  const [copied, setCopied] = useState<string | null>(null);
  const guide = agent.manual;
  if (!guide) return null;
  const token = payload?.gateway.bindings[agent.agent as GatewayAgentId]?.token ?? "";
  const copy = (key: string, text: string) => {
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(key);
      window.setTimeout(() => setCopied((current) => (current === key ? null : current)), 1500);
    }).catch(() => undefined);
  };
  const regenerate = async () => {
    if (await confirmRegenerate(agent.name)) {
      await run(`rotate:${agent.agent}`, "rotate_gateway_token", { agent: agent.agent }, t("重新生成 API Key"));
    }
  };
  return (
    <div className="gw-modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div aria-label={tf("{0} 接入指引", [agent.name])} className="gw-modal gw-manual-guide" role="dialog">
        <header>
          <strong>{tf("{0} 接入指引", [agent.name])}</strong>
          <button aria-label={t("关闭")} className="gw-icon-button" onClick={onClose} type="button"><X className="h-4 w-4" /></button>
        </header>
        <div className="gw-modal-body">
          <p className="gw-manual-path">{tf("在 {0} 里打开：{1}", [agent.name, t(guide.menuPath)])}</p>
          {!agent.enabled ? <p className="gw-manual-off">{t("开关关着时网关会拒绝它的请求（503）；API Key 保留，打开后即可继续用。")}</p> : null}
          <section className="gw-manual-section">
            <h4>{t("API 格式与请求地址")}</h4>
            {guide.endpoints.map((endpoint) => (
              <div className="gw-manual-line" key={endpoint.wire}>
                <span>{MANUAL_WIRE_LABELS[endpoint.wire]}</span>
                <code>{endpoint.baseUrl}</code>
                <CopyButton copied={copied === `url:${endpoint.wire}`} label={t("复制地址")} onCopy={() => copy(`url:${endpoint.wire}`, endpoint.baseUrl)} />
              </div>
            ))}
          </section>
          <section className="gw-manual-section">
            <h4>{t("API 密钥")}</h4>
            {token ? (
              <div className="gw-manual-line">
                <code>{maskApiKey(token)}</code>
                <CopyButton copied={copied === "key"} label={t("复制 API Key")} onCopy={() => copy("key", token)} />
                <button className="gw-text-button" disabled={busy !== null} onClick={() => void regenerate()} type="button">{t("重新生成")}</button>
              </div>
            ) : <p className="gw-manual-hint">{t("打开右侧开关后生成 API Key")}</p>}
          </section>
          <section className="gw-manual-section">
            <h4>
              {tf("模型 ID（{0}）", [guide.models.length])}
              {guide.models.length ? (
                <button className="gw-text-button" onClick={() => copy("models", guide.models.map((model) => model.id).join("\n"))} type="button">
                  {copied === "models" ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                  {t("复制全部模型 ID")}
                </button>
              ) : null}
            </h4>
            {guide.models.length ? (
              <div className="gw-manual-models">
                {guide.models.map((model) => (
                  <div className="gw-manual-line" key={model.id}>
                    <code>{model.id}</code>
                    {model.displayName !== model.id ? <small>{model.displayName}</small> : null}
                    <CopyButton copied={copied === `model:${model.id}`} label={t("复制模型 ID")} onCopy={() => copy(`model:${model.id}`, model.id)} />
                  </div>
                ))}
              </div>
            ) : <p className="gw-manual-hint">{t("供应商池里还没有模型，先去添加供应商。")}</p>}
          </section>
          <ul className="gw-manual-notes">
            {guide.notes.map((note) => <li key={note}>{t(note)}</li>)}
          </ul>
        </div>
      </div>
    </div>
  );
}
