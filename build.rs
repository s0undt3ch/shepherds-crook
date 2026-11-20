// `include_dir!` tracks the files it embedded but not a new file appearing
// under `plugin/`, so a new mod file would otherwise ship in no build.
fn main() {
    println!("cargo:rerun-if-changed=plugin");
}
