//! World-local native preset editor, using the game's existing buttons and sliders.
use super::{Button, Mode, Title};
use crate::{
    hud::{HudAtlas, HudBatch},
    lang,
    save::custom_presets::{self, PresetSetting, RegisteredPreset, SettingKind},
    text::Text,
    ui,
};
use horizon_world::nbt::Tag;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct State {
    pub preset: RegisteredPreset,
    pub value: Value,
    fields: Vec<PresetSetting>,
    page: usize,
    dragging: Option<u8>,
    error: Option<String>,
}
impl State {
    pub(super) fn new(preset: RegisteredPreset, stem: Option<&Tag>) -> Result<Self, String> {
        let editor = custom_presets::editor(preset).ok_or("Custom preset editor unavailable")?;
        let value = match stem.map(|s| (editor.settings)(s)).transpose()?.flatten() {
            Some(value) => value,
            None => (editor.defaults)()?,
        };
        custom_presets::generate_custom(preset, &value)?;
        let fields = (editor.fields)();
        for field in &fields {
            let valid = match (field.kind, value.pointer(field.path)) {
                (SettingKind::Boolean, Some(value)) => value.is_boolean(),
                (
                    SettingKind::Number {
                        min, max, integer, ..
                    },
                    Some(value),
                ) => value.as_f64().is_some_and(|n| {
                    n.is_finite()
                        && (min..=max).contains(&n)
                        && (!integer || value.as_i64().is_some())
                }),
                _ => false,
            };
            if !valid {
                return Err(format!("Invalid editor field {}", field.path));
            }
        }
        Ok(Self {
            preset,
            value,
            fields,
            page: 0,
            dragging: None,
            error: None,
        })
    }
    fn capacity(h: f32) -> usize {
        (((h.ceil() - 128.0) / 24.0).floor().max(1.0) as usize) * 2
    }
    fn pages(&self, h: f32) -> usize {
        self.fields.len().div_ceil(Self::capacity(h))
    }
    fn page(&self, h: f32) -> usize {
        self.page.min(self.pages(h) - 1)
    }
    fn label(&self, index: usize) -> String {
        let field = &self.fields[index];
        let value = self.value.pointer(field.path).unwrap();
        let display = match field.kind {
            SettingKind::Boolean => lang::get(if value.as_bool().unwrap() {
                "options.on"
            } else {
                "options.off"
            }),
            SettingKind::Number { integer: true, .. } => value.as_i64().unwrap().to_string(),
            SettingKind::Number { .. } => format!("{:.2}", value.as_f64().unwrap()),
        };
        format!("{}: {display}", lang::get(field.label))
    }
    fn set_number(&mut self, index: usize, number: f64) {
        if let SettingKind::Number {
            min,
            max,
            step,
            integer,
        } = self.fields[index].kind
        {
            let number = (min + ((number - min) / step).round() * step).clamp(min, max);
            let number = (number * 1_000_000.0).round() / 1_000_000.0;
            let value = if integer {
                Value::from(number.round() as i64)
            } else {
                Value::from(number)
            };
            *self.value.pointer_mut(self.fields[index].path).unwrap() = value;
            self.error = None;
        }
    }
    pub(super) fn adjust(&mut self, id: u8, direction: f64) -> bool {
        let Some(index) = id
            .checked_sub(100)
            .map(usize::from)
            .filter(|&i| i < self.fields.len())
        else {
            return false;
        };
        if let SettingKind::Number { step, .. } = self.fields[index].kind {
            self.set_number(
                index,
                self.value
                    .pointer(self.fields[index].path)
                    .unwrap()
                    .as_f64()
                    .unwrap()
                    + step * direction,
            );
            return true;
        }
        false
    }
    pub(super) fn drag(&mut self, cursor: (f32, f32), size: (f32, f32)) {
        if let Some(id) = self.dragging
            && let Some(button) = buttons(size.0, size.1, self)
                .into_iter()
                .find(|b| b.id == id)
            && let SettingKind::Number { min, max, .. } = self.fields[usize::from(id - 100)].kind
        {
            self.set_number(
                usize::from(id - 100),
                min + f64::from(ui::slider_value(button.x, button.w, cursor.0)) * (max - min),
            );
        }
    }
    pub(super) fn release(&mut self) {
        self.dragging = None;
    }
}

