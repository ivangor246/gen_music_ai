//! Built-in and user presets, mirroring the Python `PresetStore`.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::paths::presets_file;
use crate::settings::GenerationSettings;

#[derive(Clone)]
pub struct Preset {
    pub name: String,
    pub settings: GenerationSettings,
    pub built_in: bool,
}

#[derive(Serialize, Deserialize)]
struct StoredPreset {
    name: String,
    settings: GenerationSettings,
}

fn builtin(
    name: &str,
    instruments: &[&str],
    drum_kit: &str,
    bpm: u16,
    time_signature: &str,
    bars: u32,
    events_per_bar: u32,
    temperature: f32,
    top_p: f32,
    top_k: usize,
) -> Preset {
    Preset {
        name: name.to_string(),
        settings: GenerationSettings {
            instruments: instruments.iter().map(|s| s.to_string()).collect(),
            drum_kit: drum_kit.to_string(),
            bpm,
            time_signature: time_signature.to_string(),
            bars,
            events_per_bar,
            temperature,
            top_p,
            top_k,
            ..GenerationSettings::default()
        },
        built_in: true,
    }
}

fn default_presets() -> Vec<Preset> {
    vec![
        builtin("Баллада", &["Bright Acoustic", "Cello", "String Ensemble 1"], "Нет", 76, "4/4", 24, 20, 0.88, 0.92, 18),
        builtin("Биосферный эмбиент", &["Pad 1 (new age)", "Pad 7 (halo)", "FX 4 (atmosphere)", "Synth Voice"], "Нет", 64, "4/4", 24, 18, 0.9, 0.96, 24),
        builtin("Джазовый квартет", &["Acoustic Grand", "Acoustic Bass", "Tenor Sax"], "Джазовая", 132, "4/4", 32, 30, 1.0, 0.95, 32),
        builtin("Загадочный саундтрек", &["Pizzicato Strings", "Clarinet", "Pad 1 (new age)"], "Нет", 72, "3/4", 24, 24, 1.02, 0.97, 36),
        builtin("Кинематографический эпос", &["String Ensemble 1", "Cello", "French Horn", "Tuba", "Orchestra Hit"], "Оркестровая", 104, "4/4", 32, 38, 0.94, 0.95, 30),
        builtin("Классическое фортепиано", &["Acoustic Grand"], "Нет", 84, "4/4", 24, 18, 0.85, 0.92, 16),
        builtin("Лоу-фай хип-хоп", &["Electric Piano 1", "Acoustic Bass", "Pad 2 (warm)"], "Мягкая", 78, "4/4", 32, 26, 0.96, 0.95, 28),
        builtin("Медитативная музыка", &["Flute", "Pad 1 (new age)", "Pad 2 (warm)"], "Нет", 60, "4/4", 24, 16, 0.82, 0.92, 16),
        builtin("Неоновая заставка", &["Electric Piano 2", "Pad 3 (polysynth)", "FX 3 (crystal)", "Synth Bass 2"], "Электронная", 92, "4/4", 24, 24, 0.94, 0.95, 28),
        builtin("Пульсирующее напряжение", &["Pizzicato Strings", "Synth Bass 2", "Pad 6 (metallic)", "Reverse Cymbal"], "Электронная", 122, "4/4", 32, 34, 1.03, 0.97, 38),
        builtin("Рок", &["Electric Guitar(clean)", "Overdriven Guitar", "Electric Bass(finger)"], "Стандартная", 120, "4/4", 32, 30, 1.0, 0.94, 24),
        builtin("Симфонический оркестр", &["Acoustic Grand", "String Ensemble 1", "Cello", "French Horn", "Flute", "Oboe"], "Оркестровая", 96, "4/4", 32, 34, 0.95, 0.96, 28),
        builtin("Тактический синтвейв", &["Lead 8 (bass+lead)", "Pad 3 (polysynth)", "Synth Bass 1", "Orchestra Hit"], "TR-808", 116, "4/4", 32, 32, 1.0, 0.96, 34),
        builtin("Тревожный саундтрек", &["Tremolo Strings", "Pizzicato Strings", "Synth Bass 1", "Orchestra Hit"], "Оркестровая", 116, "7/8", 32, 36, 1.08, 0.97, 42),
        builtin("Фанк", &["Electric Guitar(muted)", "Electric Bass(finger)", "Electric Piano 1"], "Комнатная", 112, "4/4", 32, 34, 1.0, 0.94, 28),
        builtin("Фолк", &["Acoustic Guitar(steel)", "Violin", "Flute"], "Нет", 104, "6/8", 24, 24, 0.92, 0.94, 24),
        builtin("Чиптюн", &["Lead 2 (sawtooth)", "Lead 5 (charang)", "Synth Bass 1"], "Электронная", 150, "4/4", 32, 32, 1.02, 0.96, 36),
        builtin("Электронная музыка", &["Lead 2 (sawtooth)", "Pad 2 (warm)", "Synth Bass 1"], "TR-808", 128, "4/4", 32, 32, 1.05, 0.97, 40),
        builtin("Эмбиент", &["Pad 1 (new age)", "Pad 2 (warm)", "SynthStrings 1"], "Нет", 70, "4/4", 24, 20, 0.9, 0.96, 24),
    ]
}

