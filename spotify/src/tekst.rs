//! Porównywanie tekstów przy szukaniu: bez wielkości liter, znaków diakrytycznych i interpunkcji.

/// „Zażółć – GĘŚLĄ jaźń!” → „zazolc gesla jazn”
pub fn uprosc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut odstep = false;
    for c in s.chars().flat_map(char::to_lowercase) {
        let z: &str = match c {
            'ą' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' => "a",
            'ć' | 'ç' | 'č' | 'ĉ' | 'ċ' => "c",
            'ď' | 'đ' | 'ð' => "d",
            'ę' | 'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ě' | 'ė' => "e",
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' | 'į' => "i",
            'ł' | 'ľ' | 'ĺ' | 'ļ' => "l",
            'ń' | 'ñ' | 'ň' | 'ņ' => "n",
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
            'ř' | 'ŕ' => "r",
            'ś' | 'š' | 'ş' | 'ș' | 'ŝ' => "s",
            'ť' | 'ţ' | 'ț' => "t",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
            'ý' | 'ÿ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            'ß' => "ss",
            'æ' => "ae",
            'œ' => "oe",
            // znaki łączące (np. „é” zapisane jako e + akcent) i apostrofy: pomijamy bez odstępu
            '\u{0300}'..='\u{036f}' | '\'' | '’' | '`' => continue,
            c if c.is_alphanumeric() => {
                if odstep && !o.is_empty() {
                    o.push(' ');
                }
                odstep = false;
                o.push(c);
                continue;
            }
            _ => {
                odstep = true;
                continue;
            }
        };
        if odstep && !o.is_empty() {
            o.push(' ');
        }
        odstep = false;
        o.push_str(z);
    }
    o
}

/// Ocena dopasowania (0 = nie pasuje). `q` i `pole` po [`uprosc`]. Całe pole > początek > początek słowa > środek.
pub fn ocena(q: &str, pole: &str) -> u32 {
    if q.is_empty() || pole.is_empty() {
        return 0;
    }
    if pole == q {
        100
    } else if pole.starts_with(q) {
        80
    } else if pole.contains(&format!(" {q}")) {
        60
    } else if pole.contains(q) {
        40
    } else if q.split(' ').all(|s| pole.contains(s)) {
        30
    } else {
        0
    }
}

/// Kodowanie do adresu (UTF-8 procentami, odstęp jako „+”).
pub fn do_adresu(s: &str) -> String {
    let mut o = String::new();
    for b in s.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn upraszcza_polskie_znaki_i_interpunkcje() {
        assert_eq!(uprosc("Zażółć – GĘŚLĄ  jaźń!"), "zazolc gesla jazn");
        assert_eq!(uprosc("Don’t Stop (Remastered)"), "dont stop remastered");
        assert_eq!(uprosc("AC/DC"), "ac dc");
        assert_eq!(uprosc("Sigur Rós"), uprosc("sigur ro\u{0301}s"));
        assert_eq!(uprosc("  ...  "), "");
    }

    #[test]
    fn ocena_ma_wlasciwa_kolejnosc() {
        let q = "kult";
        let o: Vec<u32> = ["kult", "kultura", "stary kult", "okultyzm", "dzem"].iter().map(|p| ocena(q, p)).collect();
        assert_eq!(o, vec![100, 80, 60, 40, 0]);
        assert_eq!(ocena("arahja kult", "kult arahja"), 30);
        assert_eq!(ocena("", "cokolwiek"), 0);
    }

    #[test]
    fn adres() {
        assert_eq!(do_adresu("kult żółć"), "kult+%C5%BC%C3%B3%C5%82%C4%87");
        assert_eq!(do_adresu("a:b/c?d"), "a%3Ab%2Fc%3Fd");
    }
}