pub(super) fn buttons(w: f32, h: f32, state: &State) -> Vec<Button> {
    let x = ((w.ceil() - 310.0) / 2.0).floor();
    let capacity = State::capacity(h);
    let start = state.page(h) * capacity;
    let button = |x, y, width, label, enabled, id| Button {
        x,
        y,
        w: width,
        label,
        enabled,
        id,
        icon: None,
        trailing: None,
    };
    let mut buttons: Vec<_> = (start..(start + capacity).min(state.fields.len()))
        .enumerate()
        .map(|(at, index)| {
            button(
                x + (at % 2) as f32 * 160.0,
                42.0 + (at / 2) as f32 * 24.0,
                150.0,
                state.label(index),
                true,
                100 + index as u8,
            )
        })
        .collect();
    buttons.extend([
        button(
            x,
            h.ceil() - 51.0,
            74.0,
            lang::get("horizon.customize.previous"),
            state.page(h) > 0,
            80,
        ),
        button(
            x + 80.0,
            h.ceil() - 51.0,
            150.0,
            lang::get("controls.reset"),
            true,
            81,
        ),
        button(
            x + 236.0,
            h.ceil() - 51.0,
            74.0,
            lang::get("horizon.customize.next"),
            state.page(h) + 1 < state.pages(h),
            82,
        ),
        button(x, h.ceil() - 27.0, 150.0, lang::get("gui.done"), true, 83),
        button(
            x + 160.0,
            h.ceil() - 27.0,
            150.0,
            lang::get("gui.cancel"),
            true,
            84,
        ),
    ]);
    buttons
}
pub(super) fn draw(atlas: &HudAtlas, batch: &mut HudBatch, state: &State) {
    ui::menu_background(
        batch,
        atlas,
        0.0,
        0.0,
        batch.width.ceil(),
        batch.height.ceil(),
        false,
    );
    let label = Text::translate(
        "horizon.customize.title",
        &[Text::translate(state.preset.label(), &[])],
    )
    .plain();
    super::centered(atlas, batch, &label, 10.0, u32::MAX);
    let page = Text::translate(
        "book.pageIndicator",
        &[
            Text::literal((state.page(batch.height) + 1).to_string()),
            Text::literal(state.pages(batch.height).to_string()),
        ],
    )
    .plain();
    super::centered(atlas, batch, &page, 27.0, u32::MAX);
    if state.error.is_some() {
        super::centered(
            atlas,
            batch,
            &lang::get("horizon.customize.error"),
            batch.height.ceil() - 67.0,
            crate::hud::rgba(255, 100, 100, 255),
        );
    }
}
pub(super) fn draw_slider(
    title: &Title,
    atlas: &HudAtlas,
    batch: &mut HudBatch,
    cursor: (f32, f32),
    state: &State,
    button: &Button,
) -> bool {
    let Some(index) = button
        .id
        .checked_sub(100)
        .map(usize::from)
        .filter(|&i| i < state.fields.len())
    else {
        return false;
    };
    let SettingKind::Number { min, max, .. } = state.fields[index].kind else {
        return false;
    };
    let value = state
        .value
        .pointer(state.fields[index].path)
        .unwrap()
        .as_f64()
        .unwrap();
    let hovered = ui::hovered(cursor, button.x, button.y, button.w, 20.0);
    let focused = title.focused_button == Some(button.id);
    ui::slider_ext(
        batch,
        atlas,
        button.x,
        button.y,
        button.w,
        &Text::literal(button.label.clone()),
        ((value - min) / (max - min)) as f32,
        hovered,
        focused,
        focused,
        true,
        state.dragging == Some(button.id),
    );
    crate::widgets::widget_cursor(hovered, true);
    true
}

impl Title {
    pub(super) fn open_custom_preset(&mut self) {
        let Mode::Create { options, .. } = &self.mode else {
            return;
        };
        let crate::save::WorldPreset::Custom(preset) = options.world_type else {
            return;
        };
        let state = State::new(preset, options.custom_overworld().as_ref());
        let return_to = Box::new(std::mem::replace(&mut self.mode, Mode::Main));
        self.mode = match state {
            Ok(state) => Mode::CreateCustom { return_to, state },
            Err(message) => Mode::Error {
                message,
                return_to,
                alert: None,
            },
        };
        self.focused_button = None;
    }
    pub(super) fn close_custom_preset(&mut self, apply: bool) {
        let stem = if apply {
            let Mode::CreateCustom { state, .. } = &mut self.mode else {
                return;
            };
            match custom_presets::generate_custom(state.preset, &state.value) {
                Ok(stem) => Some(stem),
                Err(error) => {
                    log::warn!("Custom preset validation: {error}");
                    state.error = Some(error);
                    return;
                }
            }
        } else {
            None
        };
        if let Mode::CreateCustom { mut return_to, .. } =
            std::mem::replace(&mut self.mode, Mode::Main)
        {
            if let Some(stem) = stem
                && let Mode::Create { options, .. } = return_to.as_mut()
            {
                options.custom_overworld = Some(stem);
            }
            self.mode = *return_to;
            self.focused_button = Some(26);
        }
    }
    pub(super) fn custom_preset_click(
        &mut self,
        hit: Option<u8>,
        mouse_x: Option<f32>,
        size: (f32, f32),
    ) {
        if hit == Some(83) {
            self.close_custom_preset(true);
            return;
        }
        if hit == Some(84) {
            self.close_custom_preset(false);
            return;
        }
        let Mode::CreateCustom { state, .. } = &mut self.mode else {
            return;
        };
        state.dragging = None;
        match hit {
            Some(80) => state.page = state.page(size.1).saturating_sub(1),
            Some(82) => state.page = (state.page(size.1) + 1).min(state.pages(size.1) - 1),
            Some(81) => match State::new(state.preset, None) {
                Ok(mut reset) => {
                    reset.page = state.page(size.1);
                    *state = reset;
                }
                Err(error) => {
                    log::warn!("Custom preset reset: {error}");
                    state.error = Some(error);
                }
            },
            Some(id) if id >= 100 && usize::from(id - 100) < state.fields.len() => {
                let field = state.fields[usize::from(id - 100)];
                match field.kind {
                    SettingKind::Boolean => {
                        let v = state.value.pointer_mut(field.path).unwrap();
                        *v = Value::Bool(!v.as_bool().unwrap());
                        state.error = None;
                    }
                    SettingKind::Number { .. } => {
                        if let Some(x) = mouse_x {
                            state.dragging = Some(id);
                            state.drag((x, 0.0), size);
                        } else {
                            state.adjust(id, 1.0);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
