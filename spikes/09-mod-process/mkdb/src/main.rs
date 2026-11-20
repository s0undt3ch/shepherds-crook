//! Throwaway: creates the spike's project database and its `gen` file.
use crook::store::{Mode, Store};

fn main() {
    let xdg = std::env::args().nth(1).expect("usage: mkdb <xdg> <slug>");
    let slug = std::env::args().nth(2).expect("usage: mkdb <xdg> <slug>");
    let env = move |k: &str| (k == "XDG_DATA_HOME").then(|| xdg.clone());
    let s = Store::open(&env, &slug, Mode::Migrate).expect("open");
    std::fs::write(s.gen_path(), "0").expect("gen");
    println!("{}", s.gen_path().display());
}
