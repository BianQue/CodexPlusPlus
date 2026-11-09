//! 通用 Agent 接入：Codex / Claude Code / Grok 之外的 Agent。
//!
//! 每个 Agent 是一条 [`AgentSpec`]：怎么判断本机装了、配置文件在哪、接入时往配置里写什么。
//! 写入函数只在 [`FileSet`] 上改内存里的文档；落盘、接入前的存档、断开时的还原、配置被别的工具
//! 改掉的检测都由 [`connect`] 统一处理（与 Claude / Grok 的做法一致：只还原网关改过的键）。
//!
//! 请求入口是 `/gw/<id>/...`，网关同时接受 Chat Completions、Responses、Anthropic Messages 和
//! Gemini 四种入站协议（见 [`crate::gateway::server`]），Agent 用哪种由它自己的配置决定，
//! 规格里的 [`AgentWire`] 只是写配置时选协议的依据。
//!
//! 新增 Agent：在 `catalog_*.rs` 里写一条规格并登记到该文件的 `SPECS`；
//! 写配置时供应商名统一用 [`PROVIDER_ID`]，地址用 [`Wiring`] 给的，不要硬编码端口。
//! 配置不在本地文件里（存在云端账号）的应用用手动接入：规格带 [`AgentSpec::manual`]，
//! Codex++ 只签发 token、路由、记用量，用户照界面上的接入指引在应用里自己填。

pub mod catalog_apps;
pub mod catalog_cli;
pub mod catalog_core;
pub mod catalog_manual;
pub mod catalog_tools;
pub mod connect;
pub mod env;
pub mod files;
mod jsonc_edit;

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Serialize;

pub use env::{AgentEnv, Os};
pub use files::{FileFormat, FileSet, YamlDoc, object_entry};

/// 写进各 Agent 配置里的供应商 id / 名字。
pub const PROVIDER_ID: &str = "codexplus";
pub const PROVIDER_NAME: &str = "Codex++";

/// Agent 发给网关用的协议（写配置时选用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentWire {
    /// OpenAI Chat Completions（`<root>/v1/chat/completions`）。
    Chat,
    /// OpenAI Responses（`<root>/v1/responses`）。
    Responses,
    /// Anthropic Messages（`<root>/v1/messages`；Agent 填根地址 `<root>`）。
    Anthropic,
    /// Gemini generateContent（`<root>/v1beta/models/...`；Agent 填根地址 `<root>`）。
    Gemini,
}

impl AgentWire {
    /// Agent 里该填的 Base URL：OpenAI 两种带 `/v1`，Anthropic / Gemini 填根地址（客户端自己拼 `/v1/...`）。
    pub fn base_url(self, root: &str) -> String {
        match self {
            AgentWire::Chat | AgentWire::Responses => format!("{root}/v1"),
            AgentWire::Anthropic | AgentWire::Gemini => root.to_string(),
        }
    }
}

/// 手动接入：配置存在 Agent 自己的云端账号里，没有可写的文件。Codex++ 只给出地址、token 和模型，
/// 用户在 Agent 界面里自己填；路由、token、用量与写文件的通用 Agent 完全一样（`/gw/<id>`）。
pub struct ManualConnect {
    /// 在 Agent 里添加模型的菜单路径，如 `AI 设置 → 模型 → 添加模型 → 自定义`。
    pub menu_path: &'static str,
    /// 接入指引里逐条显示的提醒。
    pub notes: &'static [&'static str],
    /// 接入指引列出的协议，按显示顺序。
    pub wires: &'static [AgentWire],
    pub install: InstallPaths,
}

/// 手动接入的 Agent 的安装位置：只看存在与否，不读内容。
pub struct InstallPaths {
    /// `%LOCALAPPDATA%` 下的相对路径，如 `Programs/Trae CN/Trae CN.exe`。
    pub windows_local_app_data: &'static [&'static str],
    /// `.app` 名，在 `~/Applications` 与 `/Applications` 下找。
    pub mac_apps: &'static [&'static str],
}

