//! Propositions visuelles natives : validation des demandes et choix explicite.

use std::collections::HashSet;

use anyhow::{Context, ensure};
use langchain_rust::schemas::FunctionDefinition;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use slint::{Model, ModelRc, VecModel};

use crate::{ChatWindow, ChoiceOptionRow, MessageRow, SwatchRow};

pub const OFFER_TOOL: &str = "lyra_offer_choices";
pub const PALETTE_TOOL: &str = "lyra_palette_catalog";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceRequest {
    pub question: String,
    pub options: Vec<ChoiceOption>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub colors: Vec<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub svg: Option<String>,
    #[serde(default)]
    pub source_url: String,
}

pub fn tool_definitions() -> [FunctionDefinition; 2] {
    [
        FunctionDefinition::new(
            OFFER_TOOL,
            "Affiche une question et 2 à 8 choix cliquables dans le chat. Chaque choix peut contenir du texte, une couleur ou une palette (colors), une image locale existante (image_path), ou un schéma SVG autonome (svg). Ajoutez source_url pour les inspirations externes. Après cet outil, attendez le clic de l'utilisateur : ne choisissez et n'appliquez rien à sa place.",
            json!({
                "type": "object", "required": ["question", "options"], "additionalProperties": false,
                "properties": {
                    "question": {"type": "string", "maxLength": 200},
                    "options": {"type": "array", "minItems": 2, "maxItems": 8, "items": {
                        "type": "object", "required": ["id", "label"], "additionalProperties": false,
                        "properties": {
                            "id": {"type": "string", "description": "Identifiant unique court, lettres/chiffres/-/_"},
                            "label": {"type": "string", "maxLength": 80},
                            "description": {"type": "string", "maxLength": 300},
                            "value": {"type": "string", "description": "Valeur exacte renvoyée au modèle lors du clic"},
                            "colors": {"type": "array", "maxItems": 6, "items": {"type": "string", "pattern": "^#[0-9A-Fa-f]{6}$"}},
                            "image_path": {"type": "string", "description": "Chemin absolu d'une image locale existante, pas une URL"},
                            "svg": {"type": "string", "description": "Schéma SVG autonome, formes et texte, sans ressources externes", "maxLength": 65536},
                            "source_url": {"type": "string", "description": "URL HTTPS de la source réelle, par exemple une palette Color Hunt"}
                        }
                    }}
                }
            }),
        ),
        FunctionDefinition::new(
            PALETTE_TOOL,
            "Renvoie une sélection locale de palettes Color Hunt avec leurs couleurs exactes et liens sources. Point de départ pour les goûts et couleurs ; ce n'est pas un catalogue en temps réel. Présentez les palettes retenues avec lyra_offer_choices, et laissez l'utilisateur cliquer.",
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
        ),
    ]
}

pub fn palette_catalog() -> Value {
    json!({
        "source": "Color Hunt", "live": false,
        "palettes": [
            {"id":"graphite-cyan", "label":"Graphite & cyan", "colors":["#222831","#393E46","#00ADB5","#EEEEEE"], "source_url":"https://colorhunt.co/palette/222831393e4600adb5eeeeee"},
            {"id":"ocean", "label":"Océan", "colors":["#1B262C","#0F4C75","#3282B8","#BBE1FA"], "source_url":"https://colorhunt.co/palette/1b262c0f4c753282b8bbe1fa"},
            {"id":"graphite-amber", "label":"Graphite & ambre", "colors":["#222831","#393E46","#FFD369","#EEEEEE"], "source_url":"https://colorhunt.co/palette/222831393e46ffd369eeeeee"}
        ]
    })
}

pub fn parse_color(hex: &str) -> anyhow::Result<slint::Color> {
    ensure!(
        hex.len() == 7 && hex.starts_with('#') && hex[1..].bytes().all(|c| c.is_ascii_hexdigit()),
        "couleur attendue au format #RRGGBB"
    );
    let rgb = u32::from_str_radix(&hex[1..], 16)?;
    Ok(slint::Color::from_rgb_u8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
    ))
}

