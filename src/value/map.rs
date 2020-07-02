use std::{slice::Iter, vec::IntoIter};

use codemap::Span;

use crate::{
    common::{Brackets, ListSeparator},
    error::SassResult,
    value::Value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SassMap(Vec<(Value, Value)>);

impl SassMap {
    pub const fn new() -> SassMap {
        SassMap(Vec::new())
    }

    pub fn get(self, key: &Value, span: Span) -> SassResult<Option<Value>> {
        for (k, v) in self.0 {
            if k.equals(key.clone(), span)?.node.is_true(span)? {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, key: &Value) {
        self.0.retain(|(ref k, ..)| k != key);
    }

    pub fn merge(&mut self, other: SassMap) {
        for (key, value) in other {
            self.insert(key, value);
        }
    }

    pub fn iter(&self) -> Iter<(Value, Value)> {
        self.0.iter()
    }

    pub fn keys(self) -> Vec<Value> {
        self.0.into_iter().map(|(k, ..)| k).collect()
    }

    pub fn values(self) -> Vec<Value> {
        self.0.into_iter().map(|(.., v)| v).collect()
    }

    pub fn as_list(self) -> Vec<Value> {
        self.0
            .into_iter()
            .map(|(k, v)| Value::List(vec![k, v], ListSeparator::Space, Brackets::None))
            .collect()
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn entries(self) -> Vec<(Value, Value)> {
        self.0
    }

    /// Returns true if the key already exists
    pub fn insert(&mut self, key: Value, value: Value) -> bool {
        for (ref k, ref mut v) in &mut self.0 {
            if k == &key {
                *v = value;
                return true;
            }
        }
        self.0.push((key, value));
        false
    }
}

impl IntoIterator for SassMap {
    type Item = (Value, Value);
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
