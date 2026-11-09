//! 通用 Agent 规格：手动接入的应用（配置存在云端账号里，本地只有缓存，Codex++ 不写也不读它的文件）。
//!
//! Codex++ 只负责签发 token、路由请求、记用量；地址、Key、模型由用户照界面上的「接入指引」
//! 在应用里自己填。以后加 Trae 国际版 / TRAE SOLO CN，只需再写一个 [`ManualConnect`] 和一条规格。

use std::path::PathBuf;

use crate::gateway::agents::{AgentSpec, AgentWire, InstallPaths, ManualConnect, installed_at};

pub static SPECS: &[AgentSpec] = &[TRAE_CN];

/// Trae 系列共用的提醒：Trae 的「自定义模型」会在 Base URL 后面自己拼接口路径。
const TRAE_NOTES: &[&str] = &[
    "「完整 URL」保持关闭：Trae 会在地址后自动拼上 /chat/completions、/responses 或 /v1/messages",
    "模型 ID 要与下方列表一字不差；发一条消息后状态变为「已接入」",
    "用于 Chat 与 Agent / Builder 模式；Trae 的代码补全不走自定义模型",
];

/// Trae「API 格式」下拉里的三种。
const TRAE_WIRES: &[AgentWire] = &[AgentWire::Chat, AgentWire::Responses, AgentWire::Anthropic];

pub const TRAE_CN_MANUAL: ManualConnect = ManualConnect {
    menu_path: "AI 设置 → 模型 → 添加模型 → 自定义",
    notes: TRAE_NOTES,
    wires: TRAE_WIRES,
    install: InstallPaths {
        windows_local_app_data: &["Programs/Trae CN/Trae CN.exe"],
        mac_apps: &["Trae CN.app"],
    },
};

pub const TRAE_CN: AgentSpec = AgentSpec {
    id: "trae-cn",
    name: "Trae CN",
    binary: "",
    // 手动接入没人按它写配置，只是默认值；三种协议网关都收。
    wire: AgentWire::Responses,
    detect: |env| installed_at(env, &TRAE_CN_MANUAL.install),
    // 没有配置文件：读 config_path 的地方都先看 manual。
    config_path: |_| PathBuf::new(),
    write: |_, _, _| Ok(()),
    // 接入指引由界面的卡片提供，不在详情里再放一个复制入口。
    hint: "",
    manual: Some(&TRAE_CN_MANUAL),
};
