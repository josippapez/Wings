//! What the tests share: an empty store, and a create operation applied to one.

use crate::{
    domain::{Actor, Create, Op, Store},
    ops::{Applied, apply_op},
    storage::empty_store,
};

pub fn fresh() -> Store {
    empty_store("web")
}

pub fn make(store: &Store, title: &str, extra: impl FnOnce(&mut Create)) -> Store {
    create_with(store, title, extra, &Actor::You, 1.0).store
}

pub fn create_with(store: &Store, title: &str, extra: impl FnOnce(&mut Create), by: &Actor, now: f64) -> Applied {
    let mut create = Op::create(title);
    extra(&mut create);
    apply_op(store, &Op::Create(create), by, now).unwrap()
}
