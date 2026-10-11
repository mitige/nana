use super::*;

#[test]
fn the_world_box_draws_the_memory_as_lanes_in_time() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    while hub.current() != Section::World {
        hub.next_section();
    }
    let lanes: Vec<&str> = hub.detail().lines().map(str::trim).collect();
    for class in ["user", "feedback", "project", "reference"] {
        assert!(
            lanes.iter().any(|l| l.starts_with(class)),
            "no lane for {class}: {}",
            hub.detail()
        );
    }
    assert!(hub.detail().contains("today"), "{}", hub.detail());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn opening_the_world_box_sweeps_the_map_once() {
    let d = seeded();
    let mut hub = Hub::open(&d);
    assert_eq!(
        hub.sweep_progress(),
        None,
        "no sweep before the map is opened"
    );
    while hub.current() != Section::World {
        hub.next_section();
    }
    let start = std::time::Instant::now();
    assert_eq!(sweep_at(start, start), 0.0);
    assert_eq!(sweep_at(start, start + SWEEP), 1.0);
    assert_eq!(
        sweep_at(start, start + SWEEP * 2),
        1.0,
        "it ends and stays ended"
    );
    assert!(
        hub.sweep_progress().is_some(),
        "opening the map starts the sweep"
    );
    let _ = std::fs::remove_dir_all(&d);
}
