use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::i18n::I18n;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};

#[derive(Debug, Clone)]
pub enum SshMsg {
    SelectProfile(usize),
    NameChanged(String),
    HostChanged(String),
    UserChanged(String),
    PortChanged(String),
    KeyPathChanged(String),
    AddProfile,
    DeleteProfile(usize),
}

#[derive(Debug, Clone)]
pub struct SshProfile {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: String,
    pub key_path: String,
}

#[derive(Debug, Clone)]
pub struct SshState {
    pub profiles: Vec<SshProfile>,
    pub selected: Option<usize>,
    /// `load` 时刻磁盘上已存在的 profile 名字序列(含重名)。
    /// Save 时用于区分「用户在 UI 里删除的 profile」(快照里有、
    /// UI 里没有 → 丢弃)与「设置打开期间外部进程新增的
    /// profile」(快照里没有 → 原样保留),后者不能被
    /// Save 静默回滚。
    loaded_names: Vec<String>,
}

impl Default for SshState {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            selected: None,
            loaded_names: Vec::new(),
        }
    }
}

impl SshState {
    pub fn load(doc: &toml_edit::DocumentMut) -> Self {
        let mut profiles = Vec::new();
        let mut loaded_names = Vec::new();
        if let Some(ssh) = doc.get("ssh").and_then(|i| i.as_table()) {
            if let Some(arr) = ssh.get("profiles").and_then(|i| i.as_array_of_tables()) {
                for item in arr.iter() {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    loaded_names.push(name.clone());
                    profiles.push(SshProfile {
                        name,
                        host: item.get("host").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        user: item.get("user").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        port: item.get("port").and_then(|v| v.as_integer())
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "22".to_string()),
                        key_path: item.get("key_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    });
                }
            }
        }
        let selected = if profiles.is_empty() { None } else { Some(0) };
        Self { profiles, selected, loaded_names }
    }

    pub fn update(&mut self, msg: SshMsg, doc: &mut toml_edit::DocumentMut) {
        match msg {
            SshMsg::SelectProfile(i) => {
                self.selected = Some(i);
            }
            SshMsg::NameChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.name = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::HostChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.host = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::UserChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.user = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::PortChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.port = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::KeyPathChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.key_path = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::AddProfile => {
                self.profiles.push(SshProfile {
                    name: format!("Profile {}", self.profiles.len() + 1),
                    host: String::new(),
                    user: String::new(),
                    port: "22".to_string(),
                    key_path: String::new(),
                });
                self.selected = Some(self.profiles.len() - 1);
                self.sync_to_doc(doc);
            }
            SshMsg::DeleteProfile(i) => {
                if i < self.profiles.len() {
                    self.profiles.remove(i);
                    self.selected = if self.profiles.is_empty() {
                        None
                    } else {
                        Some(i.min(self.profiles.len() - 1))
                    };
                    self.sync_to_doc(doc);
                }
            }
        }
    }

    /// 把内存配置档列表同步回 `doc["ssh"]["profiles"]`。
    ///
    /// **合并模式(有意设计)**:设置 UI 只暴露 5 个字段
    /// (`name/host/user/port/key_path`),但磁盘 schema 有 7 个
    /// (多出 `password`——加密凭据——和 `shortcut`)。天真的
    /// `doc["ssh"]["profiles"] = ArrayOfTables::new()` 会在用户每次
    /// 触碰 SSH 页时静默丢弃这两个 UI 不可见
    /// 字段。因此我们改为按 `name` 把每个 UI 配置档与既有磁盘
    /// 表匹配,并把未知键原样带过去。
    ///
    /// 匹配语义:
    /// - `name` 相同:保留原表的条目,用 UI 值覆盖 5 个
    ///   已知字段,丢弃 UI 清空的可选字段(空 `user`/`key_path`、
    ///   默认端口 22)。重名时按出现次序逐一配对。
    /// - UI 有而文档没有的名字:追加全新的 5 字段表。
    /// - 文档有而 UI 没有的名字:仅当该名字在 `loaded_names`
    ///   快照中才视为「用户在 UI 中删除」而丢弃;快照之外的
    ///   条目(设置打开期间外部进程新增)原样保留,防止
    ///   Save 回滚外部写入。
    fn sync_to_doc(&self, doc: &mut toml_edit::DocumentMut) {
        // 按名字快照既有磁盘配置档表,以便恢复任何 UI 未知
        // 字段(password / shortcut / 未来
        // 新增)。同名多条目进同一队列,按出现次序配对,
        // 避免重名时 HashMap 静默覆盖导致字段丢失。
        let mut existing_by_name: std::collections::HashMap<String, Vec<toml_edit::Table>> =
            std::collections::HashMap::new();
        if let Some(arr) = doc
            .get("ssh")
            .and_then(|i| i.as_table())
            .and_then(|t| t.get("profiles"))
            .and_then(|i| i.as_array_of_tables())
        {
            for table in arr.iter() {
                if let Some(name) = table.get("name").and_then(|v| v.as_str()) {
                    existing_by_name
                        .entry(name.to_string())
                        .or_default()
                        .push(table.clone());
                }
            }
        }

        let mut arr = toml_edit::ArrayOfTables::new();
        for p in &self.profiles {
            // 从既有磁盘表(若有)出发,使 password 和
            // shortcut 这类未知字段在往返后存活。
            let mut table = existing_by_name
                .get_mut(&p.name)
                .and_then(|v| {
                    if v.is_empty() { None } else { Some(v.remove(0)) }
                })
                .unwrap_or_else(toml_edit::Table::new);

            // 总是用内存值覆盖 UI 已知字段。
            table.insert("name", toml_edit::Item::Value(p.name.clone().into()));
            table.insert("host", toml_edit::Item::Value(p.host.clone().into()));

            if p.user.is_empty() {
                table.remove("user");
            } else {
                table.insert("user", toml_edit::Item::Value(p.user.clone().into()));
            }

            match p.port.parse::<i64>() {
                Ok(port) if port != 22 => {
                    table.insert("port", toml_edit::Item::Value(port.into()));
                }
                Ok(_) => {
                    // 端口为 22(默认)——丢弃显式键,使
                    // 表保持精简,与最初加载时把缺失端口
                    // 视为 22 的语义一致。
                    table.remove("port");
                }
                Err(_) => {
                    // 保留现有值(可能是用户正在
                    // 文本框输入);下一次有效按键会覆盖它。
                }
            }

            if p.key_path.is_empty() {
                table.remove("key_path");
            } else {
                table.insert(
                    "key_path",
                    toml_edit::Item::Value(p.key_path.clone().into()),
                );
            }

            arr.push(table);
        }

        // 未被 UI 匹配的磁盘条目:仅「快照里存在」的才视为用户
        // 删除;其余(外部进程在设置打开期间新增)原样保留。
        for (name, tables) in existing_by_name {
            if self.loaded_names.contains(&name) {
                continue;
            }
            for table in tables {
                arr.push(table);
            }
        }

        crate::config_io::ensure_table(doc, &["ssh"]);
        doc["ssh"]["profiles"] = toml_edit::Item::ArrayOfTables(arr);
    }

    /// 把本页内存状态应用到给定文档。供
    /// `SettingsState::apply_dirty_sections` 使用:保存时重载
    /// 最新磁盘文档并只重放本页的意图。
    ///
    /// 基于 `sync_to_doc` 实现(它已做
    /// 分节合并)。
    pub fn apply(&self, doc: &mut toml_edit::DocumentMut) {
        self.sync_to_doc(doc);
    }
}

pub fn view(state: &SshState, i18n: &I18n) -> Element<'static, Message> {
    let unnamed = i18n.t("ssh.unnamed");
    let no_host = i18n.t("ssh.no_host");

    let profile_list: Vec<Element<Message>> = state
        .profiles
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let selected = state.selected == Some(i);
            let label = if p.name.is_empty() { unnamed.clone() } else { p.name.clone() };
            let subtitle = if p.host.is_empty() { no_host.clone() } else { p.host.clone() };
            button(
                column![
                    text(label).size(theme::FONT_SIZE),
                    text(subtitle).size(11.0).color(TEXT_SECONDARY),
                ]
                .spacing(2),
            )
            .width(Length::Fill)
            .padding(Padding::from([6, 10]))
            .style(move |t: &iced::Theme, s| theme::button::sidebar_item(t, s, selected))
            .on_press(Message::Ssh(SshMsg::SelectProfile(i)))
            .into()
        })
        .collect();

    let list_panel = column![
        scrollable(
            iced::widget::Column::with_children(profile_list)
                .spacing(4)
                .width(Length::Fill),
        )
        .height(Length::Fill),
        button(text(i18n.t("ssh.add_profile")).size(theme::FONT_SIZE))
            .width(Length::Fill)
            .padding(8)
            .style(theme::button::secondary)
            .on_press(Message::Ssh(SshMsg::AddProfile)),
    ]
    .spacing(SPACING)
    .width(200)
    .height(Length::Fill);

    let editor = if let Some(i) = state.selected {
        if let Some(p) = state.profiles.get(i) {
            let idx = i;
            container(
                column![
                    labeled_row(
                        i18n.t("ssh.name"),
                        text_input(&i18n.t("ssh.placeholder.name"), &p.name)
                            .on_input(|v| Message::Ssh(SshMsg::NameChanged(v)))
                            .style(theme::text_input::field)
                            .width(250)
                            .into(),
                    ),
                    labeled_row(
                        i18n.t("ssh.host"),
                        text_input(&i18n.t("ssh.placeholder.host"), &p.host)
                            .on_input(|v| Message::Ssh(SshMsg::HostChanged(v)))
                            .style(theme::text_input::field)
                            .width(250)
                            .into(),
                    ),
                    labeled_row(
                        i18n.t("ssh.user"),
                        text_input(&i18n.t("ssh.placeholder.user"), &p.user)
                            .on_input(|v| Message::Ssh(SshMsg::UserChanged(v)))
                            .style(theme::text_input::field)
                            .width(200)
                            .into(),
                    ),
                    labeled_row(
                        i18n.t("ssh.port"),
                        text_input("22", &p.port)
                            .on_input(|v| Message::Ssh(SshMsg::PortChanged(v)))
                            .style(theme::text_input::field)
                            .width(80)
                            .into(),
                    ),
                    labeled_row(
                        i18n.t("ssh.key_path"),
                        text_input(&i18n.t("ssh.placeholder.key_path"), &p.key_path)
                            .on_input(|v| Message::Ssh(SshMsg::KeyPathChanged(v)))
                            .style(theme::text_input::field)
                            .width(300)
                            .into(),
                    ),
                    row![
                        iced::widget::space::horizontal(),
                        button(text(i18n.t("ssh.delete")).size(theme::FONT_SIZE))
                            .padding(Padding::from([6, 16]))
                            .style(theme::button::danger)
                            .on_press(Message::Ssh(SshMsg::DeleteProfile(idx))),
                    ],
                ]
                .spacing(SPACING)
                .padding(Padding::from(PADDING as u16)),
            )
            .width(Length::Fill)
            .into()
        } else {
            placeholder(i18n.t("ssh.select_profile"))
        }
    } else {
        placeholder(i18n.t("ssh.no_profiles"))
    };

    container(
        row![list_panel, iced::widget::rule::vertical(1), editor]
            .spacing(0)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn placeholder(msg: String) -> Element<'static, Message> {
    container(
        text(msg)
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn labeled_row<'a>(label: String, widget: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label)
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(100),
        widget,
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_profiles(toml: &str) -> toml_edit::DocumentMut {
        toml.parse::<toml_edit::DocumentMut>().unwrap()
    }

    fn profile_tables(doc: &toml_edit::DocumentMut) -> Vec<&toml_edit::Table> {
        doc.get("ssh").unwrap().as_table().unwrap()
            .get("profiles").unwrap().as_array_of_tables().unwrap()
            .iter().collect()
    }

    fn profile_names(doc: &toml_edit::DocumentMut) -> Vec<String> {
        profile_tables(doc)
            .iter()
            .map(|t| t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn sync_preserves_invisible_fields_on_roundtrip() {
        let initial = r#"[[ssh.profiles]]
name = "prod"
host = "10.0.0.1"
password = "ENCRYPTED_BLOB"
shortcut = "Ctrl+1"
"#;
        let doc = doc_with_profiles(initial);
        let state = SshState::load(&doc);
        // 真实 Save 流程:mutate_config 重载的磁盘文档是初始内容的
        // 超集,这里用同一份内容模拟。
        let mut out = doc_with_profiles(initial);
        state.apply(&mut out);

        let tables = profile_tables(&out);
        assert_eq!(tables.len(), 1);
        let t = tables[0];
        assert_eq!(t.get("password").and_then(|v| v.as_str()), Some("ENCRYPTED_BLOB"));
        assert_eq!(t.get("shortcut").and_then(|v| v.as_str()), Some("Ctrl+1"));
        assert_eq!(t.get("host").and_then(|v| v.as_str()), Some("10.0.0.1"));
    }

    #[test]
    fn sync_keeps_externally_added_profile() {
        // 打开设置时只有 old;Save 时磁盘多出 external(主进程
        // 新增)。apply 不得回滚掉 external。
        let doc = doc_with_profiles(
            r#"[[ssh.profiles]]
name = "old"
host = "h1"
"#,
        );
        let state = SshState::load(&doc);

        let mut fresh = doc_with_profiles(
            r#"[[ssh.profiles]]
name = "old"
host = "h1"
password = "ENC"

[[ssh.profiles]]
name = "external"
host = "h2"
password = "ENC2"
"#,
        );
        state.apply(&mut fresh);

        let tables = profile_tables(&fresh);
        assert_eq!(profile_names(&fresh), vec!["old", "external"],
            "external profile must survive save");
        assert_eq!(tables[1].get("password").and_then(|v| v.as_str()), Some("ENC2"));
    }

    #[test]
    fn sync_drops_profile_deleted_in_ui() {
        // 快照里有 a/b,UI 删除了 b → Save 时 b 丢弃;外部新增
        // 的 c 保留。
        let doc = doc_with_profiles(
            "[[ssh.profiles]]\nname = \"a\"\n[[ssh.profiles]]\nname = \"b\"\n",
        );
        let mut state = SshState::load(&doc);
        state.profiles.remove(1); // 删除 b
        state.loaded_names = vec!["a".into(), "b".into()];

        let mut fresh = doc_with_profiles(
            "[[ssh.profiles]]\nname = \"a\"\n[[ssh.profiles]]\nname = \"b\"\n[[ssh.profiles]]\nname = \"c\"\n",
        );
        state.apply(&mut fresh);

        assert_eq!(profile_names(&fresh), vec!["a", "c"]);
    }

    #[test]
    fn sync_handles_duplicate_names_pairwise() {
        // 两个同名 profile:按出现次序配对,各自携带各自的
        // 不可见字段,不再被 HashMap 覆盖。
        let initial = r#"[[ssh.profiles]]
name = "dup"
host = "h1"
password = "P1"

[[ssh.profiles]]
name = "dup"
host = "h2"
password = "P2"
"#;
        let doc = doc_with_profiles(initial);
        let state = SshState::load(&doc);
        let mut out = doc_with_profiles(initial);
        state.apply(&mut out);

        let tables = profile_tables(&out);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].get("password").and_then(|v| v.as_str()), Some("P1"));
        assert_eq!(tables[1].get("password").and_then(|v| v.as_str()), Some("P2"));
    }
}
