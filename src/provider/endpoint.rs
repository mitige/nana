//! custom endpoints: a provider you added in settings.json, raised to the same
//! standing as the built-in ones.
//!
//! the registry in `provider.rs` holds `&'static` rows, which a file cannot
//! change. this module owns the same information as strings, so the hub can
//! list what you added, the client can call it, and the cli can name it.

use super::{by_id, detect, Kind, Provider, PROVIDERS};
use crate::settings::{ProviderCfg, Settings};

/// one endpoint nana can talk to: a built-in provider, or one you added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// the id you write in settings.json and in `provider`
    pub id: String,
    /// the name shown in the hub
    pub label: String,
    pub kind: Kind,
    pub base_url: String,
    /// environment variable holding the key, if this endpoint wants one
    pub key_env: Option<String>,
    /// true when this endpoint came from settings.json, not the built-in table
    pub custom: bool,
    /// model names this endpoint serves, so a model picks it by itself
    pub models: Vec<String>,
}

impl Endpoint {
    /// the same row the built-in registry declares, as an owned value.
    pub fn of(p: &'static Provider) -> Endpoint {
        Endpoint {
            id: p.id.to_string(),
            label: p.label.to_string(),
            kind: p.kind,
            base_url: p.base_url.to_string(),
            key_env: p.key_env.map(str::to_string),
            custom: false,
            models: Vec::new(),
        }
    }

    /// one endpoint per provider: every built-in, then every custom one from
    /// settings, so a caller lists exactly what the client can reach. a custom
    /// entry replaces the built-in of the same id.
    pub fn all(settings: &Settings) -> Vec<Endpoint> {
        let mut out: Vec<Endpoint> = Vec::with_capacity(PROVIDERS.len() + settings.providers.len());
        for p in PROVIDERS {
            if !settings.providers.contains_key(p.id) {
                out.push(Endpoint::of(p));
            }
        }
        out.extend(
            settings
                .providers
                .iter()
                .map(|(id, cfg)| Endpoint::custom(id, cfg)),
        );
        out
    }

    /// Reads one entry of `settings.providers`. A missing kind means the
    /// openai-compatible shape, which is what "custom endpoint" usually is;
    /// an explicitly empty `api_key_env` means the endpoint wants no key.
    fn custom(id: &str, cfg: &ProviderCfg) -> Endpoint {
        Endpoint {
            id: id.to_string(),
            label: cfg.label.clone().unwrap_or_else(|| id.to_string()),
            kind: Kind::from_name(&cfg.kind),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            key_env: match cfg.api_key_env.as_deref() {
                Some("") => None,
                Some(env) => Some(env.to_string()),
                None => Some("NANA_API_KEY".to_string()),
            },
            custom: true,
            models: cfg.models.clone(),
        }
    }

    /// Finds an endpoint by its id: a custom one wins over a built-in of the
    /// same name, since settings override the registry.
    pub fn by_id(id: &str, settings: &Settings) -> Option<Endpoint> {
        settings
            .providers
            .get(id)
            .map(|cfg| Endpoint::custom(id, cfg))
            .or_else(|| by_id(id).map(Endpoint::of))
    }

    /// The endpoint that answers for this model: the id named in `provider`
    /// when there is one, else a custom endpoint that lists this model, else
    /// the guess the registry makes from the model name.
    pub fn resolve(settings: &Settings, model: &str) -> Endpoint {
        if let Some(e) = settings
            .provider
            .as_deref()
            .and_then(|id| Endpoint::by_id(id, settings))
        {
            return e;
        }
        if let Some(e) = Endpoint::all(settings)
            .into_iter()
            .find(|e| e.custom && e.models.iter().any(|m| m == model))
        {
            return e;
        }
        Endpoint::of(detect(model))
    }

    /// how this endpoint speaks, as a word for the ui.
    pub fn kind_name(&self) -> &'static str {
        self.kind.name()
    }

    /// whether the key this endpoint wants is present, as a word for the cli
    /// and the hub: never the key itself.
    pub fn key_state(&self) -> String {
        let Some(env) = self.key_env.as_deref() else {
            return "not needed".to_string();
        };
        let set = std::env::var(env).is_ok() || super::dsh_has(env);
        format!("{env}{}", if set { " (set)" } else { " (missing)" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a settings value holding the one custom endpoint `gw`, openai-shaped.
    fn with_gw() -> Settings {
        let mut s = Settings::default();
        s.providers.insert(
            "gw".into(),
            ProviderCfg {
                kind: "openai".into(),
                label: Some("my gateway".into()),
                base_url: "https://gw.example/v1/".into(),
                api_key_env: Some("GW_KEY".into()),
                models: vec!["gw-large".into()],
            },
        );
        s
    }

    #[test]
    fn a_custom_endpoint_is_listed_next_to_the_built_ins() {
        let s = with_gw();
        let all = Endpoint::all(&s);
        assert_eq!(all.len(), PROVIDERS.len() + 1);
        let gw = all.iter().find(|e| e.id == "gw").unwrap();
        assert!(gw.custom);
        assert_eq!(gw.label, "my gateway");
        // the trailing slash is dropped, so urls join cleanly
        assert_eq!(gw.base_url, "https://gw.example/v1");
        assert_eq!(gw.kind_name(), "openai");
    }

    #[test]
    fn a_custom_endpoint_replaces_the_built_in_of_the_same_id() {
        let mut s = Settings::default();
        s.providers.insert(
            "openai".into(),
            ProviderCfg {
                base_url: "https://mine.example/v1".into(),
                kind: String::new(),
                ..Default::default()
            },
        );
        let all = Endpoint::all(&s);
        assert_eq!(all.len(), PROVIDERS.len());
        let merged: Vec<&Endpoint> = all.iter().filter(|e| e.id == "openai").collect();
        assert_eq!(merged.len(), 1);
        assert!(merged[0].custom);
        assert_eq!(merged[0].base_url, "https://mine.example/v1");
        // a missing kind means the openai-compatible shape
        assert_eq!(merged[0].kind, Kind::OpenAi);
    }

    #[test]
    fn the_model_a_custom_endpoint_serves_picks_it_without_being_named() {
        let s = with_gw();
        // no `provider` in settings: the model alone finds the endpoint
        assert_eq!(Endpoint::resolve(&s, "gw-large").id, "gw");
        // a model nobody claims still lands on the built-in routing
        assert_eq!(Endpoint::resolve(&s, "claude-sonnet-4").id, "anthropic");
    }

    #[test]
    fn an_endpoint_with_no_key_env_asks_for_no_key() {
        let mut s = Settings::default();
        s.providers.insert(
            "local".into(),
            ProviderCfg {
                base_url: "http://127.0.0.1:8000/v1".into(),
                api_key_env: Some(String::new()),
                ..Default::default()
            },
        );
        let e = Endpoint::by_id("local", &s).unwrap();
        assert_eq!(e.key_env, None);
        assert_eq!(e.key_state(), "not needed");
    }

    #[test]
    fn a_named_provider_wins_over_the_model_guess() {
        let mut s = with_gw();
        s.provider = Some("gw".into());
        // the model says anthropic, but the setting overrides it
        assert_eq!(Endpoint::resolve(&s, "claude-sonnet-4").id, "gw");
    }
}
