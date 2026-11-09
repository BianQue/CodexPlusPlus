//! 手动接入的通用 Agent（`catalog_manual.rs`，目前是 Trae CN）：只签发 token、路由、记用量，
//! 不写、不读、不还原 Agent 的任何文件；token 只在显式换新时才变。

use codex_plus_core::gateway::activity::{ActivitySnapshot, AgentActivity, RouteRecord};
use codex_plus_core::gateway::agents::{self, AgentEnv, AgentWire, Os};
use codex_plus_core::gateway::apply::{
    ApplyContext, ProviderInput, create_provider, disable_binding, enable_binding,
    reapply_enabled, reapply_if_port_changed, restore_all_for_exit,
};
use codex_plus_core::gateway::store::{AgentBinding, GatewayStore, new_client_token};
use codex_plus_core::settings::{BackendSettings, RelayProtocol};
use codex_plus_core::tools::ToolId;

const ID: &str = "trae-cn";

struct Fixture {
    _temp: tempfile::TempDir,
    ctx: ApplyContext,
    home: std::path::PathBuf,
}

fn provider(name: &str, models: &str) -> ProviderInput {
    ProviderInput {
        name: name.into(),
        base_url: format!("https://api.{}.test/v1", name.to_lowercase()),
        protocol: RelayProtocol::ChatCompletions,
        keys: vec!["sk-test".into()],
        model: models.lines().next().unwrap_or_default().into(),
        model_list: models.into(),
        price: None,
        model_prices: None,
        proxy: None,
        strip_image_generation_tool: None,
        subscription_policy: None,
        strip_responses_image_detail: None,
        disable_tool_search: None,
    }
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let ctx = ApplyContext {
        gateway: GatewayStore::at(state.join("gateway.json")),
        claude: codex_plus_core::tools::claude::ClaudeContext {
            store_path: state.join("claude-providers.json"),
            claude_home: temp.path().join("claude"),
            backup_root: state.join("backups"),
        },
        grok_home: temp.path().join("grok"),
        codex_home: temp.path().join("codex"),
        settings: codex_plus_core::settings::SettingsStore::new(state.join("settings.json")),
        backup_root: state.join("backups"),
        port: 57321,
        agent_env: AgentEnv::at(&home),
    };
    create_provider(
        &ctx.gateway,
        &provider("DeepSeek", "deepseek-v4-pro\ndeepseek-v4-flash"),
        "preset:deepseek",
        None,
    )
    .unwrap();
    Fixture { _temp: temp, ctx, home }
}

fn agent() -> ToolId {
    ToolId::Unknown(ID.into())
}

fn spec() -> &'static agents::AgentSpec {
    agents::spec(ID).expect("trae-cn 已登记")
}

fn binding() -> AgentBinding {
    AgentBinding { model: "deepseek-v4-pro".into(), ..AgentBinding::default() }
}

