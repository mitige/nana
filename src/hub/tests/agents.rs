use super::*;

#[test]
fn the_agents_box_starts_with_the_ceo_alone() {
    let d = project("company");
    let mut hub = Hub::open(&d);
    hub.next_section();
    hub.next_section();
    hub.next_section();
    hub.next_section();
    assert_eq!(hub.current(), Section::Agents);
    assert_eq!(hub.items().len(), 1, "{:?}", hub.items());
    assert_eq!(hub.items()[0].label, "● ceo");
    let _ = std::fs::remove_dir_all(&d);
}
