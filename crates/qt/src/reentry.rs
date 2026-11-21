// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! For the GUI test: tells where Qt or QML calls back into the controller or a terminal item
//! while one of their methods still runs. Such a call comes through a signal or a property
//! that the method changes, and is not to be seen in the Rust code; a change in the order of
//! the work can add one without notice. Each pair goes to stderr once, as
//! `kudari: reentrant <inner> in <outer>`, and `tests/gui/run.sh` compares them with the
//! pairs it knows, in `tests/gui/reentry.txt`.
//!
//! Nothing here may panic or stop the program, which the release does not do either: cells
//! instead of `RefCell` for the stack, and a stack that is full takes no more names.

use std::cell::{Cell, RefCell};

/// How deep the stack of names goes; deeper calls are counted, but not named.
const DEPTH: usize = 8;

thread_local! {
    static NAMES: Cell<[&'static str; DEPTH]> = const { Cell::new([""; DEPTH]) };
    static DEPTH_NOW: Cell<usize> = const { Cell::new(0) };
    static TOLD: RefCell<Vec<(&'static str, &'static str)>> = const { RefCell::new(Vec::new()) };
}

/// Marks a guarded method as running until it is dropped; see [`enter`].
pub struct Entered;

impl Drop for Entered {
    fn drop(&mut self) {
        DEPTH_NOW.set(DEPTH_NOW.get().saturating_sub(1));
    }
}

/// Enters the guarded method `name`, and tells if another one still runs on this thread.
#[must_use]
pub fn enter(name: &'static str) -> Entered {
    let depth = DEPTH_NOW.get();
    if depth > 0 {
        let outer = NAMES.get().get(depth - 1).copied().unwrap_or("a deeper call");
        tell(name, outer);
    }
    let mut names = NAMES.get();
    if let Some(slot) = names.get_mut(depth) {
        *slot = name;
        NAMES.set(names);
    }
    DEPTH_NOW.set(depth + 1);
    Entered
}

fn tell(inner: &'static str, outer: &'static str) {
    TOLD.with(|told| {
        let Ok(mut told) = told.try_borrow_mut() else {
            return;
        };
        if !told.contains(&(inner, outer)) {
            told.push((inner, outer));
            eprintln!("kudari: reentrant {inner} in {outer}");
        }
    });
}

/// The pairs told on this thread so far.
#[cfg(test)]
fn told() -> Vec<(&'static str, &'static str)> {
    TOLD.with(|told| told.borrow().clone())
}

/// Guards the method it stands in, in the build with `test-hooks`; nothing otherwise.
macro_rules! reentry {
    ($name:literal) => {
        #[cfg(feature = "test-hooks")]
        let _entered = $crate::reentry::enter($name);
    };
}
pub(crate) use reentry;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_back_is_told_once_for_each_pair() {
        {
            let _outer = enter("outer");
            {
                let _inner = enter("inner");
            }
            let _again = enter("inner");
        }
        drop(enter("inner"));
        {
            let _inner = enter("inner");
            let _deeper = enter("deeper");
        }
        assert_eq!(told(), [("inner", "outer"), ("deeper", "inner")]);
        assert_eq!(DEPTH_NOW.get(), 0);
    }

    #[test]
    fn a_stack_deeper_than_its_room_does_not_break() {
        let entered: Vec<Entered> = (0..DEPTH + 4).map(|_| enter("again")).collect();
        assert_eq!(DEPTH_NOW.get(), DEPTH + 4);
        drop(entered);
        assert_eq!(DEPTH_NOW.get(), 0);
        // Beyond the room of the stack, the outer name is not known.
        assert_eq!(told(), [("again", "again"), ("again", "a deeper call")]);
    }
}
