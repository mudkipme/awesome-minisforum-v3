// V3 Power — COSMIC panel applet for the Minisforum V3.
// Shows the active power profile + live wattage, switches v3-power modes, and lets you
// toggle allowlisted background services and mark them "auto-stop on battery".
//
// Backend contract (installed separately):
//   /etc/v3-power/services.conf     id|Label|unit unit ...     (allowlist, root-owned)
//   /var/lib/v3-power/autostop      space-separated ids        (owned by the user)
//   /var/lib/v3-power/mode          auto|saver|performance
//   /run/v3-power/effective         saver|performance
//   systemctl start v3-power-mode@<mode>.service  /  start|stop <allowlisted units>
//   (both permitted without a password by /etc/polkit-1/rules.d/50-v3-power.rules)

use std::process::Command;
use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::applet::padded_control;
use cosmic::iced::{Alignment, Length, Subscription, window::Id};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::segmented_button::{Entity, SingleSelectModel};
use cosmic::widget::{self, checkbox, icon, settings, text, toggler};
use cosmic::Element;

const APP_ID: &str = "com.manu.CosmicAppletV3Power";
const SERVICES_CONF: &str = "/etc/v3-power/services.conf";
const AUTOSTOP: &str = "/var/lib/v3-power/autostop";
const MODE: &str = "/var/lib/v3-power/mode";
const EFFECTIVE: &str = "/run/v3-power/effective";
const BAT: &str = "/sys/class/power_supply/BATT";
const AC: &str = "/sys/class/power_supply/ACAD/online";

const POPUP_W: f32 = 560.0;
const BODY_H: f32 = 600.0; // scrolling tab body; whole popup ≈ 720 px

const ICON_SAVER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="#2e3436" d="M14 1.5C7 1.5 2.5 5 2.5 10.2c0 1.2.3 2.3.8 3.2l-1.6 1.6.7.7 1.6-1.6c.9.5 2 .8 3.2.8C12.6 14.9 14.5 9.5 14 1.5zM5 12.4C6.3 9.2 8.6 6.9 11.6 5.4 9.3 7.4 7.4 9.8 6.3 12.9 5.8 12.8 5.4 12.6 5 12.4z"/></svg>"##;
const ICON_PERF: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="#2e3436" d="M9.5.5 2.5 9.5h4.6L5.8 15.5l7.7-9.6H8.7L9.5.5z"/></svg>"##;

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<V3Power>(())
}

// ---------------------------------------------------------------- model

#[derive(Clone, Debug)]
struct Svc {
    id: String,
    label: String,
    units: Vec<String>,
    running: bool,
    autostop: bool,
    busy: bool,
}

#[derive(Clone, Debug, Default)]
struct Power {
    mode: String,      // auto | saver | performance
    effective: String, // saver | performance
    ac: bool,
    percent: u32,
    watts: f32,
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn read_power() -> Power {
    let num = |p: &str| read(&format!("{BAT}/{p}")).parse::<f64>().unwrap_or(0.0);
    let mode = match read(MODE).as_str() {
        m @ ("saver" | "performance") => m.to_string(),
        _ => "auto".to_string(),
    };
    Power {
        mode,
        effective: read(EFFECTIVE),
        ac: read(AC) == "1",
        percent: num("capacity") as u32,
        watts: (num("current_now") * num("voltage_now") / 1e12) as f32,
    }
}

fn load_services() -> Vec<Svc> {
    let auto: Vec<String> = read(AUTOSTOP).split_whitespace().map(str::to_string).collect();
    read(SERVICES_CONF)
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let mut f = l.splitn(3, '|');
            let (id, label, units) = (f.next()?.trim(), f.next()?.trim(), f.next()?.trim());
            (!id.is_empty() && !units.is_empty()).then(|| Svc {
                id: id.to_string(),
                label: label.to_string(),
                units: units.split_whitespace().map(str::to_string).collect(),
                running: false,
                autostop: auto.iter().any(|a| a == id),
                busy: false,
            })
        })
        .collect()
}

/// A group counts as running when any of its *.service units is active
/// (an active socket alone just means "will start on demand").
fn running_states(svcs: &[(String, Vec<String>)]) -> Vec<(String, bool)> {
    svcs.iter()
        .map(|(id, units)| {
            let services: Vec<&String> = units.iter().filter(|u| u.ends_with(".service")).collect();
            let out = Command::new("systemctl")
                .arg("is-active")
                .args(&services)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            (id.clone(), out.lines().any(|l| l.trim() == "active"))
        })
        .collect()
}

