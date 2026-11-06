//! Sanitização de nomes de jogador vindos da rede.

/// Máximo de caracteres (chars, não bytes) em um nome de jogador.
pub const MAX_NAME_CHARS: usize = 16;

/// Limpa um nome não confiável: remove controles, junta espaços repetidos, apara e corta
/// em `MAX_NAME_CHARS`. Se sobrar vazio, devolve `fallback`.
pub fn sanitize_name(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MAX_NAME_CHARS).collect();
    let cut = cut.trim_end();
    if cut.is_empty() {
        fallback.to_string()
    } else {
        cut.to_string()
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
}
