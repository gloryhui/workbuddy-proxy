//! 设置窗口的纯业务状态：供应商编辑草稿、草稿集合和模型列表选择。
//!
//! 这些类型由 `ui::Proxy` 持有并在 GPUI UI 线程修改。`Config` 表示 Core
//! 已保存的配置，`Draft` 表示当前窗口中尚未应用的一组输入；草稿单独写入
//! `gpui-drafts.json`，不会因每次键盘输入就调用管理 HTTP。本模块没有 GPUI
//! 类型，因此也能脱离窗口做确定性测试。

use crate::config::{self, Config};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    /// 用户可见的供应商名称；新建本地草稿时允许为空。
    pub name: String,
    /// 上游 API key。空值在既有保存语义中有特定含义，不能自动替换为保存值。
    pub key: String,
    /// 上游基础 URL，协议相关后缀由 Core/管理命令解释。
    pub upstream: String,
    /// 新旧持久化文件反序列化时缺省为默认协议，便于继续读取旧草稿。
    #[serde(default)]
    pub protocol: crate::upstream_config::UpstreamProtocol,
    /// 协议专属设置；与 `protocol` 一起构成测试/保存身份的一部分。
    #[serde(default)]
    pub protocol_settings: crate::upstream_config::ProtocolSettings,
    /// `keep` 表示沿用每条模型自己的思考配置，其他值表示统一覆盖。
    pub effort: String,
    /// Core `Model` 行的 JSON 投影。这里显式携带当前保存语义认识的 id、名称、
    /// 模型 ID、思考强度和能力字段；`Value` 不承诺任意未知字段都无损往返。
    pub models: Vec<Value>,
}
impl Draft {
    /// Pure projection for one supplier; unlike `Drafts::from_config`, this
    /// does not fingerprint/read the configuration file. Renderers may compare
    /// the current draft with this saved snapshot without doing frame-time IO.
    pub fn from_provider(provider: &config::Provider) -> Self {
        Self {
            name: if provider.is_draft() { String::new() } else { provider.name.clone() },
            key: provider.api_key.clone(),
            upstream: provider.upstream.clone(),
            protocol: provider.protocol,
            protocol_settings: provider.protocol_settings.clone(),
            effort: if provider.models.iter().all(|model| provider.models.first().is_some_and(|first| first.reasoning_effort == model.reasoning_effort)) {
                provider.models.first().map(|model| model.reasoning_effort.clone()).unwrap_or_else(|| "keep".into())
            } else { "keep".into() },
            models: provider.models.iter().map(|model| json!({
                "id":model.id, "name":model.name, "model":model.model,
                "reasoning_effort":model.reasoning_effort, "capabilities":model.capabilities,
            })).collect(),
        }
    }