fn systemctl(verb: &str, units: &[String]) -> bool {
    Command::new("systemctl")
        .arg("--no-ask-password")
        .arg(verb)
        .args(units)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn systemctl_user(args: &[&str]) -> bool {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn user_unit_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", unit])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("worker panicked")
}

// ---------------------------------------------------------------- tools tab

const UFW_CONF: &str = "/etc/ufw/ufw.conf";
const KEEPAWAKE_UNIT: &str = "v3-keepawake.service";
const AUTOBRIGHT_UNIT: &str = "v3-autobrightness.service";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Power,
    Performance,
    Firewall,
    Tools,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Firewall,
    KeepAwake,
    AutoBrightness,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolsState {
    firewall: bool,
    keep_awake: bool,
    auto_brightness: bool,
}

fn read_tools() -> ToolsState {
    ToolsState {
        firewall: read(UFW_CONF).lines().any(|l| l.trim() == "ENABLED=yes"),
        keep_awake: user_unit_active(KEEPAWAKE_UNIT),
        auto_brightness: user_unit_active(AUTOBRIGHT_UNIT),
    }
}

// ---- firewall rules (published by /usr/local/lib/v3-power/v3-fw as a readable TSV)

const FW_RULES: &str = "/run/v3-power/firewall-rules";

#[derive(Clone, Debug)]
pub struct FwRule {
    id: String,
    action: String,
    proto: String,
    port: String,
    src: String,
    iface: String,
    comment: String,
}

impl FwRule {
    fn title(&self) -> String {
        if !self.iface.is_empty() {
            format!("All traffic on {}", self.iface)
        } else if self.port == "any" {
            "All ports".into()
        } else if self.proto == "any" {
            format!("Port {}", self.port.replace(':', "–"))
        } else {
            format!("{}/{}", self.port.replace(':', "–"), self.proto)
        }
    }
    fn subtitle(&self) -> String {
        let act = if self.action == "allow" { "Allow" } else { "Block" };
        let from = if self.src == "any" { "anywhere".to_string() } else { self.src.clone() };
        let mut s = format!("{act} · from {from}");
        if !self.comment.is_empty() {
            s.push_str(" · ");
            s.push_str(&self.comment);
        }
        s
    }
}

fn read_fw_rules() -> Vec<FwRule> {
    read(FW_RULES)
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() == 7).then(|| FwRule {
                id: f[0].into(),
                action: f[1].into(),
                proto: f[2].into(),
                port: f[3].into(),
                src: f[4].into(),
                iface: f[5].into(),
                comment: f[6].into(),
            })
        })
        .collect()
}

/// Run one v3-fw operation through its polkit-allowed systemd unit.
fn fw_op(op: String) -> bool {
    let unit = Command::new("systemd-escape")
        .args(["--template=v3-fw@.service", &op])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    !unit.is_empty() && systemctl("start", &[unit])
}

/// Client-side checks so mistakes get a friendly message (the root helper re-validates).
fn build_add_op(action: &str, proto: &str, port: &str, src: &str, comment: &str) -> Result<String, String> {
    let port = port.trim().replace(['-', '–'], ":");
    let src = src.trim();
    let src = if src.is_empty() { "any" } else { src };
    let port = if port.is_empty() { "any".to_string() } else { port };
    if port == "any" && src == "any" {
        return Err("Enter a port (or a source address)".into());
    }
    if port != "any" {
        let nums: Vec<u32> = port.split(':').filter_map(|p| p.parse().ok()).collect();
        let parts = port.split(':').count();
        if nums.len() != parts || parts > 2 || nums.iter().any(|n| *n < 1 || *n > 65535) {
            return Err("Port must be 1–65535, or a range like 6000-6010".into());
        }
        if parts == 2 && proto == "any" {
            return Err("A port range needs TCP or UDP".into());
        }
    }
    if src != "any" && src.parse::<std::net::IpAddr>().is_err()
        && src.split_once('/').map_or(true, |(ip, _)| ip.parse::<std::net::IpAddr>().is_err())
    {
        return Err("From must be an IP or network, like 192.168.1.0/24".into());
    }
    let comment: String = comment
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " ._()-".contains(*c))
        .take(40)
        .collect();
    Ok(format!("add;{action};{proto};{port};{src};{}", comment.trim()))
}

// ---- sensors from the minisforum_v3 hwmon driver (~/bios-hppac), if loaded

#[derive(Clone, Debug, Default)]
pub struct Sensors {
    fans: Vec<(String, u32)>,
    temps: Vec<(String, f32)>,
    profile: String,
}

fn read_sensors() -> Option<Sensors> {
    let dir = std::fs::read_dir("/sys/class/hwmon").ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| read(&format!("{}/name", p.display())) == "minisforum_v3")?;
    let d = dir.display();
    let mut s = Sensors { profile: read("/sys/firmware/acpi/platform_profile"), ..Default::default() };
    for i in 1..=4 {
        if let Ok(v) = read(&format!("{d}/fan{i}_input")).parse::<u32>() {
            s.fans.push((read(&format!("{d}/fan{i}_label")), v));
        }
    }
    for i in 1..=16 {
        if let Ok(v) = read(&format!("{d}/temp{i}_input")).parse::<f32>() {
            s.temps.push((read(&format!("{d}/temp{i}_label")), v / 1000.0));
        }
    }
    Some(s)
}

fn set_tool(tool: Tool, on: bool) -> bool {
    match tool {
        Tool::Firewall => {
            systemctl("start", &[format!("v3-firewall@{}.service", if on { "on" } else { "off" })])
        }
        Tool::KeepAwake => systemctl_user(&[if on { "start" } else { "stop" }, KEEPAWAKE_UNIT]),
        // persistent across logins
        Tool::AutoBrightness => {
            systemctl_user(&[if on { "enable" } else { "disable" }, "--now", AUTOBRIGHT_UNIT])
        }
    }
}

// ---- power limits + fan curve (backend: /usr/local/lib/v3-power/v3-tune, root, clamps everything)

