//! A runner that records every call and replays scripted answers, for tests of code that runs
//! subprocesses.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use super::{Cmd, Output, Runner};
use crate::Error;

pub struct FakeRunner {
    answers: RefCell<VecDeque<Result<Output, Error>>>,
    calls: RefCell<Vec<Cmd>>,
}

impl FakeRunner {
    pub fn new(answers: impl IntoIterator<Item = Result<Output, Error>>) -> Self {
        FakeRunner {
            answers: RefCell::new(answers.into_iter().collect()),
            calls: RefCell::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<Cmd> {
        self.calls.borrow().clone()
    }
}

impl Runner for FakeRunner {
    fn run(&self, cmd: Cmd, _timeout: Duration) -> Result<Output, Error> {
        let program = cmd.program.clone();
        self.calls.borrow_mut().push(cmd);
        self.answers.borrow_mut().pop_front().unwrap_or_else(|| {
            Err(Error::Io {
                program,
                source: std::io::Error::other("the fake runner has no answer left"),
            })
        })
    }
}