pub struct PresetStore {
    user: Vec<Preset>,
}

impl PresetStore {
    pub fn load() -> Self {
        let user = std::fs::read_to_string(presets_file())
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<StoredPreset>>(&text).ok())
            .map(|items| {
                items
                    .into_iter()
                    .map(|item| Preset {
                        name: item.name,
                        settings: item.settings,
                        built_in: false,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { user }
    }

    /// All presets (built-in + user), sorted case-insensitively by name.
    pub fn all(&self) -> Vec<Preset> {
        let mut presets = default_presets();
        presets.extend(self.user.iter().cloned());
        presets.sort_by_key(|preset| preset.name.to_lowercase());
        presets
    }

    pub fn get(&self, name: &str) -> Option<Preset> {
        self.all().into_iter().find(|preset| preset.name == name)
    }

    pub fn save(&mut self, name: &str, settings: GenerationSettings) -> Result<()> {
        let clean = name.trim();
        if clean.is_empty() {
            bail!("Введите название пресета.");
        }
        if default_presets().iter().any(|preset| preset.name == clean) {
            bail!("Название занято стандартным пресетом.");
        }
        self.user.retain(|preset| preset.name != clean);
        self.user.push(Preset {
            name: clean.to_string(),
            settings,
            built_in: false,
        });
        self.persist();
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<()> {
        if !self.user.iter().any(|preset| preset.name == name) {
            bail!("Стандартные пресеты удалить нельзя.");
        }
        self.user.retain(|preset| preset.name != name);
        self.persist();
        Ok(())
    }

    fn persist(&self) {
        let path = presets_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut items: Vec<StoredPreset> = self
            .user
            .iter()
            .map(|preset| StoredPreset {
                name: preset.name.clone(),
                settings: preset.settings.clone(),
            })
            .collect();
        items.sort_by_key(|item| item.name.to_lowercase());
        if let Ok(text) = serde_json::to_string_pretty(&items) {
            std::fs::write(path, text).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::core::midi::gm::patch_number;

    #[test]
    fn built_in_presets_have_unique_names_and_valid_instruments() {
        let presets = default_presets();
        let unique_names: HashSet<_> = presets.iter().map(|preset| &preset.name).collect();

        assert_eq!(unique_names.len(), presets.len());
        for preset in presets {
            assert!(preset.built_in);
            assert!(!preset.settings.instruments.is_empty());
            for instrument in preset.settings.instruments {
                assert!(
                    patch_number(&instrument).is_some(),
                    "unknown instrument in preset {}: {instrument}",
                    preset.name
                );
            }
        }
    }
}