const TUNING: &str = "/var/lib/v3-power/tuning.json";
const TUNING_STATE: &str = "/run/v3-power/tuning-state";
const W_MIN: u32 = 5;
const W_MAX: u32 = 45;
const FAN_TEMPS: [u8; 6] = [45, 54, 66, 76, 88, 93];
/// Factory duty tables of the EC (levels 0-5); Quiet is also the floor the backend enforces.
const FAN_QUIET: [u8; 6] = [25, 32, 38, 40, 43, 50];
const FAN_BALANCED: [u8; 6] = [25, 32, 43, 54, 58, 60];
const FAN_PERF: [u8; 6] = [25, 36, 45, 56, 60, 64];
const FAN_MAX: [u8; 6] = [50, 60, 72, 85, 100, 100];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TuneProfile {
    power: Option<(u32, u32)>, // (sustained W, boost W); None = factory (BIOS mode)
    fan: Option<[u8; 6]>,      // duty % per level; None = factory table of the BIOS mode
}

#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    saver: TuneProfile,
    perf: TuneProfile,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            saver: TuneProfile { power: Some((8, 10)), fan: None },
            perf: TuneProfile { power: None, fan: None },
        }
    }
}

fn parse_profile(v: &serde_json::Value, dflt: TuneProfile) -> TuneProfile {
    let mut p = dflt;
    if let Some(pw) = v.get("power") {
        p.power = match (pw.get("stapm").and_then(|x| x.as_u64()), pw.get("fast").and_then(|x| x.as_u64())) {
            (Some(s), Some(f)) => Some((s as u32, f as u32)),
            _ => None,
        };
    }
    if let Some(f) = v.get("fan") {
        p.fan = f.as_array().filter(|a| a.len() == 6).map(|a| {
            let mut c = [0u8; 6];
            for (i, x) in a.iter().enumerate() { c[i] = x.as_u64().unwrap_or(0).min(100) as u8; }
            c
        });
    }
    p
}

fn read_tuning() -> Tuning {
    let d = Tuning::default();
    match std::fs::read_to_string(TUNING).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
        Some(v) => Tuning {
            saver: v.get("saver").map_or(d.saver, |x| parse_profile(x, d.saver)),
            perf: v.get("performance").map_or(d.perf, |x| parse_profile(x, d.perf)),
        },
        None => d,
    }
}

fn profile_json(p: &TuneProfile) -> serde_json::Value {
    serde_json::json!({
        "power": match p.power {
            Some((s, f)) => serde_json::json!({"stapm": s, "fast": f, "slow": s}),
            None => serde_json::json!("factory"),
        },
        "fan": match p.fan {
            Some(c) => serde_json::json!(c),
            None => serde_json::json!("factory"),
        },
    })
}

/// Save the settings (user-owned file) and ask root to apply them for the active profile.
fn save_and_apply(t: Tuning) -> bool {
    let v = serde_json::json!({"saver": profile_json(&t.saver), "performance": profile_json(&t.perf)});
    // written in place: the user owns the file, not the root-owned directory
    let saved = serde_json::to_string_pretty(&v).ok()
        .and_then(|s| std::fs::write(TUNING, s + "\n").ok())
        .is_some();
    saved && systemctl("start", &["v3-tune.service".to_string()])
}

/// One-line description of what the backend last applied.
fn read_tune_state() -> String {
    let st = read(TUNING_STATE);
    let get = |k: &str| st.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_string)).unwrap_or_default();
    if st.is_empty() { return String::new(); }
    let power = match get("power").as_str() {
        "factory" => "BIOS limits".to_string(),
        p => {
            let w: Vec<&str> = p.split('/').collect();
            if w.len() == 3 { format!("{} W sustained · {} W boost", w[0], w[1]) } else { p.to_string() }
        }
    };
    let fan = match get("fan").as_str() {
        "factory" => "BIOS fan curve".to_string(),
        "custom" => "custom fan curve".to_string(),
        e => format!("fan curve not applied ({e})"),
    };
    format!("Active: {power} · {fan}")
}

fn fan_name(c: Option<[u8; 6]>) -> &'static str {
    match c {
        None => "Factory",
        Some(c) if c == FAN_QUIET => "Quiet",
        Some(c) if c == FAN_BALANCED => "Balanced",
        Some(c) if c == FAN_PERF => "Cool",
        Some(c) if c == FAN_MAX => "Max",
        Some(_) => "Custom",
    }
}

// ---- segmented controls (native COSMIC "conjoined buttons")

const TABS: [(Tab, &str); 4] =
    [(Tab::Power, "Power"), (Tab::Performance, "Performance"), (Tab::Firewall, "Firewall"), (Tab::Tools, "Tools")];
const MODES: [&str; 3] = ["auto", "saver", "performance"];
const FW_PROTOS: [&str; 3] = ["tcp", "udp", "any"];
// last entry of each preset list is "Custom"
const POWER_BAT: [(&str, Option<(u32, u32)>); 6] = [("Factory", None), ("6 W", Some((6, 8))), ("8 W", Some((8, 10))),
    ("10 W", Some((10, 12))), ("12 W", Some((12, 15))), ("Custom", None)];
const POWER_AC: [(&str, Option<(u32, u32)>); 6] = [("Factory", None), ("20 W", Some((20, 25))), ("32 W", Some((32, 37))),
    ("35 W", Some((35, 40))), ("40 W", Some((40, 45))), ("Custom", None)];
const FAN_PRESETS: [(&str, Option<[u8; 6]>); 6] = [("Factory", None), ("Quiet", Some(FAN_QUIET)),
    ("Balanced", Some(FAN_BALANCED)), ("Cool", Some(FAN_PERF)), ("Max", Some(FAN_MAX)), ("Custom", None)];

