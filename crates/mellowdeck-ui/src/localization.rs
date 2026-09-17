use std::collections::HashMap;

const EN_US: &str = include_str!("../../../assets/locales/en-US.ftl");
const PT_PT: &str = include_str!("../../../assets/locales/pt-PT.ftl");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    EnUs,
    PtPt,
}

#[derive(Clone, Debug)]
pub struct Catalog {
    messages: HashMap<String, String>,
}

impl Catalog {
    pub fn load(locale: Locale) -> Self {
        let source = match locale {
            Locale::EnUs => EN_US,
            Locale::PtPt => PT_PT,
        };
        Self { messages: parse_ftl_subset(source) }
    }

    pub fn message<'a>(&'a self, id: &'a str) -> &'a str {
        self.messages.get(id).map_or(id, String::as_str)
    }
}

fn parse_ftl_subset(source: &str) -> HashMap<String, String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
                return None;
            }
            let (id, value) = line.split_once('=')?;
            Some((id.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_catalogs_contain_core_navigation() {
        for locale in [Locale::EnUs, Locale::PtPt] {
            let catalog = Catalog::load(locale);
            assert_ne!(catalog.message("nav-home"), "nav-home");
            assert_ne!(catalog.message("nav-search"), "nav-search");
            assert_ne!(catalog.message("settings-title"), "settings-title");
        }
    }

    #[test]
    fn missing_key_is_visible_to_developers() {
        assert_eq!(Catalog::load(Locale::EnUs).message("missing-key"), "missing-key");
    }
}
