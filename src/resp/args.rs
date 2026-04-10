use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use serde::{Serialize, Serializer};

#[cfg(feature = "json")]
use crate::resp::JsonRef;
use crate::resp::{BulkString, RefBulkString};

pub trait Args {
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;

    #[inline]
    fn serialize_args_slice<S: Serializer>(items: &[Self], serializer: S) -> Result<S::Ok, S::Error>
    where
        Self: Sized,
    {
        serializer.collect_seq(items.iter().map(Serde))
    }
}

pub trait Arg: Args {}

pub struct Serde<T>(pub T);

impl<T: Serialize> Args for Serde<T> {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<T: Serialize> Arg for Serde<T> {}

impl<T: Args> Serialize for Serde<T> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize_args(serializer)
    }
}

macro_rules! primitive_impl {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Args for $ty {
                #[inline]
                fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    self.serialize(serializer)
                }
            }

            impl Arg for $ty {}
        )*
    };
}

primitive_impl! {
    bool,
    char,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    u16,
    u32,
    u64,
    u128,
    usize,
    f32,
    f64,
    str,
    String,
    (),
}

#[cfg(feature = "json")]
impl<T: Serialize> Args for JsonRef<'_, T> {
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize(serializer)
    }
}

#[cfg(feature = "json")]
impl<T: Serialize> Arg for JsonRef<'_, T> {}

impl Args for u8 {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize(serializer)
    }

    #[inline]
    fn serialize_args_slice<S: Serializer>(items: &[Self], serializer: S) -> Result<S::Ok, S::Error>
    where
        Self: Sized,
    {
        serializer.serialize_bytes(items)
    }
}

impl Arg for u8 {}
impl Arg for [u8] {}
impl<const N: usize> Arg for [u8; N] {}
impl Arg for Vec<u8> {}
impl Arg for BulkString {}
impl Arg for RefBulkString<'_> {}

impl<T: Args> Args for Option<T> {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Some(value) => serializer.serialize_some(&Serde(value)),
            None => serializer.serialize_none(),
        }
    }
}

impl<T: Arg> Arg for Option<T> {}

macro_rules! deref_impl {
    ($($ty:ty),* $(,)?) => {
        $(
            impl<T: Args + ?Sized> Args for $ty {
                #[inline]
                fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    (**self).serialize_args(serializer)
                }
            }

            impl<T: Arg + ?Sized> Arg for $ty {}
        )*
    };
}

deref_impl! {
    &'_ T,
    &'_ mut T,
    Box<T>,
    Rc<T>,
    Arc<T>,
}

impl<T: Args + ToOwned> Args for Cow<'_, T> {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize_args(serializer)
    }
}

impl<T: Arg + ToOwned> Arg for Cow<'_, T> {}

impl<T: Args> Args for [T] {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        T::serialize_args_slice(self, serializer)
    }
}

impl<const N: usize, T: Args> Args for [T; N] {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        T::serialize_args_slice(&self[..], serializer)
    }
}

impl<T: Args> Args for Vec<T> {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        T::serialize_args_slice(&self[..], serializer)
    }
}

macro_rules! iter_impl {
    ($($ty:ty),* $(,)?) => {
        $(
            impl<T: Args> Args for $ty {
                #[inline]
                fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    serializer.collect_seq(self.iter().map(Serde))
                }
            }
        )*
    };
}

iter_impl! {
    HashSet<T>,
    BTreeSet<T>,
}

macro_rules! map_impl {
    ($($ty:ty),* $(,)?) => {
        $(
            impl<K: Args, V: Args> Args for $ty {
                #[inline]
                fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    serializer.collect_map(self.iter().map(|(k, v)| (Serde(k), Serde(v))))
                }
            }
        )*
    };
}

map_impl! {
    HashMap<K, V>,
    BTreeMap<K, V>,
}

impl<T0: Args, T1: Args> Args for (T0, T1) {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (Serde(&self.0), Serde(&self.1)).serialize(serializer)
    }
}

impl<T0: Args, T1: Args, T2: Args> Args for (T0, T1, T2) {
    #[inline]
    fn serialize_args<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (Serde(&self.0), Serde(&self.1), Serde(&self.2)).serialize(serializer)
    }
}
