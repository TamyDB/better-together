//! Códigos de sala no formato "PATO-4821".

use rand::Rng;

pub const WORDS: [&str; 64] = [
    "PATO", "LAGO", "MATO", "LUAR", "NEVE", "BOLO", "DOCE", "CAFE", "MELA", "UVAS", "PERA", "LIMA",
    "COCO", "FIGO", "GATO", "RATO", "SAPO", "URSO", "LOBO", "ALCE", "FOCA", "PUMA", "CERA", "ONDA",
    "ILHA", "VALE", "VELA", "REMO", "MAPA", "FARO", "DUNA", "MARE", "TRIO", "PATA", "GALO", "BODE",
    "ZEBU", "KIWI", "LEAO", "TATU", "JACA", "ROSA", "FLOR", "RAIO", "TOCA", "OVOS", "PAES", "VACA",
    "CAVA", "POMO", "LUVA", "SINO", "TEIA", "PIPA", "CUCA", "NATA", "LOTE", "PINO", "LIRA", "TUBO",
    "DEDO", "RUBI", "JADE", "ABRE",
];

/// Gera um código "{PALAVRA}-{0000..=9999}".
pub fn generate(rng: &mut impl Rng) -> String {
    let word = WORDS[rng.random_range(0..WORDS.len())];
    format!("{word}-{:04}", rng.random_range(0..=9999u32))
}

/// Aceita "pato 4821", "PATO4821", " Pato-4821 " etc. e devolve "PATO-4821".
pub fn normalize(input: &str) -> Option<String> {
    let s: String = input
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase();
    let (word, digits) = match s.split_once('-') {
        Some((w, d)) => (w, d),
        None if s.len() >= 4 && s.is_char_boundary(4) => s.split_at(4),
        None => return None,
    };
    let ok = word.len() == 4
        && word.bytes().all(|b| b.is_ascii_uppercase())
        && digits.len() == 4
        && digits.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| format!("{word}-{digits}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn words_valid() {
        assert_eq!(WORDS.len(), 64);
        let set: std::collections::HashSet<_> = WORDS.iter().collect();
        assert_eq!(set.len(), 64);
        assert!(WORDS
            .iter()
            .all(|w| w.len() == 4 && w.bytes().all(|b| b.is_ascii_uppercase())));
        assert!(WORDS.contains(&"PATO"));
    }

    #[test]
    fn generate_format() {
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..1000 {
            let c = generate(&mut rng);
            let b = c.as_bytes();
            assert_eq!(b.len(), 9, "{c}");
            assert!(b[..4].iter().all(u8::is_ascii_uppercase), "{c}");
            assert_eq!(b[4], b'-');
            assert!(b[5..].iter().all(u8::is_ascii_digit), "{c}");
        }
    }

    #[test]
    fn normalize_variants() {
        for ok in ["pato 4821", " Pato-4821 ", "PATO4821"] {
            assert_eq!(normalize(ok).as_deref(), Some("PATO-4821"), "{ok}");
        }
        for bad in ["PAT-4821", "PATO-48210", "PATO-48A1", ""] {
            assert_eq!(normalize(bad), None, "{bad}");
        }
    }
}
