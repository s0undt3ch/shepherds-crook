//! `crook unpark <child>`: lifts a park by hand, for when the mod's filter is
//! skipped or broken.

use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, crew};

use super::address;

pub fn run(env: Env, target: &str) -> Result<String, Error> {
    let (slug, child) = address::resolve(env, target)?;
    let mut store = Store::open(env, &slug, Mode::ReadWrite)?;
    let lifted = store.write(Touch::STATUS, |tx| {
        crew::parked(tx, &child)?;
        crew::lift(tx, &child)
    })?;
    Ok(if lifted {
        format!("{slug}/{child}: unparked")
    } else {
        format!("{slug}/{child}: was not parked")
    })
}
