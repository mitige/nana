use super::*;
use std::path::PathBuf;

/// a unique scratch project per call: tests run in parallel inside one
/// process, and a shared path means one test deletes another's fixture.
fn project(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "nana-hub-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".nana")).unwrap();
    d
}

fn seeded() -> PathBuf {
    let d = project("seeded");
    let m = Memory::open(&d);
    m.write(Class::Project, "stack", "rust only, no async")
        .unwrap();
    m.write(Class::User, "tone", "blunt, no filler").unwrap();
    std::fs::create_dir_all(d.join(".nana/skills")).unwrap();
    std::fs::write(
        d.join(".nana/skills/release.md"),
        "---\nname: release\ndescription: cut a release\ntrigger: release\n---\nsteps\n",
    )
    .unwrap();
    std::fs::create_dir_all(d.join(".nana/personas")).unwrap();
    std::fs::write(
        d.join(".nana/personas/reviewer.md"),
        "---\ntitle: The Reviewer\ndescription: reads code\n---\nbe precise.\n",
    )
    .unwrap();
    d
}

mod agents;
mod boxes;
mod navigation;
mod settings;
mod world;