/// 目录下全部文件的 (相对路径, 内容)；目录不存在时为空。
fn snapshot(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy().to_string();
                out.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn token(fixture: &Fixture) -> String {
    fixture.ctx.gateway.load().unwrap().bindings[ID].token.clone()
}

#[test]
fn trae_cn_is_a_registered_manual_agent() {
    let spec = spec();
    assert!(spec.manual.is_some());
    assert!(agents::is_manual(&agent()));
    assert!(agents::is_generic(&agent()));
    assert!(spec.binary.is_empty());
    assert_eq!((spec.config_path)(&AgentEnv::at("/home")), std::path::PathBuf::new());
}

#[test]
fn connecting_and_disconnecting_touches_no_file() {
    let fixture = fixture();
    let ctx = &fixture.ctx;
    let home = snapshot(&fixture.home);
    let backups = snapshot(&ctx.backup_root);

    let gateway = enable_binding(ctx, &agent(), binding()).unwrap();
    assert!(gateway.active_binding(&agent()).is_some());
    assert_eq!(snapshot(&fixture.home), home, "手动接入不能写 Agent 的文件");
    assert_eq!(snapshot(&ctx.backup_root), backups, "手动接入不留存档");
    assert!(!agents::connect::recorded(ctx, spec()));
    assert!(!agents::connect::points_at_gateway(ctx, spec()));
    assert!(codex_plus_core::gateway::drift::check(ctx, &agent()).is_none());

    disable_binding(ctx, &BackendSettings::default(), &agent()).unwrap();
    assert_eq!(snapshot(&fixture.home), home);
    assert_eq!(snapshot(&ctx.backup_root), backups);
}

#[test]
fn token_survives_disconnect_and_reconnect() {
    let fixture = fixture();
    let ctx = &fixture.ctx;
    enable_binding(ctx, &agent(), binding()).unwrap();
    let first = token(&fixture);
    assert!(first.starts_with("cpx-gw-trae-cn-"), "{first}");

    let gateway = disable_binding(ctx, &BackendSettings::default(), &agent()).unwrap();
    assert!(gateway.active_binding(&agent()).is_none(), "关掉后网关对它回 503");
    assert_eq!(gateway.bindings[ID].token, first, "关掉不换 token");

    enable_binding(ctx, &agent(), binding()).unwrap();
    assert_eq!(token(&fixture), first, "重新打开沿用 token");
}

#[test]
fn rotating_the_token_changes_it_without_writing_files() {
    let fixture = fixture();
    let ctx = &fixture.ctx;
    enable_binding(ctx, &agent(), binding()).unwrap();
    let old = token(&fixture);
    let home = snapshot(&fixture.home);
    // 与 rotate_gateway_token 命令同样的改法：只换这个 Agent 的 token，开着时再接入一次。
    let (_, rotated) = ctx
        .gateway
        .update(|gateway| {
            let binding = gateway.bindings.get_mut(ID).unwrap();
            binding.token = new_client_token(&agent());
            Ok(binding.clone())
        })
        .unwrap();
    enable_binding(ctx, &agent(), rotated).unwrap();
    assert_ne!(token(&fixture), old);
    assert_eq!(snapshot(&fixture.home), home);
}

#[test]
fn exit_restore_startup_and_port_change_keep_the_binding() {
    let fixture = fixture();
    let mut ctx = fixture.ctx.clone();
    enable_binding(&ctx, &agent(), binding()).unwrap();
    let first = token(&fixture);
    let home = snapshot(&fixture.home);

    restore_all_for_exit(&ctx, &BackendSettings::default(), true).unwrap();
    reapply_enabled(&ctx).unwrap();
    ctx.port = 57400;
    reapply_if_port_changed(&ctx).unwrap();
    reapply_enabled(&ctx).unwrap();

    let gateway = ctx.gateway.load().unwrap();
    assert!(gateway.active_binding(&agent()).is_some(), "退出、重启、换端口后仍开着");
    assert_eq!(gateway.bindings[ID].token, first);
    assert_eq!(snapshot(&fixture.home), home);
}

#[test]
fn guide_lists_the_same_model_ids_as_wired_agents() {
    let fixture = fixture();
    let ctx = &fixture.ctx;
    // 两家都有 deepseek-v4-pro：裸名只出现一次，ID 规则与写配置的 Agent 相同。
    create_provider(&ctx.gateway, &provider("Mirror", "deepseek-v4-pro\nkimi-k3"), "custom", None)
        .unwrap();
    let gateway = ctx.gateway.load().unwrap();
    let wired: Vec<String> = agents::connect::wiring(&gateway, &binding(), ID, ctx.port)
        .model_ids()
        .map(str::to_string)
        .collect();
    let guide = agents::connect::manual_guide(spec(), &gateway, Some(&binding()), ctx.port, false)
        .unwrap();
    let listed: Vec<String> = guide.models.iter().map(|model| model.id.clone()).collect();
    assert_eq!(listed, wired);
    assert_eq!(listed.iter().filter(|id| id.as_str() == "deepseek-v4-pro").count(), 1);
    assert!(listed.iter().any(|id| id == "kimi-k3"));
    assert_eq!(guide.menu_path, "AI 设置 → 模型 → 添加模型 → 自定义");
    assert!(!guide.seen);
}

#[test]
fn guide_endpoints_follow_what_trae_appends() {
    let fixture = fixture();
    let gateway = fixture.ctx.gateway.load().unwrap();
    let guide = agents::connect::manual_guide(spec(), &gateway, None, 57321, true).unwrap();
    let endpoints: Vec<(AgentWire, &str)> =
        guide.endpoints.iter().map(|endpoint| (endpoint.wire, endpoint.base_url.as_str())).collect();
    assert_eq!(
        endpoints,
        vec![
            (AgentWire::Chat, "http://127.0.0.1:57321/gw/trae-cn/v1"),
            (AgentWire::Responses, "http://127.0.0.1:57321/gw/trae-cn/v1"),
            (AgentWire::Anthropic, "http://127.0.0.1:57321/gw/trae-cn"),
        ]
    );
    assert!(guide.seen);
    assert!(!guide.models.is_empty(), "从没打开过也能先看到池里的模型");
    assert!(agents::connect::manual_guide(agents::spec("workbuddy").unwrap(), &gateway, None, 57321, false)
        .is_none());
}

#[test]
fn install_detection_only_looks_at_the_app() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let local = temp.path().join("Local");
    let windows = AgentEnv::at(home.clone())
        .with_os(Os::Windows)
        .with_var("LOCALAPPDATA", local.to_str().unwrap());
    // 只有 Trae 的用户数据目录（%APPDATA%\Trae CN）不算装了。
    std::fs::create_dir_all(home.join("AppData").join("Roaming").join("Trae CN")).unwrap();
    assert!(!agents::installed(spec(), &windows));
    let exe = local.join("Programs").join("Trae CN").join("Trae CN.exe");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"MZ").unwrap();
    assert!(agents::installed(spec(), &windows));

    let mac = AgentEnv::at(home.clone()).with_os(Os::Mac);
    assert!(!agents::installed(spec(), &mac));
    std::fs::create_dir_all(home.join("Applications").join("Trae CN.app")).unwrap();
    assert!(agents::installed(spec(), &mac));
    assert!(!agents::installed(spec(), &AgentEnv::at(home).with_os(Os::Linux)));
}

