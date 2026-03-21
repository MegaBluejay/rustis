#[cfg(feature = "json")]
pub use crate::resp::JsonRef;
#[cfg(debug_assertions)]
use crate::resp::next_sequence_counter;
use crate::resp::{ArgLayout, BulkString, Command, hash_slot};
use bytes::{BufMut, BytesMut};
use dtoa::Float;
use itoa::Integer;
use serde::{Serialize, Serializer, ser};
use smallvec::SmallVec;
use std::{borrow::Cow, fmt::Error, ops::Range, rc::Rc, sync::Arc};

pub trait FastSerialize: Serialize {
    fn rserialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
}

pub struct WithFastSerialize<T>(pub T);

impl<T: FastSerialize> Serialize for WithFastSerialize<T> {
    #[inline(always)]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.rserialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub struct WithSerialize<T>(pub T);

macro_rules! serialize_impl {
    ($({$($desc:tt)*}),* $(,)?) => {
        $(
            impl $($desc)* {
                #[inline(always)]
                fn rserialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    self.serialize(serializer)
                }
            }
        )*
    };
}

serialize_impl! {
    { <T: Serialize> FastSerialize for WithSerialize<T> },
}

#[cfg(feature = "json")]
serialize_impl! {
    { <'a, T: Serialize> FastSerialize for JsonRef<'a, T> },
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
    fn rserialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self)
    }
}

impl<const N: usize> FastSerialize for [u8; N]
where
    [u8; N]: Serialize,
{
    #[inline(always)]
    fn rserialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_slice().rserialize(serializer)
    }
}

impl<T: FastSerialize> FastSerialize for Option<T> {
    #[inline(always)]
    fn rserialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
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
                fn rserialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    (**self).rserialize(serializer)
                }
            }
        )*
    };
}

deref_impl! {
    { <'a, T: ?Sized + FastSerialize> FastSerialize for &'a T },
    { <'a, T: ?Sized + FastSerialize> FastSerialize for &'a mut T },
    { <T: ?Sized + FastSerialize> FastSerialize for Box<T> },
    { <T: ?Sized + FastSerialize> FastSerialize for Rc<T> where Rc<T>: Serialize },
    { <T: ?Sized + FastSerialize> FastSerialize for Arc<T> where Arc<T>: Serialize },
    { <'a, T: ?Sized + FastSerialize + ToOwned> FastSerialize for Cow<'a, T> },
    { FastSerialize for Vec<u8> },
}

pub struct FastPathCommandBuilder {
    buffer: BytesMut,
    name_layout: (usize, usize),
    args_layout: SmallVec<[ArgLayout; 10]>,
}

impl FastPathCommandBuilder {
    #[inline(always)]
    pub fn new(header: &[u8], name_layout: (usize, usize)) -> Self {
        let mut buffer = BytesMut::with_capacity(128);
        buffer.put_slice(header);

        FastPathCommandBuilder {
            buffer,
            name_layout,
            args_layout: SmallVec::new(),
        }
    }

    #[inline(always)]
    pub fn arg(mut self, arg: impl FastSerialize) -> Self {
        let mut serializer = FastPathRespSerializer::new(&mut self.buffer);
        let range = arg
            .rserialize(&mut serializer)
            .expect("Argument serialization failed");

        self.args_layout.push(ArgLayout::arg(range));
        self
    }

    #[inline(always)]
    pub fn key(mut self, key: impl FastSerialize) -> Self {
        let mut serializer = FastPathRespSerializer::new(&mut self.buffer);
        let range = key
            .rserialize(&mut serializer)
            .expect("Argument serialization failed");

        self.args_layout.push(ArgLayout::key(
            range.clone(),
            hash_slot(&self.buffer[range]),
        ));
        self
    }

    #[inline(always)]
    pub fn build(self) -> Command {
        Command::new(
            self.buffer.freeze(),
            self.name_layout,
            self.args_layout,
            #[cfg(debug_assertions)]
            0,
            #[cfg(debug_assertions)]
            next_sequence_counter(),
            None,
            None,
            0,
        )
    }

    #[inline(always)]
    pub fn get(key: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*2\r\n$3\r\nGET\r\n", (8, 3))
            .key(key)
            .build()
    }

    #[inline(always)]
    pub fn set(key: impl FastSerialize, value: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$3\r\nSET\r\n", (8, 3))
            .key(key)
            .arg(value)
            .build()
    }

