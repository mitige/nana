use super::*;

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
