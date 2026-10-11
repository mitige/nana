use super::*;

#[test]
fn the_detail_pane_shows_the_page_itself() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    // find the stack page and show it
    while hub.selected().map(|i| i.label.as_str()) != Some("project: stack") {
        hub.down();
    }
    assert!(
        hub.detail().contains("rust only, no async"),
        "{}",
        hub.detail()
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn sections_cycle_and_keep_their_own_cursor() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    hub.down();
    let first_cursor = hub.cursor();
    assert_eq!(first_cursor, 1);
    hub.next_section();
    assert_eq!(hub.current(), Section::Providers);
    assert_eq!(hub.cursor(), 0, "a fresh section starts at the top");
    hub.prev_section();
    assert_eq!(hub.current(), Section::Memory);
    assert_eq!(hub.cursor(), first_cursor, "and remembers where you were");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_page_shows_its_links_its_backlinks_and_its_history() {
    let d = seeded();
    let m = Memory::open(&d);
    m.write(Class::Project, "stack", "rust only, see [[deploy]]")
        .unwrap();
    m.write(Class::Project, "deploy", "ships from [[stack]]")
        .unwrap();
    m.write(
        Class::Project,
        "stack",
        "rust only, no async, see [[deploy]]",
    )
    .unwrap();
    let mut hub = Hub::open(&d);
    while hub.selected().map(|i| i.label.as_str()) != Some("project: stack") {
        hub.down();
    }
    let detail = hub.detail().to_string();
    assert!(detail.contains("links to: deploy"), "{detail}");
    assert!(detail.contains("linked from: deploy"), "{detail}");
    assert!(detail.contains("history:"), "{detail}");
    assert!(detail.contains("3 version"), "{detail}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn typing_filters_the_list() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    for c in "tone".chars() {
        hub.push_char(c);
    }
    assert_eq!(hub.items().len(), 1, "{:?}", hub.items());
    assert_eq!(hub.items()[0].label, "user: tone");
    hub.pop_char();
    hub.pop_char();
    hub.pop_char();
    hub.pop_char();
    assert_eq!(hub.items().len(), 2);
    let _ = std::fs::remove_dir_all(&d);
}
