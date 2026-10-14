use super::*;
use crate::settings::ProviderCfg;

#[test]
fn picking_a_persona_writes_it_to_the_project_settings() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    for _ in 0..3 {
        hub.next_section();
    }
    assert_eq!(hub.current(), Section::Personas);
    assert_eq!(hub.selected().map(|i| i.label.as_str()), Some("  reviewer"));
    let action = hub.selected().unwrap().action.clone();
    let msg = apply(&d, &action).unwrap();
    assert_eq!(msg, "persona is now reviewer");
    assert_eq!(Settings::load(&d).persona.as_deref(), Some("reviewer"));
    // and the box now marks it as the current one
    hub.refresh();
    assert_eq!(hub.selected().map(|i| i.label.as_str()), Some("● reviewer"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn picking_a_model_writes_it_and_its_provider() {
    let d = seeded();
    let msg = apply(&d, &Action::SetModel("gemini-2.5-pro".into())).unwrap();
    assert!(msg.contains("gemini-2.5-pro"), "{msg}");
    let s = Settings::load(&d);
    assert_eq!(s.model.as_deref(), Some("gemini-2.5-pro"));
    assert_eq!(
        s.provider.as_deref(),
        Some("gemini"),
        "the provider follows the model name"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn picking_a_model_reroutes_even_after_an_endpoint_was_picked() {
    let d = seeded();
    let mut s = Settings::load(&d);
    s.provider = Some("gw".into());
    s.providers.insert(
        "gw".into(),
        ProviderCfg {
            base_url: "https://gw.example/v1".into(),
            ..Default::default()
        },
    );
    s.save_project(&d).unwrap();

    // the stale choice must not outvote the model just picked
    apply(&d, &Action::SetModel("claude-sonnet-4".into())).unwrap();
    let after = Settings::load(&d);
    assert_eq!(after.model.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(after.provider.as_deref(), Some("anthropic"));
    // the endpoint itself stays available, it is just not the one answering
    assert!(after.providers.contains_key("gw"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_provider_box_lists_a_custom_endpoint_and_enter_picks_it() {
    let d = seeded();
    let mut s = Settings::load(&d);
    s.providers.insert(
        "gw".into(),
        ProviderCfg {
            base_url: "https://gw.example/v1".into(),
            api_key_env: Some("GW_KEY".into()),
            kind: "openai".into(),
            ..Default::default()
        },
    );
    s.save_project(&d).unwrap();
    let mut hub = Hub::open(&d);
    hub.next_section();
    assert_eq!(hub.current(), Section::Providers);

    // the custom endpoint sits next to the built-ins, and select it
    let gw = hub
        .items()
        .iter()
        .position(|i| i.label.ends_with("gw"))
        .expect("gw is listed");
    while hub.selected().map(|i| i.label.as_str()) != Some("  gw") {
        hub.down();
        assert!(
            hub.selected().map(|i| i.label.as_str()) != Some("● gw"),
            "gw is already marked before it was picked"
        );
    }
    let _ = gw;
    hub.submit().unwrap();

    let after = Settings::load(&d);
    assert_eq!(after.provider.as_deref(), Some("gw"));
    assert!(
        after.model.is_none(),
        "an endpoint with no model of its own leaves the model alone"
    );
    // and the row now wears the dot
    assert_eq!(hub.selected().map(|i| i.label.as_str()), Some("● gw"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ctrl_w_adds_a_custom_endpoint_that_then_answers() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    hub.next_section();
    assert_eq!(hub.current(), Section::Providers);

    hub.begin(Purpose::NewProvider);
    assert!(hub.typing(), "the box takes the endpoint line");
    for c in "gw https://gw.example/v1 GW_KEY anthropic".chars() {
        hub.type_char(c);
    }
    let outcome = hub.submit().unwrap();

    let s = Settings::load(&d);
    let gw = s.providers.get("gw").expect("gw was written");
    assert_eq!(gw.base_url, "https://gw.example/v1");
    assert_eq!(gw.api_key_env.as_deref(), Some("GW_KEY"));
    assert_eq!(gw.kind, "anthropic");
    assert_eq!(s.provider.as_deref(), Some("gw"));
    assert!(matches!(outcome, Outcome::Say(m) if m.contains("gw")));

    // it is listed, and it is the one answering
    let marked: Vec<&str> = hub
        .items()
        .iter()
        .filter(|i| i.label.starts_with("● "))
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(marked, vec!["● gw"], "{:?}", hub.items());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_nonsense_provider_line_is_refused_and_writes_nothing() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    hub.next_section();
    hub.begin(Purpose::NewProvider);
    for c in "gw not-a-url".chars() {
        hub.type_char(c);
    }
    let err = hub.submit().unwrap_err();
    assert!(err.contains("usage"), "{err}");
    let s = Settings::load(&d);
    assert!(s.providers.is_empty(), "nothing was written");
    let _ = std::fs::remove_dir_all(&d);
}