/// 写进 Agent 配置的一个模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry {
    /// 网关里的模型名：`<供应商 id>/<模型>`、裸模型名或 `group/<组 id>`。
    pub id: String,
    /// 当前显示名，模型组改名不改变请求 ID。
    pub display_name: String,
    /// 上下文窗口（供应商池里写了的才有）。
    pub context_window: Option<u64>,
    /// 思考档位（由低到高，模型组取成员的并集）；没有资料时为空。
    pub reasoning_levels: Vec<String>,
}

/// 接入一个 Agent 时写配置需要的全部信息。
#[derive(Debug, Clone)]
pub struct Wiring {
    pub agent: String,
    /// 网关根地址：`http://127.0.0.1:<端口>/gw/<id>`（不含 `/v1`）。
    pub root: String,
    /// 网关签发给这个 Agent 的本地 token。
    pub token: String,
    /// 主模型。
    pub default_model: String,
    /// 轻量任务用的模型（起名、摘要等）；没单独设置时同主模型。
    pub small_model: String,
    /// 暴露给 Agent 的模型（主模型排第一）。
    pub models: Vec<ModelEntry>,
}

impl Wiring {
    /// OpenAI 兼容地址：`<root>/v1`。
    pub fn v1(&self) -> String {
        format!("{}/v1", self.root)
    }

    pub fn model_ids(&self) -> impl Iterator<Item = &str> {
        self.models.iter().map(|model| model.id.as_str())
    }

    /// 模型的上下文窗口（供应商池里写了的才有）。
    pub fn window(&self, model: &str) -> Option<u64> {
        self.models
            .iter()
            .find(|entry| entry.id == model)
            .and_then(|entry| entry.context_window)
    }
}

/// 一个通用 Agent 的规格。
pub struct AgentSpec {
    /// 网关路径与绑定的键：`/gw/<id>`，只用小写字母、数字和 `-`。
    pub id: &'static str,
    pub name: &'static str,
    /// 探测版本用的命令（`<命令> --version`）；没有命令行的为空。
    pub binary: &'static str,
    pub wire: AgentWire,
    /// 本机装了（或配置过）这个 Agent。
    pub detect: fn(&AgentEnv) -> bool,
    /// 界面上展示的主配置文件。
    pub config_path: fn(&AgentEnv) -> PathBuf,
    /// 往配置里写入网关。
    pub write: fn(&AgentEnv, &Wiring, &mut FileSet) -> anyhow::Result<()>,
    /// 在 Agent 里怎么切模型、接入后要不要重启等提示；没有为空。
    pub hint: &'static str,
    /// 手动接入（见 [`ManualConnect`]）：不写、不读、不还原任何 Agent 文件，也不检测漂移；
    /// 这时 `config_path` 返回空路径，`write` 为空操作。None 为写配置文件的接入。
    pub manual: Option<&'static ManualConnect>,
}

impl std::fmt::Debug for AgentSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSpec").field("id", &self.id).finish()
    }
}

/// 全部通用 Agent，按界面上的顺序。
pub fn all() -> &'static [&'static AgentSpec] {
    static ALL: OnceLock<Vec<&'static AgentSpec>> = OnceLock::new();
    ALL.get_or_init(|| {
        catalog_core::SPECS
            .iter()
            .chain(catalog_cli::SPECS)
            .chain(catalog_tools::SPECS)
            .chain(catalog_apps::SPECS)
            .chain(catalog_manual::SPECS)
            .collect()
    })
}

pub fn spec(id: &str) -> Option<&'static AgentSpec> {
    all().iter().copied().find(|spec| spec.id == id)
}

/// 是不是通用 Agent（`ToolId::Unknown` 里登记过的 id）。
pub fn is_generic(agent: &crate::tools::ToolId) -> bool {
    matches!(agent, crate::tools::ToolId::Unknown(id) if spec(id).is_some())
}

/// 是不是手动接入的通用 Agent（见 [`ManualConnect`]）。
pub fn is_manual(agent: &crate::tools::ToolId) -> bool {
    matches!(agent, crate::tools::ToolId::Unknown(id)
        if spec(id).is_some_and(|spec| spec.manual.is_some()))
}