impl ChoiceRequest {
    pub fn parse(raw: &str) -> anyhow::Result<Self> {
        ensure!(raw.len() <= 600_000, "proposition trop volumineuse");
        let request: Self = serde_json::from_str(raw).context("proposition mal formée")?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.question.trim().is_empty() && self.question.chars().count() <= 200,
            "question attendue (200 caractères maximum)"
        );
        ensure!(
            (2..=8).contains(&self.options.len()),
            "proposez entre 2 et 8 choix"
        );
        let mut ids = HashSet::new();
        for option in &self.options {
            ensure!(
                !option.id.is_empty()
                    && option.id.len() <= 64
                    && option
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
                "identifiant de choix invalide"
            );
            ensure!(ids.insert(&option.id), "identifiants de choix dupliqués");
            ensure!(
                !option.label.trim().is_empty() && option.label.chars().count() <= 80,
                "libellé attendu (80 caractères maximum)"
            );
            ensure!(
                option.description.chars().count() <= 300 && option.value.len() <= 8192,
                "description ou valeur trop longue"
            );
            ensure!(option.colors.len() <= 6, "6 couleurs maximum par palette");
            for color in &option.colors {
                parse_color(color)?;
            }
            ensure!(
                option.source_url.is_empty()
                    || (option.source_url.starts_with("https://")
                        && option.source_url.len() <= 2048
                        && !option.source_url.chars().any(char::is_control)),
                "source HTTPS attendue"
            );
            ensure!(
                option.svg.is_none() || option.image_path.is_none(),
                "choisissez svg ou image_path"
            );
            if let Some(svg) = &option.svg {
                let lower = svg.to_ascii_lowercase();
                ensure!(
                    svg.len() <= 65536 && lower.contains("<svg"),
                    "schéma SVG attendu, 64 Ko maximum"
                );
                for forbidden in [
                    "href",
                    "<image",
                    "<script",
                    "<foreignobject",
                    "<!entity",
                    "<!doctype",
                ] {
                    ensure!(
                        !lower.contains(forbidden),
                        "le schéma doit être autonome, avec formes et texte uniquement"
                    );
                }
            }
            if let Some(path) = &option.image_path {
                let path = std::path::Path::new(path);
                ensure!(path.is_absolute(), "chemin d'image absolu attendu");
                let meta = std::fs::metadata(path).context("image locale inaccessible")?;
                ensure!(
                    meta.is_file() && meta.len() <= 5 * 1024 * 1024,
                    "image locale attendue, 5 Mo maximum"
                );
                ensure!(
                    path.extension()
                        .is_some_and(|ext| ["png", "jpg", "jpeg", "webp", "gif"]
                            .iter()
                            .any(|allowed| ext.eq_ignore_ascii_case(allowed))),
                    "image PNG/JPEG/WebP/GIF attendue ; pour un schéma, utilisez svg"
                );
            }
        }
        Ok(())
    }

    /// À appeler sur le thread UI : les images et modèles Slint ne sont pas Send.
    pub fn to_row(&self) -> MessageRow {
        let options: Vec<ChoiceOptionRow> = self
            .options
            .iter()
            .map(|option| {
                let swatches: Vec<SwatchRow> = option
                    .colors
                    .iter()
                    .map(|hex| SwatchRow {
                        hex: hex.to_uppercase().into(),
                        paint: parse_color(hex).unwrap_or_default(),
                    })
                    .collect();
                let preview = if let Some(svg) = &option.svg {
                    slint::Image::load_from_svg_data(svg.as_bytes()).ok()
                } else {
                    option.image_path.as_ref().and_then(|path| {
                        slint::Image::load_from_path(std::path::Path::new(path)).ok()
                    })
                };
                ChoiceOptionRow {
                    id: option.id.clone().into(),
                    label: option.label.clone().into(),
                    description: option.description.clone().into(),
                    value: if option.value.is_empty() {
                        option.label.clone()
                    } else {
                        option.value.clone()
                    }
                    .into(),
                    colors_text: option.colors.join(" · ").into(),
                    swatches: ModelRc::new(VecModel::from(swatches)),
                    preview_missing: (option.svg.is_some() || option.image_path.is_some())
                        && preview.is_none(),
                    preview: preview.unwrap_or_default(),
                    source: option.source_url.clone().into(),
                    source_label: option
                        .source_url
                        .strip_prefix("https://")
                        .unwrap_or("")
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .into(),
                    selected: false,
                }
            })
            .collect();
        MessageRow {
            kind: "choice".into(),
            text: self.question.clone().into(),
            choices: ModelRc::new(VecModel::from(options)),
            choice_state: "pending".into(),
            ..Default::default()
        }
    }
}

