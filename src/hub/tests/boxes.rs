use super::*;

#[test]
fn the_memory_box_lists_what_the_project_remembers() {
    let d = seeded();
    let hub = Hub::open(&d);
    assert_eq!(hub.current(), Section::Memory);
    let names: Vec<&str> = hub.items().iter().map(|i| i.label.as_str()).collect();
    assert!(names.contains(&"project: stack"), "{names:?}");
    assert!(names.contains(&"user: tone"), "{names:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_knowledge_box_lists_the_wiki_pages() {
    let d = seeded();
    crate::knowledge::write(&d, "build", "# how it builds\n\ncargo build").unwrap();
    let mut hub = Hub::open(&d);
    while hub.current() != Section::Knowledge {
        hub.next_section();
    }
    let labels: Vec<&str> = hub.items().iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["build"], "{labels:?}");
    assert!(
        hub.items()[0].note.contains("how it builds"),
        "{}",
        hub.items()[0].note
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_provider_box_marks_the_current_model() {
    let d = seeded();
    let mut s = Settings::load(&d);
    s.model = Some("claude-sonnet-4".into());
    s.save_project(&d).unwrap();
    let mut hub = Hub::open(&d);
    hub.next_section();
    assert_eq!(hub.current(), Section::Providers);
    let marked: Vec<&str> = hub
        .items()
        .iter()
        .filter(|i| i.label.starts_with("● "))
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(marked, vec!["● anthropic"], "{:?}", hub.items());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_skill_box_offers_to_run_it() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    hub.next_section();
    hub.next_section();
    assert_eq!(hub.current(), Section::Skills);
    assert_eq!(hub.selected().map(|i| i.label.as_str()), Some("release"));
    assert_eq!(
        hub.selected().map(|i| i.action.clone()),
        Some(Action::RunSkill("release".into()))
    );
    assert!(hub.detail().contains("cut a release"), "{}", hub.detail());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_empty_project_still_opens_every_box() {
    let d = project("empty");
    let mut hub = Hub::open(&d);
    for _ in 0..Section::ALL.len() {
        assert!(!hub.current().title().is_empty());
        assert!(
            !hub.detail().is_empty(),
            "{} has no detail",
            hub.current().title()
        );
        hub.next_section();
    }
    assert_eq!(hub.current(), Section::Memory, "it comes back around");
    let _ = std::fs::remove_dir_all(&d);
}