fn answered_at(time: u64) -> ActivitySnapshot {
    ActivitySnapshot {
        agents: vec![AgentActivity {
            agent: ID.into(),
            requests: 1,
            last: Some(RouteRecord {
                requested_model: "deepseek-v4-pro".into(),
                model: "deepseek-v4-pro".into(),
                group_id: None,
                planned: Vec::new(),
                steps: Vec::new(),
                result: "answered".into(),
                cooling_ms: None,
                finished_at_ms: time,
                elapsed_ms: 1,
            }),
            ..AgentActivity::default()
        }],
    }
}

#[test]
fn diagnostics_read_no_config_and_follow_traffic_only() {
    use codex_plus_core::gateway::diagnostics::inspect;
    let fixture = fixture();
    let ctx = &fixture.ctx;
    let gateway = enable_binding(ctx, &agent(), binding()).unwrap();
    let empty = ActivitySnapshot::default();

    let waiting = inspect(ctx, &agent(), &gateway, true, &empty);
    assert_eq!(waiting.state, "awaitingTraffic");
    assert!(waiting.config_evidence.is_empty(), "手动接入没有配置可核对");
    assert_eq!(inspect(ctx, &agent(), &gateway, false, &empty).state, "gatewayStopped");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    // 一分钟前的请求照样算「有流量」：不拿安装文件的修改时间过滤（Trae 自动更新会改它）。
    let active = inspect(ctx, &agent(), &gateway, true, &answered_at(now - 60_000));
    assert_eq!(active.state, "active");
    assert!(active.config_evidence.is_empty());
}

#[test]
fn disconnect_preview_reads_nothing_and_allows_disconnect() {
    let fixture = fixture();
    let ctx = &fixture.ctx;
    enable_binding(ctx, &agent(), binding()).unwrap();
    let preview = codex_plus_core::gateway::disconnect_preview::preview(
        ctx,
        &BackendSettings::default(),
        &agent(),
    )
    .unwrap();
    assert!(preview.can_disconnect);
    assert!(preview.files.is_empty());
    assert_eq!(preview.warnings.len(), 1, "{:?}", preview.warnings);
    assert!(preview.warnings[0].contains("没有改动"), "不能出现「配置文件原文差异」这类提示");
}

#[test]
fn switching_on_without_a_model_uses_the_pool_default() {
    // 手动接入的行里没有模型下拉，界面打开开关时传的是空模型：要能接入，默认模型取池里的。
    let fixture = fixture();
    let gateway = enable_binding(&fixture.ctx, &agent(), AgentBinding::default()).unwrap();
    assert!(gateway.active_binding(&agent()).is_some());
    let wired = agents::connect::wiring(&gateway, &gateway.bindings[ID], ID, fixture.ctx.port);
    assert!(!wired.default_model.is_empty());
}
