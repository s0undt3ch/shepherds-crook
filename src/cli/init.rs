use crate::{Env, Error, project};

pub fn run(env: Env, slug: &str) -> Result<String, Error> {
    project::init(env, slug)?;
    Ok(format!("created project {slug}"))
}