fn preset_index<T: PartialEq + Copy>(list: &[(&str, Option<T>)], v: Option<T>) -> usize {
    list[..list.len() - 1].iter().position(|(_, x)| *x == v).unwrap_or(list.len() - 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegKind { Tab, Mode, Which, Power, Fan, Proto }

pub struct Segs {
    tab: SingleSelectModel,
    mode: SingleSelectModel,
    which: SingleSelectModel,
    power_bat: SingleSelectModel,
    power_ac: SingleSelectModel,
    fan: SingleSelectModel,
    proto: SingleSelectModel,
}

impl Default for Segs {
    fn default() -> Self {
        Segs {
            tab: seg(&TABS.map(|t| t.1)),
            mode: seg(&["Auto", "Saver", "Performance"]),
            which: seg(&["On battery", "Plugged in"]),
            power_bat: seg(&POWER_BAT.map(|p| p.0)),
            power_ac: seg(&POWER_AC.map(|p| p.0)),
            fan: seg(&FAN_PRESETS.map(|p| p.0)),
            proto: seg(&["TCP", "UDP", "Both"]),
        }
    }
}

fn seg(labels: &[&'static str]) -> SingleSelectModel {
    let mut m = SingleSelectModel::default();
    for (i, l) in labels.iter().enumerate() {
        m.insert().text(*l).data(i);
    }
    m
}

fn seg_set(m: &mut SingleSelectModel, idx: usize) {
    let found = m.iter().find(|e| m.data::<usize>(*e) == Some(&idx));
    if let Some(e) = found {
        if !m.is_active(e) { m.activate(e); }
    }
}

fn seg_control(m: &SingleSelectModel, kind: SegKind) -> Element<'_, Message> {
    widget::segmented_control::horizontal(m)
        .minimum_button_width(0)
        .on_activate(move |e| Message::Seg(kind, e))
        .into()
}

fn slider_item(label: impl Into<String>, range: std::ops::RangeInclusive<u32>, v: u32, unit: &'static str,
               msg: impl Fn(u32) -> Message + 'static) -> Element<'static, Message> {
    settings::item(label.into(), widget::Row::new().spacing(12).align_y(Alignment::Center)
        .push(widget::slider(range, v, msg).on_release(Message::TuneApply).width(Length::Fixed(260.0)))
        .push(text::body(format!("{v} {unit}")).width(Length::Fixed(48.0))))
    .into()
}

// ---------------------------------------------------------------- app

#[derive(Default)]
pub struct V3Power {
    core: Core,
    popup: Option<Id>,
    power: Power,
    services: Vec<Svc>,
    error: Option<String>,
    tab: Tab,
    tools: ToolsState,
    tool_busy: Option<Tool>,
    fw_rules: Vec<FwRule>,
    fw_port: String,
    fw_src: String,
    fw_comment: String,
    fw_proto: &'static str,
    fw_busy: bool,
    fw_msg: Option<String>,
    sensors: Option<Sensors>,
    tuning: Tuning,
    tune_saver: bool, // which profile the editor shows
    tune_busy: bool,
    tune_pending: bool, // a newer change arrived while applying
    tune_state: String,
    segs: Segs,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    Surface(cosmic::surface::Action<Message>),
    Tick,
    Running(Vec<(String, bool)>),
    SetMode(&'static str),
    ModeDone(bool),
    ToggleService(usize, bool),
    ServiceDone(usize, bool),
    ToggleAutostop(usize, bool),
    SetTab(Tab),
    Tools(ToolsState),
    ToggleTool(Tool, bool),
    ToolDone(Tool, bool),
    FwRules(Vec<FwRule>),
    FwPort(String),
    FwSrc(String),
    FwComment(String),
    FwProto(&'static str),
    FwAdd(&'static str),
    FwDelete(String),
    FwDone(bool),
    Sensors(Option<Sensors>),
    TuneEdit(bool),
    TunePower(Option<(u32, u32)>),
    TuneSustained(u32),
    TuneBoost(u32),
    TuneFanPreset(Option<[u8; 6]>),
    TuneFan(usize, u8),
    TuneApply,
    TuneDone(bool),
    TuneState(String),
    Seg(SegKind, Entity),
}

impl V3Power {
    fn refresh_running(&self) -> Task<Message> {
        let q: Vec<(String, Vec<String>)> =
            self.services.iter().map(|s| (s.id.clone(), s.units.clone())).collect();
        Task::perform(blocking(move || running_states(&q)), |r| {
            cosmic::Action::App(Message::Running(r))
        })
    }

    fn refresh_tools(&self) -> Task<Message> {
        Task::batch([
            Task::perform(blocking(read_tools), |t| cosmic::Action::App(Message::Tools(t))),
            Task::perform(blocking(read_fw_rules), |r| cosmic::Action::App(Message::FwRules(r))),
            Task::perform(blocking(read_sensors), |s| cosmic::Action::App(Message::Sensors(s))),
            Task::perform(blocking(read_tune_state), |s| cosmic::Action::App(Message::TuneState(s))),
        ])
    }

    fn run_fw(&mut self, op: String) -> Task<Message> {
        self.fw_busy = true;
        self.fw_msg = None;
        Task::perform(blocking(move || fw_op(op)), |ok| cosmic::Action::App(Message::FwDone(ok)))
    }

    fn tune_profile(&mut self) -> &mut TuneProfile {
        if self.tune_saver { &mut self.tuning.saver } else { &mut self.tuning.perf }
    }

    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Surface(a) => return cosmic::task::message(cosmic::Action::Surface(a)),
            Message::TogglePopup => {
                if let Some(id) = self.popup.take() {
                    return cosmic::task::message(cosmic::Action::Surface(destroy_popup(id)));
                }
                self.error = None;
                self.power = read_power();
                if !self.tune_busy { self.tuning = read_tuning(); }
                self.tune_saver = self.power.effective == "saver";
                // re-read config so edits to services.conf show up without restarting the panel
                let old = std::mem::take(&mut self.services);
                self.services = load_services();
                for s in &mut self.services {
                    if let Some(o) = old.iter().find(|o| o.id == s.id) { s.running = o.running; }
                }
                let open = app_popup::<V3Power>(
                    |_| Default::default(),
                    |app: &mut V3Power| {
                        let id = Id::unique();
                        app.popup = Some(id);
                        let mut s = app.core.applet.get_popup_settings(
                            app.core.main_window_id().unwrap(), id, None, None, None,
                        );
                        // default applet popups are capped at 360 px wide
                        s.positioner.size_limits = cosmic::iced::Limits::NONE
                            .min_width(POPUP_W)
                            .max_width(POPUP_W)
                            .min_height(1.0)
                            .max_height(1080.0);
                        s
                    },
                    None,
                );
                return Task::batch([
                    cosmic::task::message(cosmic::Action::Surface(open)),
                    self.refresh_running(),
                    self.refresh_tools(),
                ]);
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) { self.popup = None; }
            }
            Message::Tick => {
                self.power = read_power();
                if self.popup.is_some() {
                    return Task::batch([self.refresh_running(), self.refresh_tools()]);
                }
            }
            Message::Seg(kind, e) => {
                let cur = if self.tune_saver { self.tuning.saver } else { self.tuning.perf };
                let m = match kind {
                    SegKind::Tab => &self.segs.tab,
                    SegKind::Mode => &self.segs.mode,
                    SegKind::Which => &self.segs.which,
                    SegKind::Power => if self.tune_saver { &self.segs.power_bat } else { &self.segs.power_ac },
                    SegKind::Fan => &self.segs.fan,
                    SegKind::Proto => &self.segs.proto,
                };
                let Some(&i) = m.data::<usize>(e) else { return Task::none() };
                return match kind {
                    SegKind::Tab => { self.tab = TABS[i].0; Task::none() }
                    SegKind::Mode => self.update_inner(Message::SetMode(MODES[i])),
                    SegKind::Which => { self.tune_saver = i == 0; Task::none() }
                    SegKind::Power => {
                        let list = if self.tune_saver { &POWER_BAT } else { &POWER_AC };
                        let v = if i == list.len() - 1 {
                            cur.power.or(Some(if self.tune_saver { (10, 12) } else { (25, 28) }))
                        } else { list[i].1 };
                        self.update_inner(Message::TunePower(v))
                    }
                    SegKind::Fan => {
                        let v = if i == FAN_PRESETS.len() - 1 { cur.fan.or(Some(FAN_QUIET)) } else { FAN_PRESETS[i].1 };
                        self.update_inner(Message::TuneFanPreset(v))
                    }
                    SegKind::Proto => self.update_inner(Message::FwProto(FW_PROTOS[i])),
                };
            }
            Message::SetTab(tab) => self.tab = tab,
            Message::FwRules(r) => self.fw_rules = r,
            Message::Sensors(s) => self.sensors = s,
            Message::TuneState(s) => self.tune_state = s,
            Message::TuneEdit(saver) => self.tune_saver = saver,
            Message::TunePower(v) => {
                self.tune_profile().power = v;
                return self.update_inner(Message::TuneApply);
            }
            Message::TuneSustained(v) => {
                let p = self.tune_profile();
                if let Some((s, f)) = p.power.as_mut() { *s = v; if *f < v { *f = v; } }
            }
            Message::TuneBoost(v) => {
                let p = self.tune_profile();
                if let Some((s, f)) = p.power.as_mut() { *f = v; if *s > v { *s = v; } }
            }
            Message::TuneFanPreset(v) => {
                self.tune_profile().fan = v;
                return self.update_inner(Message::TuneApply);
            }
            Message::TuneFan(i, v) => {
                if let Some(c) = self.tune_profile().fan.as_mut() {
                    c[i] = v;
                    for j in i + 1..6 { c[j] = c[j].max(v); }          // keep the curve rising
                    for j in 0..i { c[j] = c[j].min(v).max(FAN_QUIET[j]); }
                }
            }
            Message::TuneApply => {
                if self.tune_busy { self.tune_pending = true; return Task::none(); }
                self.tune_busy = true;
                let t = self.tuning;
                return Task::perform(blocking(move || save_and_apply(t)), |ok| cosmic::Action::App(Message::TuneDone(ok)));
            }
            Message::TuneDone(ok) => {
                self.tune_busy = false;
                if std::mem::take(&mut self.tune_pending) { return self.update_inner(Message::TuneApply); }
                if !ok { self.error = Some("Couldn't apply power/fan settings".into()); }
                return Task::perform(blocking(read_tune_state), |s| cosmic::Action::App(Message::TuneState(s)));
            }
            Message::FwPort(v) => self.fw_port = v,
            Message::FwSrc(v) => self.fw_src = v,
            Message::FwComment(v) => self.fw_comment = v,
            Message::FwProto(p) => self.fw_proto = p,
            Message::FwAdd(action) => {
                match build_add_op(action, self.fw_proto, &self.fw_port, &self.fw_src, &self.fw_comment) {
                    Ok(op) => return self.run_fw(op),
                    Err(e) => self.fw_msg = Some(e),
                }
            }
            Message::FwDelete(id) => return self.run_fw(format!("del;{id}")),
            Message::FwDone(ok) => {
                self.fw_busy = false;
                if ok {
                    self.fw_port.clear();
                    self.fw_src.clear();
                    self.fw_comment.clear();
                    self.fw_msg = Some("Saved".into());
                } else {
                    self.fw_msg = Some("Rejected by the firewall: check the values".into());
                }
                return Task::perform(blocking(read_fw_rules), |r| cosmic::Action::App(Message::FwRules(r)));
            }
            Message::Tools(t) => {
                if self.tool_busy.is_none() { self.tools = t; }
            }
            Message::ToggleTool(tool, on) => {
                self.tool_busy = Some(tool);
                match tool {
                    Tool::Firewall => self.tools.firewall = on,
                    Tool::KeepAwake => self.tools.keep_awake = on,
                    Tool::AutoBrightness => self.tools.auto_brightness = on,
                }
                return Task::perform(blocking(move || set_tool(tool, on)), move |ok| {
                    cosmic::Action::App(Message::ToolDone(tool, ok))
                });
            }
            Message::ToolDone(tool, ok) => {
                self.tool_busy = None;
                if !ok {
                    self.error = Some(match tool {
                        Tool::Firewall => "Couldn't change the firewall".into(),
                        Tool::KeepAwake => "Couldn't change keep awake".into(),
                        Tool::AutoBrightness => "Couldn't change auto-brightness".into(),
                    });
                }
                return self.refresh_tools();
            }
            Message::Running(states) => {
                for (id, running) in states {
                    if let Some(s) = self.services.iter_mut().find(|s| s.id == id && !s.busy) {
                        s.running = running;
                    }
                }
            }
            Message::SetMode(mode) => {
                self.power.mode = mode.to_string();
                let unit = vec![format!("v3-power-mode@{mode}.service")];
                return Task::perform(blocking(move || systemctl("start", &unit)), |ok| {
                    cosmic::Action::App(Message::ModeDone(ok))
                });
            }
            Message::ModeDone(ok) => {
                if !ok { self.error = Some("Couldn't change power mode".into()); }
                self.power = read_power();
            }
            Message::ToggleService(i, on) => {
                let Some(s) = self.services.get_mut(i) else { return Task::none() };
                s.busy = true;
                s.running = on;
                let mut units = s.units.clone();
                if !on { units.reverse(); }
                let verb = if on { "start" } else { "stop" };
                return Task::perform(blocking(move || systemctl(verb, &units)), move |ok| {
                    cosmic::Action::App(Message::ServiceDone(i, ok))
                });
            }
            Message::ServiceDone(i, ok) => {
                if let Some(s) = self.services.get_mut(i) {
                    s.busy = false;
                    if !ok { self.error = Some(format!("Couldn't change {}", s.label)); }
                }
                return self.refresh_running();
            }
            Message::ToggleAutostop(i, on) => {
                if let Some(s) = self.services.get_mut(i) { s.autostop = on; }
                self.save_autostop();
            }
        }
        Task::none()
    }

    // ------------------------------------------------------------ views (one per tab)

    fn power_tab(&self) -> Element<'_, Message> {
        let mode = settings::section().title("Power mode").add(
            widget::Column::new().spacing(8)
                .push(seg_control(&self.segs.mode, SegKind::Mode))
                .push(text::caption(match self.power.mode.as_str() {
                    "saver" => "Always battery saver, even when plugged in",
                    "performance" => "Always full performance, even on battery",
                    _ => "Battery saver on battery, full performance when plugged in",
                })),
        );
        let mut svcs = settings::section().header(
            widget::Row::new().align_y(Alignment::Center)
                .push(text::heading("Background services").width(Length::Fill))
                .push(text::caption("Auto = stop on battery")),
        );
        for (i, s) in self.services.iter().enumerate() {
            let state = if s.busy { "Working…" } else if s.running { "Running" } else { "Stopped" };
            svcs = svcs.add(
                settings::item::builder(s.label.as_str()).description(state).control(
                    widget::Row::new().spacing(16).align_y(Alignment::Center)
                        .push(checkbox(s.autostop).label("Auto").on_toggle(move |v| Message::ToggleAutostop(i, v)))
                        .push(toggler(s.running).on_toggle_maybe((!s.busy).then_some(move |v| Message::ToggleService(i, v)))),
                ),
            );
        }
        widget::Column::new().spacing(18).push(mode).push(svcs).into()
    }

    fn perf_tab(&self) -> Element<'_, Message> {
        let p = if self.tune_saver { self.tuning.saver } else { self.tuning.perf };
        let busy = self.tune_busy;

        let profile = settings::section().title("Settings for").add(
            widget::Column::new().spacing(8)
                .push(seg_control(&self.segs.which, SegKind::Which))
                .push(text::caption(if busy { "Applying…".to_string() } else if self.tune_state.is_empty() {
                    "Nothing applied yet".to_string() } else { self.tune_state.clone() })),
        );

        let ptxt = match p.power {
            None => "Factory: the BIOS mode's own limit (15 W on battery, 28 W plugged in)".to_string(),
            Some((s, f)) => format!("{s} W sustained, up to {f} W for short bursts"),
        };
        let mut power = settings::section().title("Power limit").add(
            widget::Column::new().spacing(8)
                .push(seg_control(if self.tune_saver { &self.segs.power_bat } else { &self.segs.power_ac }, SegKind::Power))
                .push(text::caption(ptxt))
                .push_maybe(p.power.filter(|(s, _)| *s > 28).map(|_| text::caption(
                    "Above 28 W the chip runs into its 90 °C limit: pair it with the Cool or Max fan curve."))),
        );
        if let Some((s, f)) = p.power {
            power = power
                .add(slider_item("Sustained", W_MIN..=W_MAX, s, "W", Message::TuneSustained))
                .add(slider_item("Boost", W_MIN..=W_MAX, f, "W", Message::TuneBoost));
        }

        let mut fan = settings::section().title("Fan curve").add(
            widget::Column::new().spacing(8)
                .push(seg_control(&self.segs.fan, SegKind::Fan))
                .push(text::caption(match fan_name(p.fan) {
                    "Factory" => "Factory: the curve of the current BIOS mode",
                    "Quiet" => "Quietest allowed: the BIOS Quiet curve",
                    "Balanced" => "The BIOS Balanced curve",
                    "Cool" => "The BIOS Performance curve: cooler, louder",
                    "Max" => "Aggressive: high speeds early",
                    _ => "Your own curve: drag the steps below",
                })),
        );
        if let Some(c) = p.fan {
            for i in 0..6 {
                let lo = if i == 0 { "below".to_string() } else { format!("{}–", FAN_TEMPS[i - 1]) };
                fan = fan.add(slider_item(format!("{lo}{} °C", FAN_TEMPS[i]), FAN_QUIET[i] as u32..=100, c[i] as u32, "%",
                    move |v| Message::TuneFan(i, v as u8)));
            }
            fan = fan.add(text::caption("Above 93 °C the EC's emergency level always runs the fan at 90 %. \
                The curve is reset by a reboot or a BIOS mode change; V3 Power re-applies it automatically."));
        }

        let mut col = widget::Column::new().spacing(18).push(profile).push(power).push(fan);
        if let Some(s) = &self.sensors {
            col = col.push(self.sensors_section(s));
        }
        col.into()
    }

    fn sensors_section<'a>(&'a self, s: &'a Sensors) -> Element<'a, Message> {
        let mut sec = settings::section().title("Sensors");
        sec = sec.add(settings::item("BIOS fan mode", text::body(if s.profile.is_empty() { "?" } else { s.profile.as_str() })));
        for (l, v) in &s.fans {
            sec = sec.add(settings::item(l.as_str(), text::body(format!("{v} rpm"))));
        }
        for pair in s.temps.chunks(2) {
            let mut r = widget::Row::new().spacing(32);
            for (l, v) in pair {
                r = r.push(widget::Row::new().width(Length::Fill)
                    .push(text::body(l.as_str()).width(Length::Fill))
                    .push(text::body(format!("{v:.0} °C"))));
            }
            if pair.len() == 1 { r = r.push(widget::Space::new().width(Length::Fill)); }
            sec = sec.add(r);
        }
        sec.into()
    }

    fn firewall_tab(&self) -> Element<'_, Message> {
        let t = self.tools;
        let busy_tool = self.tool_busy == Some(Tool::Firewall);
        let main = settings::section().add(
            settings::item::builder("Firewall")
                .description(if busy_tool { "Working…" } else { "Blocks incoming connections except the rules below" })
                .toggler_maybe(t.firewall, (!busy_tool).then_some(|v| Message::ToggleTool(Tool::Firewall, v))),
        );

        let mut rules = settings::section().title("Allowed / blocked incoming");
        if self.fw_rules.is_empty() {
            rules = rules.add(text::caption("No rules: all incoming connections are blocked"));
        }
        for r in &self.fw_rules {
            rules = rules.add(settings::item::builder(r.title()).description(r.subtitle()).control(
                widget::button::icon(icon::from_name("edit-delete-symbolic"))
                    .tooltip("Delete rule")
                    .on_press_maybe((!self.fw_busy).then(|| Message::FwDelete(r.id.clone()))),
            ));
        }

        let busy = self.fw_busy;
        let field = |w: widget::TextInput<'static, Message>| w.width(Length::Fixed(260.0));
        let add = settings::section().title("Add a rule")
            .add(settings::item("Port", field(widget::text_input("8080 or 6000-6010", self.fw_port.clone()).on_input(Message::FwPort))))
            .add(settings::item("Protocol", widget::container(seg_control(&self.segs.proto, SegKind::Proto)).width(Length::Fixed(260.0))))
            .add(settings::item("From", field(widget::text_input("blank = anywhere", self.fw_src.clone()).on_input(Message::FwSrc))))
            .add(settings::item("Name", field(widget::text_input("optional", self.fw_comment.clone())
                .on_input(Message::FwComment).on_submit(|_| Message::FwAdd("allow")))))
            .add(widget::Row::new().spacing(8).align_y(Alignment::Center)
                .push(text::caption(match (&self.fw_msg, busy) {
                    (_, true) => "Applying…".to_string(),
                    (Some(m), _) => m.clone(),
                    _ => String::new(),
                }).width(Length::Fill))
                .push(widget::button::standard("Block").on_press_maybe((!busy).then_some(Message::FwAdd("deny"))))
                .push(widget::button::suggested("Allow").on_press_maybe((!busy).then_some(Message::FwAdd("allow")))));

        widget::Column::new().spacing(18).push(main).push(rules).push(add).into()
    }

    fn tools_tab(&self) -> Element<'_, Message> {
        let t = self.tools;
        let row = |tool: Tool, title: &'static str, desc: &'static str, on: bool| {
            let busy = self.tool_busy == Some(tool);
            settings::item::builder(title)
                .description(if busy { "Working…" } else { desc })
                .toggler_maybe(on, (!busy).then_some(move |v| Message::ToggleTool(tool, v)))
        };
        settings::section().title("Display & session")
            .add(row(Tool::KeepAwake, "Keep awake",
                "No screen-off or sleep while on (closing the lid still sleeps)", t.keep_awake))
            .add(row(Tool::AutoBrightness, "Auto-brightness",
                "Follows the light sensor. Adjust brightness by hand to teach it your taste", t.auto_brightness))
            .into()
    }

    /// Keep every segmented control showing the current state.
    fn sync_segs(&mut self) {
        let tab = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
        seg_set(&mut self.segs.tab, tab);
        seg_set(&mut self.segs.mode, MODES.iter().position(|m| *m == self.power.mode).unwrap_or(0));
        seg_set(&mut self.segs.which, if self.tune_saver { 0 } else { 1 });
        seg_set(&mut self.segs.power_bat, preset_index(&POWER_BAT, self.tuning.saver.power));
        seg_set(&mut self.segs.power_ac, preset_index(&POWER_AC, self.tuning.perf.power));
        let fan = if self.tune_saver { self.tuning.saver.fan } else { self.tuning.perf.fan };
        seg_set(&mut self.segs.fan, preset_index(&FAN_PRESETS, fan));
        seg_set(&mut self.segs.proto, FW_PROTOS.iter().position(|p| *p == self.fw_proto).unwrap_or(0));
    }

    fn save_autostop(&mut self) {
        let ids: Vec<&str> =
            self.services.iter().filter(|s| s.autostop).map(|s| s.id.as_str()).collect();
        if let Err(e) = std::fs::write(AUTOSTOP, ids.join(" ") + "\n") {
            self.error = Some(format!("Can't save auto-stop list: {e}"));
        }
    }

    /// Embedded symbolic icons (the COSMIC theme has no power-profile icons):
    /// leaf = battery saver, lightning bolt = performance.
    fn panel_icon(&self) -> icon::Handle {
        let svg: &'static [u8] = match self.power.effective.as_str() {
            "saver" => ICON_SAVER,
            _ => ICON_PERF,
        };
        let mut h = icon::from_svg_bytes(svg);
        h.symbolic = true;
        h
    }

    fn summary(&self) -> String {
        let p = &self.power;
        let profile = match p.effective.as_str() {
            "saver" => "Battery saver",
            "performance" => "Performance",
            _ => "Unknown",
        };
        let src = if p.ac {
            if p.watts > 0.3 { format!("Charging {:.1} W", p.watts) } else { "Plugged in".into() }
        } else {
            format!("Drawing {:.1} W", p.watts)
        };
        let awake = if self.tools.keep_awake { " · Keep awake" } else { "" };
        format!("{profile} · {}% · {src}{awake}", p.percent)
    }

}

