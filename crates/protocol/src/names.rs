//! Sanitização de nomes de jogador vindos da rede.

/// Máximo de caracteres (chars, não bytes) em um nome de jogador.
pub const MAX_NAME_CHARS: usize = 16;

/// Máximo de caracteres (chars, não bytes) em uma mensagem de chat.
pub const MAX_CHAT_CHARS: usize = 120;

/// Caracteres invisíveis ou de controle bidirecional que não podem aparecer em nomes.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200C}'
            | '\u{200E}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{2800}'
            | '\u{061C}'
            | '\u{00AD}'
            | '\u{034F}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{180E}'
            | '\u{3164}'
            | '\u{FFA0}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// Junta de emoji (ZWJ) ou seletor de variação: fazem parte de emojis compostos no chat, mas
/// não são visíveis sozinhos.
fn is_emoji_glue(c: char) -> bool {
    matches!(c, '\u{200D}' | '\u{FE00}'..='\u{FE0F}')
}

/// Marca combinante (acentos empilhados, usados em "zalgo").
fn is_combining_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

/// Máximo de marcas combinantes seguidas aceitas no chat.
const MAX_COMBINING_RUN: usize = 2;

/// Remove controles, invisíveis e marcas bidi, junta espaços repetidos e apara. Com `chat`,
/// preserva ZWJ e seletores de variação (emojis compostos) e limita marcas combinantes seguidas.
fn clean_text(raw: &str, chat: bool) -> String {
    let mut run = 0;
    let cleaned: String = raw
        .chars()
        .filter(|&c| {
            if c.is_control() || is_invisible(c) {
                return false;
            }
            if is_emoji_glue(c) {
                return chat;
            }
            if chat && is_combining_mark(c) {
                run += 1;
                return run <= MAX_COMBINING_RUN;
            }
            run = 0;
            true
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Limpa um nome não confiável: remove controles, invisíveis e marcas bidi, junta espaços
/// repetidos, apara e corta em `MAX_NAME_CHARS`. Se sobrar vazio, devolve `fallback`.
pub fn sanitize_name(raw: &str, fallback: &str) -> String {
    let joined = clean_text(raw, false);
    let cut: String = joined.chars().take(MAX_NAME_CHARS).collect();
    let cut = cut.trim_end();
    if cut.is_empty() {
        fallback.to_string()
    } else {
        cut.to_string()
    }
}

/// Limpa uma mensagem de chat não confiável com as mesmas regras de `sanitize_name`, cortando
/// em `MAX_CHAT_CHARS`. Se sobrar vazio, devolve `None`.
pub fn sanitize_chat(raw: &str) -> Option<String> {
    let joined = clean_text(raw, true);
    let cut: String = joined.chars().take(MAX_CHAT_CHARS).collect();
    let cut = cut.trim_end();
    let visible = cut
        .chars()
        .any(|c| !c.is_whitespace() && !is_invisible(c) && !is_emoji_glue(c));
    if !visible {
        None
    } else {
        Some(cut.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_name_rules() {
        assert_eq!(sanitize_name("  Tamy  ", "x"), "Tamy");
        assert_eq!(sanitize_name("a\u{0}b\nc", "x"), "abc");
        assert_eq!(sanitize_name("Ana   Maria", "x"), "Ana Maria");
        assert_eq!(sanitize_name(&"x".repeat(10_000), "x").chars().count(), 16);
        let long = sanitize_name("çãoçãoçãoçãoçãoção", "x");
        assert_eq!(long.chars().count(), 16);
        assert_eq!(sanitize_name("\n\t ", "Convidado"), "Convidado");
    }

    #[test]
    fn sanitize_name_strips_invisible_and_bidi() {
        assert_eq!(sanitize_name("\u{200B}", "Convidado"), "Convidado");
        let invisible = "\u{200B}\u{200F}\u{202A}\u{2060}\u{2064}\u{2066}\u{2069}\u{FEFF}\u{2800}";
        assert_eq!(sanitize_name(invisible, "Convidado"), "Convidado");
        assert_eq!(sanitize_name("Ana\u{202E}Maria", "x"), "AnaMaria");
        assert_eq!(sanitize_name(" \u{200B} Tamy \u{FEFF}", "x"), "Tamy");
    }

    #[test]
    fn sanitize_chat_rules() {
        assert_eq!(sanitize_chat("  oi  "), Some("oi".to_string()));
        assert_eq!(sanitize_chat("a\u{0}b\nc"), Some("abc".to_string()));
        let long = sanitize_chat(&"x".repeat(10_000)).unwrap();
        assert_eq!(long.chars().count(), MAX_CHAT_CHARS);
        let multi = sanitize_chat(&"ç".repeat(500)).unwrap();
        assert_eq!(multi.chars().count(), 120);
        assert!(multi.chars().all(|c| c == 'ç'));
        assert_eq!(sanitize_chat("\u{200B}\u{202E}"), None);
        assert_eq!(sanitize_chat("   "), None);
        assert_eq!(sanitize_chat("oi   gente"), Some("oi gente".to_string()));
    }

    #[test]
    fn chat_keeps_composed_emoji() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(sanitize_chat(family), Some(family.to_string()));
        let shrug = "\u{1F937}\u{200D}\u{2640}\u{FE0F}";
        assert_eq!(sanitize_chat(shrug), Some(shrug.to_string()));
    }

    #[test]
    fn chat_rejects_only_invisible() {
        for s in [
            "\u{200D}",
            "\u{3164}",
            "\u{FE0F}",
            "\u{00AD}",
            "\u{E0041}",
            "\u{061C}",
        ] {
            assert_eq!(sanitize_chat(s), None, "{s:?}");
        }
    }

    #[test]
    fn name_strips_new_invisibles_and_zwj() {
        assert_eq!(sanitize_name("\u{3164}", "Convidado"), "Convidado");
        assert_eq!(sanitize_name("A\u{061C}na", "x"), "Ana");
        assert_eq!(sanitize_name("A\u{200D}na", "x"), "Ana");
    }

    #[test]
    fn chat_limits_combining_marks() {
        let zalgo = format!("a{}", "\u{0301}".repeat(100));
        assert_eq!(sanitize_chat(&zalgo), Some("a\u{0301}\u{0301}".to_string()));
        assert_eq!(sanitize_chat("ação"), Some("ação".to_string()));
    }

    #[test]
    fn chat_cut_falling_on_space() {
        let s = format!("{} y", "x".repeat(119));
        assert_eq!(sanitize_chat(&s), Some("x".repeat(119)));
    }
}
