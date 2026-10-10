//! the one-line installers must exist, parse, and name the assets the release
//! publishes: a script that asks for an asset nobody uploads installs nothing.

use std::fs;
use std::path::Path;
use std::process::Command;

const LINUX_ASSET: &str = "nana-linux-x86_64.tar.gz";
const WINDOWS_ASSET: &str = "nana-windows-x86_64.zip";

fn read(path: &str) -> String {
    let full = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&full).unwrap_or_else(|e| panic!("{path} is missing: {e}"))
}

#[test]
fn the_linux_installer_is_posix_sh_and_fetches_the_release_asset() {
    let script = read("install.sh");
    assert!(
        script.contains(LINUX_ASSET),
        "install.sh must fetch {LINUX_ASSET}"
    );
    assert!(
        script.contains("github.com/mitige/nana/releases"),
        "install.sh must download from the release page"
    );
    let status = Command::new("sh")
        .arg("-n")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
        .status()
        .expect("sh is available");
    assert!(status.success(), "install.sh does not parse as sh");
}

#[test]
fn the_windows_installer_fetches_the_release_asset() {
    let script = read("install.ps1");
    assert!(
        script.contains(WINDOWS_ASSET),
        "install.ps1 must fetch {WINDOWS_ASSET}"
    );
    assert!(
        script.contains("github.com/mitige/nana/releases"),
        "install.ps1 must download from the release page"
    );
}

#[test]
fn the_readme_gives_both_one_line_installs() {
    let readme = read("README.md");
    assert!(
        readme.contains("install.sh"),
        "README lacks the linux one-liner"
    );
    assert!(
        readme.contains("install.ps1"),
        "README lacks the windows one-liner"
    );
}