impl cosmic::Application for V3Power {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let app = V3Power { core, power: read_power(), services: load_services(), ..Default::default() };
        let t = Task::batch([app.refresh_running(), app.refresh_tools()]);
        (app, t)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        // cheap sysfs reads; poll faster only while the popup is open
        let every = if self.popup.is_some() { 2 } else { 10 };
        cosmic::iced::time::every(Duration::from_secs(every)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let t = self.update_inner(message);
        self.sync_segs();
        t
    }

    fn view(&self) -> Element<'_, Message> {
        let btn = self.core.applet.icon_button_from_handle(self.panel_icon()).on_press(Message::TogglePopup);
        self.core
            .applet
            .applet_tooltip::<Message>(btn, self.summary(), self.popup.is_some(), Message::Surface, None)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        let mut content = widget::Column::new().padding([10, 0]).spacing(10).width(Length::Fixed(POPUP_W));
        content = content.push(padded_control(
            widget::Row::new()
                .align_y(Alignment::Center)
                .spacing(12)
                .push(widget::icon(self.panel_icon()).size(24))
                .push(widget::Column::new().push(text::title4("V3 Power")).push(text::caption(self.summary()))),
        ));
        content = content.push(padded_control(seg_control(&self.segs.tab, SegKind::Tab)));
        let body = match self.tab {
            Tab::Power => self.power_tab(),
            Tab::Performance => self.perf_tab(),
            Tab::Firewall => self.firewall_tab(),
            Tab::Tools => self.tools_tab(),
        };
        content = content.push(
            widget::scrollable(widget::container(body).padding([4, 18, 8, 14])).height(Length::Fixed(BODY_H)),
        );
        if let Some(err) = &self.error {
            content = content.push(padded_control(text::caption(err.as_str())));
        }
        // popup_container() caps popups at 360 px; override it
        self.core.applet.popup_container(widget::container(content))
            .limits(cosmic::iced::Limits::NONE.min_height(1.0).min_width(POPUP_W).max_width(POPUP_W).max_height(1000.0))
            .into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
