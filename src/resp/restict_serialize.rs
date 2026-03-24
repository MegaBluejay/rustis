use std::{borrow::Cow, rc::Rc, sync::Arc};

use serde::Serializer;

use crate::resp::BulkString;
#[cfg(feature = "json")]
use crate::resp::JsonRef;

pub trait FastSerialize: serde::Serialize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
}

pub struct WithFastSerialize<T>(pub T);

impl<T: FastSerialize> serde::Serialize for WithFastSerialize<T> {
    #[inline(always)]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        FastSerialize::serialize(&self.0, serializer)
    }
}

#[derive(serde::Serialize)]
#[serde(transparent)]
pub struct WithSerialize<T>(pub T);

macro_rules! serialize_impl {
    ($({$($desc:tt)*}),* $(,)?) => {
        $(
            impl $($desc)* {
                #[inline(always)]
                fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    serde::Serialize::serialize(self, serializer)
                }
            }
        )*
    };
}

serialize_impl! {
    { <T: serde::Serialize> FastSerialize for WithSerialize<T> },
}

#[cfg(feature = "json")]
serialize_impl! {
    { <'a, T: serde::Serialize> FastSerialize for JsonRef<'a, T> },
}

macro_rules! primitive_impl {
    ($($ty:ty),* $(,)?) => {
        serialize_impl!($({ FastSerialize for $ty }),*);
    }
}

primitive_impl!(
    bool, isize, i8, i16, i32, i64, i128, usize, u8, u16, u32, u64, u128, f32, f64, char, str,
    String, BulkString,
);

impl FastSerialize for [u8] {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self)
    }
}

impl<const N: usize> FastSerialize for [u8; N]
where
    [u8; N]: serde::Serialize,
{
    #[inline(always)]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(serializer)
    }
}

impl<T: FastSerialize> FastSerialize for Option<T> {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Some(value) => serializer.serialize_some(&WithFastSerialize(value)),
            None => serializer.serialize_none(),
        }
    }
}

macro_rules! deref_impl {
    ($({$($desc:tt)*}),* $(,)?) => {
        $(
            impl $($desc)* {
                #[inline(always)]
                fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    FastSerialize::serialize(&**self, serializer)
                }
            }
        )*
    };
}

deref_impl! {
    { <'a, T: ?Sized + FastSerialize> FastSerialize for &'a T },
    { <'a, T: ?Sized + FastSerialize> FastSerialize for &'a mut T },
    { <T: ?Sized + FastSerialize> FastSerialize for Box<T> },
    { <T: ?Sized + FastSerialize> FastSerialize for Rc<T> where Rc<T>: serde::Serialize },
    { <T: ?Sized + FastSerialize> FastSerialize for Arc<T> where Arc<T>: serde::Serialize },
    { <'a, T: ?Sized + FastSerialize + ToOwned> FastSerialize for Cow<'a, T> },
    { FastSerialize for Vec<u8> },
}
