//! Per-model WorkBuddy capability editor, independent from request overrides.
use crate::capabilities::{ContextWindow, EFFORTS, ModelCapabilities};
use gpui_kit::component::{
    input::InputState,
    select::{SelectItem, SelectState},
};
use gpui_kit::*;
use serde_json::Value;

#[derive(Clone)]
pub struct EffortChoice {
    code: String,
    title: SharedString,
}
impl SelectItem for EffortChoice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &String {
        &self.code
    }
}
fn choices(zh: bool) -> Vec<EffortChoice> {
    EFFORTS
        .iter()
        .zip([
            ("低", "Low"),
            ("中", "Medium"),
            ("高", "High"),
            ("超高", "Extra high"),
            ("极致", "Max"),
        ])
        .map(|(code, (cn, en))| EffortChoice {
            code: (*code).into(),
            title: (if zh { cn } else { en }).into(),
        })
        .collect()
}
pub struct OptionsEditor {
    pub automatic: bool,
    pub source: String,
    pub image_known: bool,
    pub value: ModelCapabilities,
    pub default_effort: Entity<SelectState<Vec<EffortChoice>>>,
    pub(crate) limits: [Entity<InputState>; 4],
}
impl OptionsEditor {
    pub fn new(zh: bool, window: &mut Window, cx: &mut App) -> Self {
        Self {
            automatic: true,
            source: "unknown".into(),
            image_known: false,
            value: ModelCapabilities::default(),
            default_effort: cx.new(|cx| SelectState::new(choices(zh), None, window, cx)),
            limits: std::array::from_fn(|_| cx.new(|cx| InputState::new(window, cx))),
        }
    }
    pub fn load(&mut self, row: &Value, model: &str, zh: bool, window: &mut Window, cx: &mut App) {
        let stored = row
            .get("capabilities")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        self.automatic = stored.is_none();
        self.value = stored.unwrap_or_else(|| ModelCapabilities::for_model(model));
        self.source = if !self.automatic {
            "manual"
        } else if self.value != ModelCapabilities::default() {
            "builtin"
        } else {
            "unknown"
        }
        .into();
        self.image_known = self.source != "unknown";
        self.refresh(zh, window, cx);
    }
    pub fn apply_automatic(
        &mut self,
        metadata: &Value,
        zh: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Ok(value) =
            serde_json::from_value::<ModelCapabilities>(metadata["capabilities"].clone())
        else {
            return false;
        };
        if !self.automatic || value.validate().is_err() {
            return false;
        }
        let source = metadata["sources"]
            .as_array()
            .and_then(|s| s.last())
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let known = metadata["image_known"].as_bool().unwrap_or(false);
        if self.value == value && self.source == source && self.image_known == known {
            return false;
        }
        self.value = value;
        self.source = source.to_owned();
        self.image_known = known;
        self.refresh(zh, window, cx);
        true
    }
    pub fn refresh(&mut self, zh: bool, window: &mut Window, cx: &mut App) {
        let window_values = self.value.context_window.as_ref();
        let values = [
            self.value
                .max_input_tokens
                .map(|n| n.to_string())
                .unwrap_or_default(),
            self.value
                .max_output_tokens
                .map(|n| n.to_string())
                .unwrap_or_default(),
            window_values
                .map(|w| w.default_length.to_string())
                .unwrap_or_default(),
            window_values
                .map(|w| {
                    w.supported_lengths
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
        ];
        for (i, (state, value)) in self.limits.iter().zip(values).enumerate() {
            state.update(cx, |s, cx| {
                s.set_placeholder(
                    (if i == 3 {
                        if zh {
                            "例如 128000, 300000, 1000000"
                        } else {
                            "e.g. 128000, 300000, 1000000"
                        }
                    } else if zh {
                        "留空使用默认值"
                    } else {
                        "Provider default"
                    })
                    .to_owned(),
                    window,
                    cx,
                );
                s.set_value(value, window, cx);
            });
        }
        self.default_effort.update(cx, |s, cx| {
            s.set_items(choices(zh), window, cx);
            s.set_selected_value(&self.value.reasoning.default_effort, window, cx);
        });
    }
    pub fn read(&self, cx: &App) -> Result<Option<ModelCapabilities>, String> {
        if self.automatic {
            return Ok(None);
        }
        let mut value = self.value.clone();
        value.max_input_tokens = number(&self.limits[0].read(cx).value())?;
        value.max_output_tokens = number(&self.limits[1].read(cx).value())?;
        let default = number(&self.limits[2].read(cx).value())?;
        let raw = self.limits[3].read(cx).value();
        let lengths = raw
            .split([',', '，'])
            .filter(|s| !s.trim().is_empty())
            .map(|s| number(s).map(|n| n.unwrap()))
            .collect::<Result<Vec<_>, _>>()?;
        value.context_window = match default {
            Some(default_length) => Some(ContextWindow {
                default_length,
                supported_lengths: if lengths.is_empty() {
                    vec![default_length]
                } else {
                    lengths
                },
            }),
            None if lengths.is_empty() => None,
            None => return Err("请填写默认上下文长度 / Enter the default context length".into()),
        };
        value.validate()?;
        Ok(Some(value))
    }
}
fn number(text: &str) -> Result<Option<u32>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
        .map(Some)
        .ok_or_else(|| {
            "Token 数须为正整数，留空使用默认值 / Token counts must be positive integers or empty"
                .into()
        })
}

#[cfg(test)]
mod tests {
    use super::number;
    #[test]
    fn token_limits_cannot_silently_fall_back_on_invalid_input() {
        assert_eq!(number(" ").unwrap(), None);
        assert_eq!(number("1000000").unwrap(), Some(1_000_000));
        for input in ["0", "-1", "1.5", "4294967296", "many"] {
            assert!(number(input).is_err());
        }
    }
}
