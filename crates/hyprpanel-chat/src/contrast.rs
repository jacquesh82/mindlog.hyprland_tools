//! Contraste WCAG calculé, sans réseau, shell ou accès aux fichiers utilisateur.
use crate::i18n::Lang;
use anyhow::{Context, ensure};
use langchain_rust::schemas::FunctionDefinition;
use serde_json::{Value, json};

pub const TOOL: &str = "lyra_check_contrast";

pub fn instructions(lang: Lang) -> String {
    let policy = match lang {
        Lang::Fr => include_str!("../prompts/color-accessibility.fr.md"),
        Lang::En => include_str!("../../../docs/COLOR_ACCESSIBILITY.md"),
    };
    format!(
        "\n\n{policy}\n\nCOLOR TOOL PROTOCOL: Before proposing or applying interface colors, call lyra_check_contrast for every actual BG/FG role and component state. A decorative palette swatch is not a validated text/background assignment. Use the computed repairs for failing pairs and check them again before use. Pass known opaque canvas colors when transparency is involved. For gradients/images, check the worst actual background samples or place content on a verified opaque surface; never claim conformance from unknown backgrounds. Preserve user choice and command/filesystem confirmations. Contrast validation is not a full WCAG certification."
    )
}

pub fn definition() -> FunctionDefinition {
    FunctionDefinition::new(
        TOOL,
        "Calcule le contraste WCAG 2.2 de 1 à 32 associations BG/FG réelles (rôles et états). Renvoie ratio exact, conformité AA/AAA et corrections proposées, en ajustant d'abord FG puis BG si nécessaire. Appliquer et revérifier les corrections avant usage. Les fonds d’images/dégradés nécessitent des échantillons réels ou une surface opaque vérifiée.",
        json!({"type":"object","required":["pairs"],"properties":{"pairs":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"object","required":["id","background","foreground"],"properties":{
            "id":{"type":"string","description":"Rôle/état, par exemple button.hover ou normal-text.dark"},
            "background":{"type":"string","description":"#RRGGBB ou #RRGGBBAA (alpha à la fin)"},
            "foreground":{"type":"string","description":"#RRGGBB ou #RRGGBBAA"},
            "canvas":{"type":"string","description":"Fond opaque connu #RRGGBB, obligatoire si background est transparent"},
            "kind":{"type":"string","enum":["text","graphic"]},
            "font_size_px":{"type":"number","description":"Taille réelle du texte, 14 px par défaut. Ne pas supposer du grand texte."},
            "bold":{"type":"boolean"}
        }}}}}),
    )
}

#[derive(Clone, Copy, Debug)]
pub struct Color {
    rgb: [f64; 3],
    alpha: f64,
}
impl Color {
    fn parse(value: &str) -> anyhow::Result<Self> {
        ensure!(
            matches!(value.len(), 7 | 9)
                && value.starts_with('#')
                && value[1..].bytes().all(|b| b.is_ascii_hexdigit()),
            "couleur attendue : #RRGGBB ou #RRGGBBAA"
        );
        let channels = (0..(value.len() - 1) / 2)
            .map(|i| {
                u8::from_str_radix(&value[1 + i * 2..3 + i * 2], 16).map(|v| f64::from(v) / 255.)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            rgb: [channels[0], channels[1], channels[2]],
            alpha: channels.get(3).copied().unwrap_or(1.),
        })
    }
    fn over(self, background: Self) -> Self {
        Self {
            rgb: std::array::from_fn(|i| {
                self.rgb[i] * self.alpha + background.rgb[i] * (1. - self.alpha)
            }),
            alpha: 1.,
        }
    }
    fn mix(self, target: f64, amount: f64) -> Self {
        Self {
            rgb: self
                .rgb
                .map(|v| (v * (1. - amount) + target * amount).clamp(0., 1.)),
            alpha: 1.,
        }
    }
    fn hex(self) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            (self.rgb[0] * 255.).round() as u8,
            (self.rgb[1] * 255.).round() as u8,
            (self.rgb[2] * 255.).round() as u8
        )
    }
    fn quantized(self) -> Self {
        Self::parse(&self.hex()).expect("couleur calculée")
    }
    fn distance(self, other: Self) -> f64 {
        self.rgb
            .iter()
            .zip(other.rgb)
            .map(|(a, b)| (a - b).powi(2))
            .sum()
    }
}