/// Une réponse écrite ou un nouveau choix ferme les propositions précédentes.
pub fn finish_pending_choices(ui: &ChatWindow) {
    let model = ui.get_messages();
    for index in 0..model.row_count() {
        if let Some(mut row) = model.row_data(index) {
            if row.kind == "choice" && row.choice_state == "pending" {
                row.choice_state = "skipped".into();
                model.set_row_data(index, row);
            }
        }
    }
}

pub fn select_choice(ui: &ChatWindow, index: i32, id: &str) -> Option<(String, String)> {
    let index = usize::try_from(index).ok()?;
    let model = ui.get_messages();
    let mut row = model.row_data(index)?;
    if row.kind != "choice" || row.choice_state != "pending" {
        return None;
    }
    let option_index = (0..row.choices.row_count()).find(|i| {
        row.choices
            .row_data(*i)
            .is_some_and(|option| option.id == id)
    })?;
    let mut option = row.choices.row_data(option_index)?;
    let display = format!("{} « {} ».", ui.get_tr().choice_selected, option.label);
    let colors: Vec<String> = option
        .swatches
        .iter()
        .map(|color| color.hex.to_string())
        .collect();
    let context = serde_json::json!({
        "type": "user_choice", "question": row.text.as_str(), "id": option.id.as_str(),
        "label": option.label.as_str(), "value": option.value.as_str(),
        "colors": colors, "source_url": option.source.as_str(),
    })
    .to_string();
    finish_pending_choices(ui);
    option.selected = true;
    row.choices.set_row_data(option_index, option);
    row.choice_state = "selected".into();
    model.set_row_data(index, row);
    Some((display, context))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valide_textes_palettes_et_schemas() {
        let raw = r##"{"question":"Quelle ambiance ?","options":[{"id":"cyan","label":"Cyan","colors":["#00ADB5"]},{"id":"schema","label":"Schéma","svg":"<svg xmlns='http://www.w3.org/2000/svg'><rect width='20' height='20'/></svg>"}]}"##;
        assert!(ChoiceRequest::parse(raw).is_ok());
        assert!(ChoiceRequest::parse(&raw.replace("#00ADB5", "rouge")).is_err());
        assert!(ChoiceRequest::parse(&raw.replace("\"schema\"", "\"cyan\"")).is_err());
        assert!(
            ChoiceRequest::parse(&raw.replace("<rect", "<image href='file:///etc/passwd' /><rect"))
                .is_err()
        );
    }

    #[test]
    fn catalogue_source_et_couleurs_valides() {
        for palette in palette_catalog()["palettes"].as_array().unwrap() {
            assert!(
                palette["source_url"]
                    .as_str()
                    .unwrap()
                    .starts_with("https://colorhunt.co/palette/")
            );
            for hex in palette["colors"].as_array().unwrap() {
                parse_color(hex.as_str().unwrap()).unwrap();
            }
        }
        assert!(parse_color("#ééé").is_err());
    }
}