    /// 生成保存命令。此函数只构造 JSON，不执行 I/O；真正的后台步骤由
    /// `backend::Backend` 所属 worker 接收，新供应商则由 `provider_create`
    /// 进入本地创建流程。
    pub fn payload(&self, id: &str) -> Value {
        json!({"command":"save","provider_id":id,"name":self.name,"key":self.key,"upstream":self.upstream,"protocol":self.protocol,"protocol_settings":self.protocol_settings,"reasoning_effort":self.effort,"models":self.models})
    }
}
#[derive(Default, Serialize, Deserialize)]
pub struct Drafts {
    /// 草稿创建时读取的 `config.json` 指纹。冷启动只在指纹匹配时恢复草稿，
    /// 防止把外部保存前的旧输入覆盖到新配置。
    pub base: String,
    /// 当前显示的供应商 ID；空字符串用于“尚无已保存供应商”的页面状态。
    pub selected: String,
    /// 按供应商 ID 保存的本地编辑快照。`BTreeMap` 的确定性遍历顺序不表示
    /// 业务优先级。
    pub providers: BTreeMap<String, Draft>,
    /// 新供应商创建流程。它记录本地临时 ID、返回位置、复制来源及已创建/
    /// 不确定状态；普通已保存供应商没有该字段，因此通常为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<crate::provider_create::Pending>,
}
impl Drafts {
    fn fingerprint(dir: &Path) -> String {
        format!(
            "{:x}",
            Sha256::digest(std::fs::read(dir.join("config.json")).unwrap_or_default())
        )
    }
    /// 载入窗口草稿。文件不存在、超过 4 MiB、JSON 无效或配置指纹已变化时，
    /// 回退为当前 `Config` 的干净投影；不会在这次读取路径中主动迁移配置。
    ///
    /// 恢复后会移除已经失效的供应商和不合法的 pending 创建记录，保留本地尚未
    /// 创建的草稿，以及仍与 Core 配置/pending 创建记录匹配的编辑。
    pub fn load(dir: &Path, cfg: &Config) -> Self {
        let stored = std::fs::read(dir.join("gpui-drafts.json"))
            .ok()
            .filter(|s| s.len() <= 4 * 1024 * 1024)
            .and_then(|s| {
                let mut value: Value = serde_json::from_slice(&s).ok()?;
                if let Some(providers) = value.get_mut("providers").and_then(Value::as_object_mut) {
                    for provider in &cfg.providers {
                        if let Some(draft) = providers
                            .get_mut(&provider.id)
                            .and_then(Value::as_object_mut)
                        {
                            draft
                                .entry("protocol")
                                .or_insert_with(|| json!(provider.protocol));
                            draft
                                .entry("protocol_settings")
                                .or_insert_with(|| json!(provider.protocol_settings));
                        }
                    }
                }
                serde_json::from_value::<Self>(value).ok()
            })
            .filter(|s| s.base == Self::fingerprint(dir));
        let mut loaded = stored.unwrap_or_else(|| Self::from_config(dir, cfg));
        if loaded.pending.as_ref().is_some_and(|pending| {
            !loaded.providers.contains_key(&pending.id)
                || (cfg.providers.iter().any(|p| p.id == pending.id) != pending.created)
                || pending.id.len() > 64
                || !pending
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            loaded.pending = None;
        }
        loaded.providers.retain(|id, _| {
            cfg.providers.iter().any(|p| p.id == *id)
                || loaded.pending.as_ref().is_some_and(|p| p.id == *id)
        });
        loaded
    }
    /// 从可信的已保存配置建立全部干净草稿。这是“没有本地未保存修改”的
    /// 基线，而不是 Core 当前路由或运行时状态的副本。
    pub fn from_config(dir: &Path, cfg: &Config) -> Self {
        Self {
            base: Self::fingerprint(dir),
            selected: cfg
                .providers
                .first()
                .map(|provider| provider.id.clone())
                .unwrap_or_default(),
            providers: cfg
                .providers
                .iter()
                .map(|provider| (provider.id.clone(), Draft::from_provider(provider)))
                .collect(),
            pending: None,
        }
    }
    /// 先刷新 `base` 指纹再写 `gpui-drafts.json`，使下次启动能判断草稿是否
    /// 基于当前配置。`&mut self` 是排他借用，调用期间同一 `Drafts` 不能
    /// 被另一条 Rust 借用路径同时修改。
    pub fn persist(&mut self, dir: &Path) -> Result<(), String> {
        self.base = Self::fingerprint(dir);
        config::write_json(&dir.join("gpui-drafts.json"), self)
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct Selection {
    /// 已选模型行号，使用集合保证去重；删除时按逆序处理以保持其余索引稳定。
    pub rows: BTreeSet<usize>,
    /// Shift 范围选择的锚点行；鼠标点击与键盘扩展共享它。
    pub anchor: usize,
    /// 键盘操作的活动行。它与 `rows` 分开，才能在取消选中当前行后仍找到
    /// 下一个编辑目标；没有可用行时为 `None`。
    pub cursor: Option<usize>,
}
#[derive(Clone, Copy)]
pub enum ModelMove {
    Previous,
    Next,
    First,
    Last,
}
impl Selection {
    /// 鼠标/键盘共同的选择入口：`extend` 对应 Shift，`toggle` 对应
    /// Ctrl/Cmd。本函数只修改 `rows/anchor/cursor`，不修改模型数据或 `Draft`；
    /// 调用方按具体事件决定何时 `capture` 和重绘。
    pub fn click(&mut self, row: usize, extend: bool, toggle: bool) {
        self.cursor = Some(row);
        if extend {
            if !toggle {
                self.rows.clear();
            }
            self.rows
                .extend(self.anchor.min(row)..=self.anchor.max(row));
        } else if toggle {
            if !self.rows.remove(&row) {
                self.rows.insert(row);
            }
            self.anchor = row;
        } else {
            self.rows.clear();
            self.rows.insert(row);
            self.anchor = row;
        }
    }
    pub fn select_all(&mut self, count: usize) {
        self.rows = (0..count).collect();
        self.cursor = self
            .cursor
            .filter(|row| *row < count)
            .or((count > 0).then_some(0));
        self.anchor = self.cursor.unwrap_or(0);
    }
    pub fn edit_row(&self) -> Option<usize> {
        self.cursor
            .filter(|row| self.rows.contains(row))
            .or_else(|| self.rows.first().copied())
    }
    /// Keyboard range extension keeps the mouse anchor and moves a separate
    /// active row, so reversing direction shrinks the range instead of jumping.
    pub fn navigate(&mut self, movement: ModelMove, count: usize, extend: bool) -> Option<usize> {
        if count == 0 {
            self.rows.clear();
            self.cursor = None;
            self.anchor = 0;
            return None;
        }
        let previous = self.cursor.filter(|row| *row < count);
        let target = match movement {
            ModelMove::Previous => previous.map_or(count - 1, |row| row.saturating_sub(1)),
            ModelMove::Next => previous.map_or(0, |row| (row + 1).min(count - 1)),
            ModelMove::First => 0,
            ModelMove::Last => count - 1,
        };
        if extend && (previous.is_none() || self.anchor >= count) {
            self.anchor = previous.unwrap_or(target);
        }
        self.click(target, extend, false);
        Some(target)
    }
    /// 在当前模型列表长度内删除选中的行。逆序删除避免前面一行的移除导致
    /// 后面保存的行号偏移，然后清空锚点与活动行。
    pub fn remove(&mut self, models: &mut Vec<Value>) {
        for row in self.rows.iter().rev() {
            if *row < models.len() {
                models.remove(*row);
            }
        }
        self.rows.clear();
        self.cursor = None;
        self.anchor = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_ranges_extend_and_shrink_from_the_mouse_anchor() {
        let mut selection = Selection::default();
        selection.click(2, false, false);
        selection.navigate(ModelMove::Next, 6, true);
        selection.navigate(ModelMove::Next, 6, true);
        assert_eq!(selection.rows, BTreeSet::from([2, 3, 4]));
        assert_eq!(selection.edit_row(), Some(4));
        assert_eq!(selection.anchor, 2);
        selection.navigate(ModelMove::Previous, 6, true);
        assert_eq!(selection.rows, BTreeSet::from([2, 3]));
        assert_eq!(selection.edit_row(), Some(3));
        selection.navigate(ModelMove::Previous, 6, true);
        selection.navigate(ModelMove::Previous, 6, true);
        assert_eq!(selection.rows, BTreeSet::from([1, 2]));
        assert_eq!(selection.cursor, Some(1));
        selection.navigate(ModelMove::Next, 6, false);
        assert_eq!(selection.rows, BTreeSet::from([2]));
        assert_eq!(selection.anchor, 2);
    }

    #[test]
    fn keyboard_navigation_handles_empty_boundaries_and_shortened_lists() {
        let mut selection = Selection::default();
        assert_eq!(selection.navigate(ModelMove::Previous, 0, true), None);
        assert_eq!(selection.navigate(ModelMove::Next, 3, false), Some(0));
        assert_eq!(selection.navigate(ModelMove::Previous, 3, false), Some(0));
        selection.navigate(ModelMove::Last, 3, false);
        assert_eq!(selection.navigate(ModelMove::Next, 3, false), Some(2));
        selection.navigate(ModelMove::First, 3, true);
        assert_eq!(selection.rows, BTreeSet::from([0, 1, 2]));
        selection.click(8, false, false);
        assert_eq!(selection.navigate(ModelMove::Previous, 2, true), Some(1));
        assert_eq!(selection.rows, BTreeSet::from([1]));
        selection.navigate(ModelMove::Last, 0, false);
        assert!(selection.rows.is_empty());
        assert!(selection.cursor.is_none());
        assert_eq!(selection.anchor, 0);
    }

    #[test]
    fn select_all_keeps_the_active_row_and_delete_resets_keyboard_state() {
        let mut selection = Selection::default();
        selection.click(3, false, false);
        selection.select_all(5);
        assert_eq!(selection.cursor, Some(3));
        assert_eq!(selection.rows, BTreeSet::from([0, 1, 2, 3, 4]));
        selection.navigate(ModelMove::Previous, 5, true);
        assert_eq!(selection.rows, BTreeSet::from([2, 3]));
        let mut models = (0..5).map(|i| json!({"model":i})).collect();
        selection.remove(&mut models);
        assert_eq!(
            models,
            vec![json!({"model":0}), json!({"model":1}), json!({"model":4})]
        );
        assert!(selection.cursor.is_none());
        assert_eq!(selection.navigate(ModelMove::Next, 3, false), Some(0));
    }

    #[test]
    fn editing_uses_an_active_selected_row_and_handles_a_toggled_off_cursor() {
        let mut selection = Selection::default();
        selection.click(1, false, false);
        selection.click(4, true, false);
        assert_eq!(selection.edit_row(), Some(4));
        selection.click(4, false, true);
        assert_eq!(selection.edit_row(), Some(1));
        selection.select_all(5);
        assert_eq!(selection.edit_row(), Some(4));
        selection.select_all(0);
        assert_eq!(selection.edit_row(), None);
    }
    #[test]
    fn legacy_private_draft_inherits_current_protocol_without_losing_typed_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        let p = &mut cfg.providers[0];
        p.protocol = crate::upstream_config::UpstreamProtocol::AnthropicMessages;
        p.upstream = "https://example.test/v1".into();
        p.api_key = "fixture".into();
        let id = p.id.clone();
        config::write_json(&dir.path().join("config.json"), &cfg).unwrap();
        let drafts = Drafts::from_config(dir.path(), &cfg);
        let mut value = serde_json::to_value(drafts).unwrap();
        let draft = value["providers"][&id].as_object_mut().unwrap();
        draft.remove("protocol");
        draft.remove("protocol_settings");
        draft.insert("name".into(), json!("unsaved provider name"));
        config::write_json(&dir.path().join("gpui-drafts.json"), &value).unwrap();
        let loaded = Drafts::load(dir.path(), &cfg);
        let payload = loaded.providers[&id].payload(&id);
        assert_eq!(payload["protocol"], "anthropic_messages");
        assert_eq!(payload["name"], "unsaved provider name");
    }
    #[test]
    fn model_capabilities_survive_private_drafts_and_save_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.providers[0].set_model_rows(&[json!({
            "model":"custom-vision", "capabilities":{"supportsImages":true,"supportsToolCall":false}
        })], "default").unwrap();
        config::write_json(&dir.path().join("config.json"), &cfg).unwrap();
        let mut drafts = Drafts::from_config(dir.path(), &cfg);
        drafts.persist(dir.path()).unwrap();
        let restored = Drafts::load(dir.path(), &cfg);
        let provider = &cfg.providers[0].id;
        let payload = restored.providers[provider].payload(provider);
        assert_eq!(payload["models"][0]["capabilities"]["supportsImages"], true);
        assert_eq!(
            payload["models"][0]["capabilities"]["supportsToolCall"],
            false
        );
    }
    #[test]
    fn modifier_selection_and_batch_delete_keep_unselected_models() {
        let mut s = Selection::default();
        s.click(1, false, false);
        s.click(3, true, false);
        s.click(2, false, true);
        s.click(5, false, true);
        assert_eq!(s.rows, BTreeSet::from([1, 3, 5]));
        let mut rows = (0..6).map(|i| json!({"model":i})).collect();
        s.remove(&mut rows);
        assert_eq!(
            rows,
            vec![json!({"model":0}), json!({"model":2}), json!({"model":4})]
        );
    }
    #[test]
    fn drafts_survive_reopen_without_overwriting_saved_config_and_reject_stale_state() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(dir.path()).unwrap();
        let old = std::fs::read(dir.path().join("config.json")).unwrap();
        let mut drafts = Drafts::from_config(dir.path(), &cfg);
        drafts.providers.values_mut().next().unwrap().name = "unsaved".into();
        drafts.persist(dir.path()).unwrap();
        assert_eq!(std::fs::read(dir.path().join("config.json")).unwrap(), old);
        assert_eq!(
            Drafts::load(dir.path(), &cfg)
                .providers
                .values()
                .next()
                .unwrap()
                .name,
            "unsaved"
        );
        let mut updated = cfg.clone();
        updated.language = "en".into();
        config::write_json(&dir.path().join("config.json"), &updated).unwrap();
        assert_ne!(
            Drafts::load(dir.path(), &updated)
                .providers
                .values()
                .next()
                .unwrap()
                .name,
            "unsaved"
        );
    }
}