fn luminance(color: Color) -> f64 {
    let linear = color.rgb.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
}

/// Équivalent Rust de getContrastRatio ; couleurs opaques effectives après composition.
pub fn get_contrast_ratio(background: Color, foreground: Color) -> f64 {
    let a = luminance(background);
    let b = luminance(foreground.over(background));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn repair_fg(bg: Color, fg: Color, threshold: f64) -> Option<Color> {
    [0., 1.]
        .into_iter()
        .filter_map(|target| {
            if get_contrast_ratio(bg, fg.mix(target, 1.)) < threshold {
                return None;
            }
            let (mut lo, mut hi) = (0., 1.);
            for _ in 0..32 {
                let mid = (lo + hi) / 2.;
                if get_contrast_ratio(bg, fg.mix(target, mid).quantized()) >= threshold {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            Some(fg.mix(target, hi).quantized())
        })
        .min_by(|a, b| fg.distance(*a).total_cmp(&fg.distance(*b)))
}

fn repair(bg: Color, fg: Color, threshold: f64) -> Value {
    // Les remplacements sont opaques et sur 8 bits : mesurer ces valeurs exactes.
    let bg = bg.quantized();
    if let Some(corrected) = repair_fg(bg, fg, threshold) {
        return json!({"background":bg.hex(),"foreground":corrected.hex(),"ratio":get_contrast_ratio(bg,corrected),"changed":"foreground"});
    }
    // Le contraste AAA 7:1 n'est pas toujours atteignable sur un gris moyen.
    let (mut lo, mut hi) = (0., 1.);
    let dark = luminance(fg) > 0.179;
    let target = if dark { 0. } else { 1. };
    for _ in 0..32 {
        let mid = (lo + hi) / 2.;
        if repair_fg(bg.mix(target, mid).quantized(), fg, threshold).is_some() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let corrected_bg = bg.mix(target, hi).quantized();
    let corrected_fg = repair_fg(corrected_bg, fg, threshold).expect("noir/blanc atteint le seuil");
    json!({"background":corrected_bg.hex(),"foreground":corrected_fg.hex(),"ratio":get_contrast_ratio(corrected_bg,corrected_fg),"changed":"background_and_foreground"})
}

pub fn call(args: &Value) -> anyhow::Result<Value> {
    let pairs = args["pairs"].as_array().context("pairs attendu")?;
    ensure!(
        (1..=32).contains(&pairs.len()),
        "1 à 32 associations attendues"
    );
    let mut output = Vec::new();
    for pair in pairs {
        let id = pair["id"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 200)
            .context("id attendu")?;
        let mut bg = Color::parse(pair["background"].as_str().context("background attendu")?)?;
        let fg = Color::parse(pair["foreground"].as_str().context("foreground attendu")?)?;
        if bg.alpha < 1. {
            let canvas = Color::parse(
                pair["canvas"]
                    .as_str()
                    .context("fond canvas opaque requis pour la transparence")?,
            )?;
            ensure!(canvas.alpha == 1., "canvas doit être opaque");
            bg = bg.over(canvas);
        }
        let kind = pair["kind"].as_str().unwrap_or("text");
        ensure!(
            matches!(kind, "text" | "graphic"),
            "kind attendu : text ou graphic"
        );
        let size = pair["font_size_px"].as_f64().unwrap_or(14.);
        ensure!(
            size.is_finite() && size > 0. && size <= 1000.,
            "taille de texte invalide"
        );
        let large = size >= 24. || (size >= 18.7 && pair["bold"].as_bool().unwrap_or(false));
        let aa = if kind == "graphic" || large { 3. } else { 4.5 };
        let aaa = if kind == "graphic" {
            3.
        } else if large {
            4.5
        } else {
            7.
        };
        let ratio = get_contrast_ratio(bg, fg);
        output.push(json!({"id":id,"ratio":ratio,"aa_minimum":aa,"passes_aa":ratio>=aa,"aaa_text_minimum":if kind=="text" { Some(aaa) } else { None },"passes_aaa_text":if kind=="text" { Some(ratio>=aaa) } else { None },
            "effective_background":bg.hex(),"effective_foreground":fg.over(bg).hex(),
            "repair_aa":if ratio<aa {repair(bg,fg.over(bg),aa)} else {Value::Null},
            "repair_aaa_text":if kind=="text" && ratio<aaa {repair(bg,fg.over(bg),aaa)} else {Value::Null}}));
    }
    Ok(
        json!({"standard":"WCAG 2.2, sRGB relative luminance, no threshold rounding","results":output,"note":"Measured flat samples only; use verified repairs and recheck actual backgrounds, all states and roles. This is not a complete accessibility audit."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn color(s: &str) -> Color {
        Color::parse(s).unwrap()
    }
    #[test]
    fn native_semantic_tokens_cover_text_button_states_and_focus() {
        let source = include_str!("../ui/components.slint");
        let tokens: std::collections::HashMap<_, _> = source
            .split("export global AvatarFrames")
            .next()
            .unwrap()
            .lines()
            .filter_map(|line| {
                let (name, value) = line
                    .trim()
                    .strip_prefix("out property <color> ")?
                    .split_once(": ")?;
                Some((name, color(value.trim_end_matches(';'))))
            })
            .collect();
        for background in ["canvas", "panel", "raised", "hover", "pressed"] {
            for foreground in ["text", "secondary", "accent"] {
                let ratio = get_contrast_ratio(tokens[background], tokens[foreground]);
                assert!(ratio >= 4.5, "{foreground} / {background}: {ratio}");
            }
        }
        for background in ["canvas", "panel", "raised"] {
            assert!(get_contrast_ratio(tokens[background], tokens["muted"]) >= 4.5);
        }
        for background in ["accent", "primary-hover", "primary-pressed"] {
            assert!(
                get_contrast_ratio(tokens[background], tokens["on-primary"]) >= 7.,
                "bouton primaire : {background}"
            );
        }
        for background in ["canvas", "panel", "raised", "hover", "pressed"] {
            assert!(
                get_contrast_ratio(tokens[background], tokens["accent"]) >= 3.,
                "focus : {background}"
            );
        }
    }
    #[test]
    fn official_luminance_and_exact_boundaries() {
        assert!((get_contrast_ratio(color("#000000"), color("#FFFFFF")) - 21.).abs() < 1e-12);
        assert_eq!(get_contrast_ratio(color("#123456"), color("#123456")), 1.);
        assert!((luminance(color("#FF0000")) - 0.2126).abs() < 1e-12);
        assert!(get_contrast_ratio(color("#FFFFFF"), color("#777777")) < 4.5);
        assert!(get_contrast_ratio(color("#FFFFFF"), color("#767676")) >= 4.5);
        assert!(Color::parse("#１２３").is_err());
    }
    #[test]
    fn repairs_keep_background_first_and_meet_both_targets() {
        let out =
            call(&json!({"pairs":[{"id":"text","background":"#888888","foreground":"#999999"}]}))
                .unwrap();
        let row = &out["results"][0];
        assert_eq!(row["passes_aa"], false);
        assert_eq!(row["repair_aa"]["background"], "#888888");
        assert!(row["repair_aa"]["ratio"].as_f64().unwrap() >= 4.5);
        assert!(row["repair_aaa_text"]["ratio"].as_f64().unwrap() >= 7.);
    }
    #[test]
    fn transparency_and_real_text_sizes_are_checked() {
        let pair = json!({"id":"alpha","background":"#00000080","foreground":"#FFFFFF"});
        assert!(call(&json!({"pairs":[pair.clone()]})).is_err());
        let mut pair = pair;
        pair["canvas"] = json!("#000000");
        let alpha = call(&json!({"pairs":[pair]})).unwrap();
        assert_eq!(alpha["results"][0]["ratio"], 21.);
        let out=call(&json!({"pairs":[
            {"id":"normal","background":"#FFFFFF","foreground":"#777777","font_size_px":23.9},
            {"id":"large","background":"#FFFFFF","foreground":"#777777","font_size_px":24},
            {"id":"bold","background":"#FFFFFF","foreground":"#777777","font_size_px":18.7,"bold":true},
            {"id":"icon","background":"#FFFFFF","foreground":"#777777","kind":"graphic"}]})).unwrap();
        assert_eq!(out["results"][0]["passes_aa"], false);
        for i in 1..4 {
            assert_eq!(out["results"][i]["passes_aa"], true);
        }
    }
}