/// 手动接入的 Agent 装在本机：只检查安装位置存在，不读任何内容。
pub fn installed_at(env: &AgentEnv, paths: &InstallPaths) -> bool {
    match env.os {
        Os::Windows => paths
            .windows_local_app_data
            .iter()
            .any(|relative| env.local_app_data().join(relative).is_file()),
        Os::Mac => catalog_apps::mac_app(env, paths.mac_apps),
        Os::Linux => false,
    }
}

/// 本机真的装了（或配置过）这个 Agent：规格自己的探测通过，并且有它的命令、它的配置文件，
/// 或者配置所在的目录是 Agent 自己建的——目录里只剩 `logs`、`cache` 这类运行时残留（卸载后留下的）
/// 或别的工具顺手建的 `skills` 时不算。手动接入的没有配置文件，只认探测（看的就是安装位置）。
pub fn installed(spec: &AgentSpec, env: &AgentEnv) -> bool {
    if !(spec.detect)(env) {
        return false;
    }
    if spec.manual.is_some() {
        return true;
    }
    if env.has_binary(spec.binary) {
        return true;
    }
    let config = (spec.config_path)(env);
    config.is_file() || config.parent().is_some_and(agent_dir)
}

/// 目录里有 Agent 自己写的东西（不只是残留目录、`skills` 和 `.DS_Store`）。
pub fn agent_dir(path: &std::path::Path) -> bool {
    const LEFTOVER: &[&str] = &["logs", "log", "debug", "diagnostics", "cache"];
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        name != "skills" && name != ".ds_store" && !(is_dir && LEFTOVER.contains(&name.as_str()))
    })
}

/// 常用的探测：用户目录下某个路径存在。
pub fn home_exists(env: &AgentEnv, relative: &str) -> bool {
    env.home_path(relative).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leftover_only_folder_is_not_an_install() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".agent");
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        std::fs::create_dir_all(dir.join("diagnostics")).unwrap();
        std::fs::create_dir_all(dir.join("skills")).unwrap();
        assert!(!agent_dir(&dir));
        std::fs::write(dir.join("settings.json"), "{}").unwrap();
        assert!(agent_dir(&dir));
        assert!(!agent_dir(&temp.path().join("missing")));
    }

    #[test]
    fn wire_base_url_matches_what_clients_append() {
        let root = "http://127.0.0.1:57321/gw/x";
        assert_eq!(AgentWire::Chat.base_url(root), format!("{root}/v1"));
        assert_eq!(AgentWire::Responses.base_url(root), format!("{root}/v1"));
        assert_eq!(AgentWire::Anthropic.base_url(root), root);
        assert_eq!(AgentWire::Gemini.base_url(root), root);
    }

    #[test]
    fn installed_at_only_looks_at_install_locations() {
        const PATHS: InstallPaths = InstallPaths {
            windows_local_app_data: &["Programs/Demo/Demo.exe"],
            mac_apps: &["Demo.app"],
        };
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let local = temp.path().join("Local");
        let windows = AgentEnv::at(home.clone())
            .with_os(Os::Windows)
            .with_var("LOCALAPPDATA", local.to_str().unwrap());
        assert!(!installed_at(&windows, &PATHS));
        let exe = local.join("Programs").join("Demo").join("Demo.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        assert!(installed_at(&windows, &PATHS));

        let mac = AgentEnv::at(home.clone()).with_os(Os::Mac);
        assert!(!installed_at(&mac, &PATHS));
        std::fs::create_dir_all(home.join("Applications").join("Demo.app")).unwrap();
        assert!(installed_at(&mac, &PATHS));

        assert!(!installed_at(
            &AgentEnv::at(home.clone()).with_os(Os::Linux),
            &PATHS
        ));
    }

    #[test]
    fn ids_are_unique_and_path_safe() {
        let mut seen = std::collections::HashSet::new();
        for spec in all() {
            assert!(seen.insert(spec.id), "重复的 Agent id：{}", spec.id);
            assert!(
                spec.id
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'),
                "Agent id 只能用小写字母、数字和 -：{}",
                spec.id
            );
            assert!(
                !matches!(spec.id, "codex" | "claude" | "grok"),
                "{} 已经是内置 Agent",
                spec.id
            );
        }
    }
}