    #[inline(always)]
    pub fn expire(key: impl FastSerialize, seconds: u64) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$6\r\nEXPIRE\r\n", (8, 6))
            .key(key)
            .arg(seconds)
            .build()
    }

    #[inline(always)]
    pub fn hget(key: impl FastSerialize, field: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$4\r\nHGET\r\n", (8, 4))
            .key(key)
            .arg(field)
            .build()
    }

    #[inline(always)]
    pub fn hincrby(key: impl FastSerialize, field: impl FastSerialize, increment: i64) -> Command {
        FastPathCommandBuilder::new(b"*4\r\n$7\r\nHINCRBY\r\n", (8, 7))
            .key(key)
            .arg(field)
            .arg(increment)
            .build()
    }

    #[inline(always)]
    pub fn sismember(key: impl FastSerialize, member: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$9\r\nSISMEMBER\r\n", (8, 9))
            .key(key)
            .arg(member)
            .build()
    }

    #[inline(always)]
    pub fn zincrby(key: impl FastSerialize, increment: f64, member: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*4\r\n$7\r\nZINCRBY\r\n", (8, 7))
            .key(key)
            .arg(increment)
            .arg(member)
            .build()
    }

    #[inline(always)]
    pub fn publish(channel: impl FastSerialize, message: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$7\r\nPUBLISH\r\n", (8, 7))
            .arg(channel)
            .arg(message)
            .build()
    }

    #[inline(always)]
    pub fn lpush(key: impl FastSerialize, element: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$5\r\nLPUSH\r\n", (8, 5))
            .key(key)
            .arg(element)
            .build()
    }

    #[inline(always)]
    pub fn rpush(key: impl FastSerialize, element: impl FastSerialize) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$5\r\nRPUSH\r\n", (8, 5))
            .key(key)
            .arg(element)
            .build()
    }

    #[inline(always)]
    pub fn lpop(key: impl FastSerialize, count: u32) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$4\r\nLPOP\r\n", (8, 4))
            .key(key)
            .arg(count)
            .build()
    }

    #[inline(always)]
    pub fn rpop(key: impl FastSerialize, count: u32) -> Command {
        FastPathCommandBuilder::new(b"*3\r\n$4\r\nRPOP\r\n", (8, 4))
            .key(key)
            .arg(count)
            .build()
    }
}

struct FastPathRespSerializer<'a> {
    buffer: &'a mut BytesMut,
}

impl<'a> FastPathRespSerializer<'a> {
    #[inline(always)]
    pub fn new(buffer: &'a mut BytesMut) -> Self {
        FastPathRespSerializer { buffer }
    }

    #[inline(always)]
    fn serialize_integer<I: Integer>(&mut self, i: I) -> Range<usize> {
        let mut buf = itoa::Buffer::new();
        self.write_arg(buf.format(i).as_bytes())
    }

    #[inline(always)]
    fn serialize_float<F: Float>(&mut self, f: F) -> Range<usize> {
        let mut buf = dtoa::Buffer::new();
        self.write_arg(buf.format(f).as_bytes())
    }

    /// Serializes a raw argument into the buffer using RESP format (BulkString).
    ///
    /// # Format
    /// `$Length\r\nData\r\n`
    #[inline(always)]
    pub fn write_arg(&mut self, data: &[u8]) -> Range<usize> {
        // 1. Write the RESP BulkString header ($Len\r\n)
        let data_len = data.len();
        let mut len_buf = itoa::Buffer::new();
        let len_str = len_buf.format(data_len);
        let len_bytes = len_str.as_bytes();
        let total_size = 1 + len_bytes.len() + 2 + data_len + 2;
        self.buffer.reserve(total_size);
        self.buffer.put_u8(b'$');
        self.buffer.put_slice(len_bytes);
        self.buffer.put_slice(b"\r\n");

        // 2. Capture the absolute position of the data for the index
        let start_pos = self.buffer.len();

        // 3. Write the actual data
        self.buffer.put_slice(data);
        self.buffer.put_slice(b"\r\n");

        // 4. return the layout range
        start_pos..start_pos + data_len
    }
}

impl<'a> Serializer for &'a mut FastPathRespSerializer<'a> {
    type Ok = Range<usize>;
    type Error = Error;
    type SerializeSeq = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeMap = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = ser::Impossible<Self::Ok, Self::Error>;

    #[inline(always)]
    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        const BOOL_VALS: [&[u8]; 2] = [b"0", b"1"];
        Ok(self.write_arg(BOOL_VALS[v as usize]))
    }

    #[inline(always)]
    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_integer(v))
    }

    #[inline(always)]
    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_float(v))
    }

    #[inline(always)]
    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(self.serialize_float(v))
    }

    #[inline(always)]
    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let mut buf = [0; 4];
        let str = v.encode_utf8(&mut buf);
        Ok(self.write_arg(str.as_bytes()))
    }

    #[inline(always)]
    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(self.write_arg(v.as_bytes()))
    }

    #[inline(always)]
    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(self.write_arg(v))
    }

    #[inline(always)]
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    #[inline(always)]
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(ser::Error::custom("FastPath only supports primitives"))
    }
}
