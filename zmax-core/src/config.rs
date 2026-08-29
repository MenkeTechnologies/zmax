use zmax_loader::workspace_trust::WorkspaceTrust;

use crate::syntax::{
    config::{Configuration, LanguageConfiguration},
    Loader, LoaderError,
};

/// Language configuration based on built-in languages.toml.
pub fn default_lang_config() -> Configuration {
    zmax_loader::config::default_lang_config()
        .try_into()
        .expect("Could not deserialize built-in languages.toml")
}

/// Language configuration loader based on built-in languages.toml.
pub fn default_lang_loader() -> Loader {
    Loader::new(default_lang_config()).expect("Could not compile loader for default config")
}

#[derive(Debug)]
pub enum LanguageLoaderError {
    DeserializeError(toml::de::Error),
    ConfigError(toml::de::Error, String),
    LoaderError(LoaderError),
}

impl std::fmt::Display for LanguageLoaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeserializeError(err) => write!(f, "Failed to parse language config: {err}"),
            Self::ConfigError(err, context) => {
                write!(f, "Failed to parse language config {context}: {err}")
            }
            Self::LoaderError(err) => write!(f, "Failed to compile language config: {err}"),
        }
    }
}

impl std::error::Error for LanguageLoaderError {}

/// Language configuration based on user configured languages.toml.
pub fn user_lang_config(trust: &WorkspaceTrust) -> Result<Configuration, toml::de::Error> {
    zmax_loader::config::user_lang_config(trust)?.try_into()
}

/// Language configuration loader based on user configured languages.toml.
pub fn user_lang_loader(trust: &WorkspaceTrust) -> Result<Loader, LanguageLoaderError> {
    let config_val = zmax_loader::config::user_lang_config(trust)
        .map_err(LanguageLoaderError::DeserializeError)?;
    let config = config_val.clone().try_into().map_err(|e| {
        if let Some(languages) = config_val.get("language").and_then(|v| v.as_array()) {
            for lang in languages.iter() {
                let res: Result<LanguageConfiguration, _> = lang.clone().try_into();
                if let Err(inner_err) = res {
                    let context = match lang.get("name") {
                        Some(name) => format!("for language {}", name),
                        None => "for unknown language".to_owned(),
                    };
                    return LanguageLoaderError::ConfigError(inner_err, context);
                }
            }
        }
        LanguageLoaderError::ConfigError(e, String::new())
    })?;
    Loader::new(config).map_err(LanguageLoaderError::LoaderError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Both of the `default_*` functions `expect`, so a malformed shipped
    /// `languages.toml` is a panic on every user's next launch rather than an
    /// error anyone can act on. Building both here turns that into a test
    /// failure: deserializing catches a bad field, and the loader additionally
    /// compiles every file-type glob and shebang table.
    #[test]
    fn the_shipped_language_config_loads_and_compiles() {
        let config = default_lang_config();
        assert!(
            config.language.len() > 100,
            "expected the full language set, got {}",
            config.language.len()
        );

        let loader = default_lang_loader();
        assert_eq!(loader.language_configs().len(), config.language.len());
    }

    /// A language is reachable by the three keys the editor looks it up with:
    /// its name (`:set-language rust`), its scope (injections and themes), and
    /// its file extension (opening a file).
    #[test]
    fn languages_are_reachable_by_name_scope_and_filename() {
        let loader = default_lang_loader();

        let rust = loader
            .language_for_name("rust")
            .expect("rust is a shipped language");
        assert_eq!(
            loader.language_for_scope("source.rust"),
            Some(rust),
            "scope and name must resolve to the same language"
        );
        assert_eq!(
            loader.language_for_filename(Path::new("main.rs")),
            Some(rust),
            "a .rs file is rust"
        );

        assert!(loader.language_for_name("not-a-language").is_none());
        assert!(loader.language_for_scope("source.nothing").is_none());
        assert!(
            loader
                .language_for_filename(Path::new("file.unknown-extension"))
                .is_none()
        );
    }

    /// Errors name the language whose entry failed. Without the context a user
    /// with 300 languages configured is told only that "a" language is wrong.
    #[test]
    fn config_errors_name_the_language_they_came_from() {
        let err = toml::from_str::<LanguageConfiguration>("name = 1")
            .expect_err("a numeric name is not a language config");

        let with_context =
            LanguageLoaderError::ConfigError(err.clone(), "for language \"rust\"".to_string());
        let rendered = with_context.to_string();
        assert!(
            rendered.contains("for language \"rust\""),
            "the language must be named: {rendered}"
        );

        let without = LanguageLoaderError::DeserializeError(err);
        assert!(without.to_string().starts_with("Failed to parse"));
    }
}
