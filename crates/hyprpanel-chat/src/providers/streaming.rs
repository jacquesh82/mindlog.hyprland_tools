//! Décode uniquement le champ `text` du JSON contraint, avant sa fermeture.
//! Les arguments d'outils et les événements de réflexion ne deviennent jamais
//! du texte visible. Les appels ne seront exécutés qu'après validation finale.

use std::time::{Duration, Instant};

fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate().skip(start + 1) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Some(index);
        }
    }
    None
}

pub(super) fn visible_text(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut pos = 0;
    let skip = |pos: &mut usize| {
        while bytes.get(*pos).is_some_and(u8::is_ascii_whitespace) {
            *pos += 1;
        }
    };
    skip(&mut pos);
    if bytes.get(pos) != Some(&b'{') {
        return None;
    }
    pos += 1;
    loop {
        skip(&mut pos);
        if bytes.get(pos) != Some(&b'"') {
            return None;
        }
        let end = string_end(bytes, pos)?;
        let key: String = serde_json::from_str(&raw[pos..=end]).ok()?;
        pos = end + 1;
        skip(&mut pos);
        if bytes.get(pos) != Some(&b':') {
            return None;
        }
        pos += 1;
        skip(&mut pos);
        if key == "text" {
            if bytes.get(pos) != Some(&b'"') {
                return None;
            }
            if let Some(end) = string_end(bytes, pos) {
                return serde_json::from_str(&raw[pos..=end]).ok();
            }
            // Un fragment peut terminer au milieu de \\uXXXX, d'une paire de
            // substituts UTF-16 ou d'un échappement. Ne restituer que le préfixe
            // décodable, sans inventer le caractère manquant.
            let mut end = raw.len();
            for _ in 0..13 {
                if let Ok(text) = serde_json::from_str::<String>(&format!("{}\"", &raw[pos..end])) {
                    return Some(text);
                }
                if end <= pos + 1 {
                    return Some(String::new());
                }
                end -= 1;
                while !raw.is_char_boundary(end) {
                    end -= 1;
                }
            }
            return None;
        }
        // Sauter un autre champ entier : un `text` imbriqué dans les arguments
        // d'outil ne doit pas être confondu avec celui de la réponse.
        let mut values =
            serde_json::Deserializer::from_str(&raw[pos..]).into_iter::<serde_json::Value>();
        values.next()?.ok()?;
        pos += values.byte_offset();
        skip(&mut pos);
        if bytes.get(pos) != Some(&b',') {
            return None;
        }
        pos += 1;
    }
}

pub(super) struct TextStream<'a, F: FnMut(&str)> {
    callback: &'a mut F,
    last_emit: Instant,
    emitted: String,
}

impl<'a, F: FnMut(&str)> TextStream<'a, F> {
    pub fn new(callback: &'a mut F) -> Self {
        Self {
            callback,
            last_emit: Instant::now(),
            emitted: String::new(),
        }
    }

    pub fn update(&mut self, raw: &str) {
        if !self.emitted.is_empty() && self.last_emit.elapsed() < Duration::from_millis(33) {
            return;
        }
        if let Some(text) = visible_text(raw) {
            if !text.is_empty() && text != self.emitted {
                (self.callback)(&text);
                self.emitted = text;
                self.last_emit = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_receives_text_before_json_or_process_finishes() {
        let mut visible = Vec::new();
        {
            let mut callback = |text: &str| visible.push(text.to_string());
            let mut stream = TextStream::new(&mut callback);
            stream.update(r#"{"text":"LYRA is writing"#);
        }
        assert_eq!(visible, ["LYRA is writing"]);
    }
    #[test]
    fn every_utf8_and_escape_prefix_stays_valid() {
        let raw = r#"{"text":"Bonjour éè \\ \"oui\"\n\uD83D\uDE80 fin","tool_calls":[]}"#;
        let complete = visible_text(raw).unwrap();
        assert_eq!(complete, "Bonjour éè \\ \"oui\"\n🚀 fin");
        for end in 0..=raw.len() {
            if raw.is_char_boundary(end) {
                if let Some(partial) = visible_text(&raw[..end]) {
                    assert!(complete.starts_with(&partial), "fragment {end}");
                }
            }
        }
    }
    #[test]
    fn tool_arguments_are_never_displayed() {
        assert_eq!(
            visible_text(
                r#"{"tool_calls":[{"arguments":"{\"text\":\"SECRET_TOOL_OUTPUT\"}"}],"text":"Hello"}"#
            ),
            Some("Hello".into())
        );
        assert_eq!(
            visible_text(r#"{"tool_calls":[{"arguments":"{\"text\":\"SECRET_TOOL_OUTPUT\"}"}]"#),
            None
        );
        assert_eq!(visible_text("thinking about tools"), None);
    }
}
