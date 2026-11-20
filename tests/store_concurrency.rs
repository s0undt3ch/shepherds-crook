use std::time::Duration;

use crook::store::{Mode, Store, Touch};

#[test]
fn immediate_concurrent() {
    let tmp = tempfile::tempdir().unwrap();
    let x = tmp.path().to_str().unwrap().to_string();
    let env = move |k: &str| (k == "XDG_DATA_HOME").then(|| x.clone());
    {
        let mut s = Store::open(&env, "p", Mode::Migrate).unwrap();
        s.write(Touch::empty(), |tx| {
            tx.execute("INSERT INTO seen(child, seq) VALUES ('c', 0)", [])?;
            Ok(())
        })
        .unwrap();
    }
    std::thread::scope(|sc| {
        let hs: Vec<_> = (0..2)
            .map(|_| {
                sc.spawn(|| {
                    let mut s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
                    s.write(Touch::empty(), |tx| {
                        let n: i64 =
                            tx.query_row("SELECT seq FROM seen WHERE child='c'", [], |r| r.get(0))?;
                        std::thread::sleep(Duration::from_millis(50));
                        tx.execute("UPDATE seen SET seq=?1 WHERE child='c'", [n + 1])?;
                        Ok(())
                    })
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap().unwrap();
        }
    });
    let s = Store::open(&env, "p", Mode::ReadOnly).unwrap();
    let n: i64 = s
        .read(|c| Ok(c.query_row("SELECT seq FROM seen WHERE child='c'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 2);
}
