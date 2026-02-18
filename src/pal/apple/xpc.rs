use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::c_void;
use std::ptr::{null, null_mut};

use serde::{de, de::IntoDeserializer, ser, Deserialize, Serialize};
use xpc_sys::{
    _xpc_type_array, _xpc_type_bool, _xpc_type_data, _xpc_type_dictionary, _xpc_type_double,
    _xpc_type_int64, _xpc_type_null, _xpc_type_string, _xpc_type_uint64,
    xpc_array_append_value, xpc_array_create, xpc_array_get_count, xpc_array_get_value,
    xpc_bool_create, xpc_bool_get_value, xpc_copy, xpc_copy_description, xpc_data_create,
    xpc_data_get_bytes_ptr, xpc_data_get_length, xpc_dictionary_create, xpc_dictionary_set_value,
    xpc_double_create, xpc_double_get_value, xpc_get_type, xpc_int64_create, xpc_int64_get_value,
    xpc_null_create, xpc_object_t, xpc_release, xpc_retain, xpc_string_create,
    xpc_string_get_string_ptr, xpc_uint64_create, xpc_uint64_get_value,
};

// We need xpc_dictionary_apply and xpc_dictionary_get_value for deserialization.
// These are available through the bindgen-generated bindings in xpc_sys.
use xpc_sys::{xpc_dictionary_get_value};

// ──────────────────────────────────────────────
// AppleObject
// ──────────────────────────────────────────────

/// A wrapper around a raw `xpc_object_t` that manages its lifetime.
pub struct AppleObject {
    ptr: xpc_object_t,
}

unsafe impl Send for AppleObject {}
unsafe impl Sync for AppleObject {}

impl AppleObject {
    /// Create an `AppleObject` from a raw `xpc_object_t`.
    ///
    /// # Safety
    /// The caller must ensure `ptr` is a valid XPC object. Ownership is transferred
    /// to `AppleObject` — it will call `xpc_release` on drop.
    pub unsafe fn from_raw(ptr: xpc_object_t) -> Self {
        Self { ptr }
    }

    /// Create an `AppleObject` from a raw `xpc_object_t`, retaining it first.
    ///
    /// # Safety
    /// The caller must ensure `ptr` is a valid XPC object.
    pub unsafe fn from_raw_retain(ptr: xpc_object_t) -> Self {
        Self {
            ptr: unsafe { xpc_retain(ptr) },
        }
    }

    /// Get the underlying `xpc_object_t` pointer.
    pub fn as_ptr(&self) -> xpc_object_t {
        self.ptr
    }
}

impl Drop for AppleObject {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { xpc_release(self.ptr) }
        }
    }
}

impl Clone for AppleObject {
    fn clone(&self) -> Self {
        unsafe {
            Self {
                ptr: xpc_copy(self.ptr),
            }
        }
    }
}

impl fmt::Debug for AppleObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ptr.is_null() {
            write!(f, "AppleObject(NULL)")
        } else {
            let desc = unsafe { xpc_copy_description(self.ptr) };
            let cstr = unsafe { CStr::from_ptr(desc) };
            write!(f, "AppleObject({})", cstr.to_string_lossy())
        }
    }
}

// ──────────────────────────────────────────────
// Error
// ──────────────────────────────────────────────

#[derive(Debug)]
pub enum Error {
    Message(String),
    UnsupportedType(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Message(msg) => write!(f, "{}", msg),
            Error::UnsupportedType(ty) => write!(f, "unsupported XPC type: {}", ty),
        }
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

// ──────────────────────────────────────────────
// Serializer
// ──────────────────────────────────────────────

pub struct Serializer;

impl<'a> ser::Serializer for &'a mut Serializer {
    type Ok = AppleObject;
    type Error = Error;

    type SerializeSeq = SeqSerializer;
    type SerializeTuple = SeqSerializer;
    type SerializeTupleStruct = SeqSerializer;
    type SerializeTupleVariant = TupleVariantSerializer;
    type SerializeMap = MapSerializer;
    type SerializeStruct = MapSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    fn serialize_bool(self, v: bool) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(xpc_bool_create(v)) })
    }

    fn serialize_i8(self, v: i8) -> Result<AppleObject, Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i16(self, v: i16) -> Result<AppleObject, Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i32(self, v: i32) -> Result<AppleObject, Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i64(self, v: i64) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(xpc_int64_create(v)) })
    }

    fn serialize_u8(self, v: u8) -> Result<AppleObject, Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u16(self, v: u16) -> Result<AppleObject, Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u32(self, v: u32) -> Result<AppleObject, Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u64(self, v: u64) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(xpc_uint64_create(v)) })
    }

    fn serialize_f32(self, v: f32) -> Result<AppleObject, Error> {
        self.serialize_f64(v as f64)
    }

    fn serialize_f64(self, v: f64) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(xpc_double_create(v)) })
    }

    fn serialize_char(self, v: char) -> Result<AppleObject, Error> {
        self.serialize_str(&v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<AppleObject, Error> {
        let cstr = CString::new(v).map_err(|e| Error::Message(e.to_string()))?;
        Ok(unsafe { AppleObject::from_raw(xpc_string_create(cstr.as_ptr())) })
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<AppleObject, Error> {
        Ok(unsafe {
            AppleObject::from_raw(xpc_data_create(v.as_ptr() as *const c_void, v.len()))
        })
    }

    fn serialize_none(self) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(xpc_null_create()) })
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<AppleObject, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(xpc_null_create()) })
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<AppleObject, Error> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<AppleObject, Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<AppleObject, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<AppleObject, Error> {
        let inner = value.serialize(&mut Serializer)?;
        let dict = unsafe { xpc_dictionary_create(null(), null_mut(), 0) };
        let key = CString::new(variant).map_err(|e| Error::Message(e.to_string()))?;
        unsafe { xpc_dictionary_set_value(dict, key.as_ptr(), inner.as_ptr()) };
        Ok(unsafe { AppleObject::from_raw(dict) })
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<SeqSerializer, Error> {
        Ok(SeqSerializer {
            array: unsafe { xpc_array_create(null_mut(), 0) },
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SeqSerializer, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqSerializer, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<TupleVariantSerializer, Error> {
        Ok(TupleVariantSerializer {
            variant,
            array: unsafe { xpc_array_create(null_mut(), 0) },
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<MapSerializer, Error> {
        Ok(MapSerializer {
            dict: unsafe { xpc_dictionary_create(null(), null_mut(), 0) },
            pending_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<MapSerializer, Error> {
        self.serialize_map(Some(_len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<StructVariantSerializer, Error> {
        Ok(StructVariantSerializer {
            variant,
            dict: unsafe { xpc_dictionary_create(null(), null_mut(), 0) },
        })
    }
}

// ──────────────────────────────────────────────
// Compound Serializers
// ──────────────────────────────────────────────

pub struct SeqSerializer {
    array: xpc_object_t,
}

impl ser::SerializeSeq for SeqSerializer {
    type Ok = AppleObject;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let obj = value.serialize(&mut Serializer)?;
        unsafe { xpc_array_append_value(self.array, obj.as_ptr()) };
        Ok(())
    }

    fn end(self) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(self.array) })
    }
}

impl ser::SerializeTuple for SeqSerializer {
    type Ok = AppleObject;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<AppleObject, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SeqSerializer {
    type Ok = AppleObject;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<AppleObject, Error> {
        ser::SerializeSeq::end(self)
    }
}

pub struct TupleVariantSerializer {
    variant: &'static str,
    array: xpc_object_t,
}

impl ser::SerializeTupleVariant for TupleVariantSerializer {
    type Ok = AppleObject;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let obj = value.serialize(&mut Serializer)?;
        unsafe { xpc_array_append_value(self.array, obj.as_ptr()) };
        Ok(())
    }

    fn end(self) -> Result<AppleObject, Error> {
        let dict = unsafe { xpc_dictionary_create(null(), null_mut(), 0) };
        let key = CString::new(self.variant).map_err(|e| Error::Message(e.to_string()))?;
        unsafe { xpc_dictionary_set_value(dict, key.as_ptr(), self.array) };
        // We own array — release it since the dict now retains it.
        unsafe { xpc_release(self.array) };
        Ok(unsafe { AppleObject::from_raw(dict) })
    }
}

pub struct MapSerializer {
    dict: xpc_object_t,
    pending_key: Option<String>,
}

impl ser::SerializeMap for MapSerializer {
    type Ok = AppleObject;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        // Serialize the key to an AppleObject, then extract as string.
        let key_obj = key.serialize(&mut Serializer)?;
        let xpc_type = unsafe { xpc_get_type(key_obj.as_ptr()) };
        let string_type = unsafe { &_xpc_type_string as *const _ };
        if xpc_type != string_type {
            return Err(Error::Message("map keys must be strings".to_string()));
        }
        let cstr = unsafe { CStr::from_ptr(xpc_string_get_string_ptr(key_obj.as_ptr())) };
        self.pending_key = Some(cstr.to_string_lossy().to_string());
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .pending_key
            .take()
            .ok_or_else(|| Error::Message("serialize_value called before serialize_key".into()))?;
        let val_obj = value.serialize(&mut Serializer)?;
        let ckey = CString::new(key).map_err(|e| Error::Message(e.to_string()))?;
        unsafe { xpc_dictionary_set_value(self.dict, ckey.as_ptr(), val_obj.as_ptr()) };
        Ok(())
    }

    fn end(self) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(self.dict) })
    }
}

impl ser::SerializeStruct for MapSerializer {
    type Ok = AppleObject;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let val_obj = value.serialize(&mut Serializer)?;
        let ckey = CString::new(key).map_err(|e| Error::Message(e.to_string()))?;
        unsafe { xpc_dictionary_set_value(self.dict, ckey.as_ptr(), val_obj.as_ptr()) };
        Ok(())
    }

    fn end(self) -> Result<AppleObject, Error> {
        Ok(unsafe { AppleObject::from_raw(self.dict) })
    }
}

pub struct StructVariantSerializer {
    variant: &'static str,
    dict: xpc_object_t,
}

impl ser::SerializeStructVariant for StructVariantSerializer {
    type Ok = AppleObject;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let val_obj = value.serialize(&mut Serializer)?;
        let ckey = CString::new(key).map_err(|e| Error::Message(e.to_string()))?;
        unsafe { xpc_dictionary_set_value(self.dict, ckey.as_ptr(), val_obj.as_ptr()) };
        Ok(())
    }

    fn end(self) -> Result<AppleObject, Error> {
        let outer = unsafe { xpc_dictionary_create(null(), null_mut(), 0) };
        let key = CString::new(self.variant).map_err(|e| Error::Message(e.to_string()))?;
        unsafe { xpc_dictionary_set_value(outer, key.as_ptr(), self.dict) };
        unsafe { xpc_release(self.dict) };
        Ok(unsafe { AppleObject::from_raw(outer) })
    }
}

// ──────────────────────────────────────────────
// Deserializer
// ──────────────────────────────────────────────

pub struct Deserializer {
    obj: xpc_object_t,
}

impl Deserializer {
    fn from_apple_object(obj: &AppleObject) -> Self {
        // Retain so we have our own reference for the duration of deserialization.
        let ptr = unsafe { xpc_retain(obj.as_ptr()) };
        Self { obj: ptr }
    }

    fn from_raw(ptr: xpc_object_t) -> Self {
        let ptr = unsafe { xpc_retain(ptr) };
        Self { obj: ptr }
    }
}

impl Drop for Deserializer {
    fn drop(&mut self) {
        if !self.obj.is_null() {
            unsafe { xpc_release(self.obj) };
        }
    }
}

macro_rules! xpc_type_eq {
    ($ptr:expr, $static_type:ident) => {
        unsafe { xpc_get_type($ptr) == (&$static_type as *const _ as xpc_sys::xpc_type_t) }
    };
}

impl<'de, 'a> de::Deserializer<'de> for &'a mut Deserializer {
    type Error = Error;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if self.obj.is_null() {
            return visitor.visit_unit();
        }

        if xpc_type_eq!(self.obj, _xpc_type_bool) {
            return visitor.visit_bool(unsafe { xpc_bool_get_value(self.obj) });
        }
        if xpc_type_eq!(self.obj, _xpc_type_int64) {
            return visitor.visit_i64(unsafe { xpc_int64_get_value(self.obj) });
        }
        if xpc_type_eq!(self.obj, _xpc_type_uint64) {
            return visitor.visit_u64(unsafe { xpc_uint64_get_value(self.obj) });
        }
        if xpc_type_eq!(self.obj, _xpc_type_double) {
            return visitor.visit_f64(unsafe { xpc_double_get_value(self.obj) });
        }
        if xpc_type_eq!(self.obj, _xpc_type_string) {
            let cstr = unsafe { CStr::from_ptr(xpc_string_get_string_ptr(self.obj)) };
            return visitor.visit_str(&cstr.to_string_lossy());
        }
        if xpc_type_eq!(self.obj, _xpc_type_data) {
            let ptr = unsafe { xpc_data_get_bytes_ptr(self.obj) };
            let len = unsafe { xpc_data_get_length(self.obj) };
            let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
            return visitor.visit_bytes(slice);
        }
        if xpc_type_eq!(self.obj, _xpc_type_null) {
            return visitor.visit_unit();
        }
        if xpc_type_eq!(self.obj, _xpc_type_array) {
            return visitor.visit_seq(XpcSeqAccess::new(self.obj));
        }
        if xpc_type_eq!(self.obj, _xpc_type_dictionary) {
            return visitor.visit_map(XpcMapAccess::new(self.obj));
        }

        Err(Error::UnsupportedType("unknown XPC type"))
    }

    fn deserialize_bool<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_bool) {
            visitor.visit_bool(unsafe { xpc_bool_get_value(self.obj) })
        } else {
            Err(Error::Message("expected bool".into()))
        }
    }

    fn deserialize_i8<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_int64) {
            visitor.visit_i64(unsafe { xpc_int64_get_value(self.obj) })
        } else {
            Err(Error::Message("expected int64".into()))
        }
    }

    fn deserialize_u8<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_uint64) {
            visitor.visit_u64(unsafe { xpc_uint64_get_value(self.obj) })
        } else {
            Err(Error::Message("expected uint64".into()))
        }
    }

    fn deserialize_f32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_f64(visitor)
    }

    fn deserialize_f64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_double) {
            visitor.visit_f64(unsafe { xpc_double_get_value(self.obj) })
        } else {
            Err(Error::Message("expected double".into()))
        }
    }

    fn deserialize_char<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_str<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_string) {
            let cstr = unsafe { CStr::from_ptr(xpc_string_get_string_ptr(self.obj)) };
            visitor.visit_str(&cstr.to_string_lossy())
        } else {
            Err(Error::Message("expected string".into()))
        }
    }

    fn deserialize_string<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_data) {
            let ptr = unsafe { xpc_data_get_bytes_ptr(self.obj) };
            let len = unsafe { xpc_data_get_length(self.obj) };
            let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
            visitor.visit_bytes(slice)
        } else {
            Err(Error::Message("expected data".into()))
        }
    }

    fn deserialize_byte_buf<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if self.obj.is_null() || xpc_type_eq!(self.obj, _xpc_type_null) {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_array) {
            visitor.visit_seq(XpcSeqAccess::new(self.obj))
        } else {
            Err(Error::Message("expected array".into()))
        }
    }

    fn deserialize_tuple<V: de::Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        // Handle XPC data as a byte sequence so [u8; N] fields work
        if xpc_type_eq!(self.obj, _xpc_type_data) {
            let ptr = unsafe { xpc_data_get_bytes_ptr(self.obj) };
            let len = unsafe { xpc_data_get_length(self.obj) };
            let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
            return visitor.visit_seq(ByteSeqAccess { iter: slice.iter() });
        }
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.obj, _xpc_type_dictionary) {
            visitor.visit_map(XpcMapAccess::new(self.obj))
        } else {
            Err(Error::Message("expected dictionary".into()))
        }
    }

    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        // Unit variant: stored as a string
        if xpc_type_eq!(self.obj, _xpc_type_string) {
            let cstr = unsafe { CStr::from_ptr(xpc_string_get_string_ptr(self.obj)) };
            let variant = cstr.to_string_lossy().to_string();
            return visitor.visit_enum(variant.into_deserializer());
        }

        // Other variants: stored as { "VariantName": value }
        if xpc_type_eq!(self.obj, _xpc_type_dictionary) {
            let map = XpcMapAccess::new(self.obj);
            if map.keys.len() == 1 {
                return visitor.visit_enum(XpcEnumAccess {
                    dict: self.obj,
                    variant: map.keys[0].clone(),
                });
            }
        }

        Err(Error::Message("expected string or single-key dictionary for enum".into()))
    }

    fn deserialize_identifier<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_any(visitor)
    }
}

// ──────────────────────────────────────────────
// SeqAccess
// ──────────────────────────────────────────────

struct ByteSeqAccess<'a> {
    iter: std::slice::Iter<'a, u8>,
}

impl<'de, 'a> de::SeqAccess<'de> for ByteSeqAccess<'a> {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        match self.iter.next() {
            Some(&byte) => seed
                .deserialize((byte as u64).into_deserializer())
                .map(Some),
            None => Ok(None),
        }
    }
}

struct XpcSeqAccess {
    array: xpc_object_t,
    index: usize,
    len: usize,
}

impl XpcSeqAccess {
    fn new(array: xpc_object_t) -> Self {
        let len = unsafe { xpc_array_get_count(array) };
        Self {
            array,
            index: 0,
            len,
        }
    }
}

impl<'de> de::SeqAccess<'de> for XpcSeqAccess {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.index >= self.len {
            return Ok(None);
        }
        let elem = unsafe { xpc_array_get_value(self.array, self.index) };
        self.index += 1;
        let mut de = Deserializer::from_raw(elem);
        seed.deserialize(&mut de).map(Some)
    }
}

// ──────────────────────────────────────────────
// MapAccess
// ──────────────────────────────────────────────

struct XpcMapAccess {
    dict: xpc_object_t,
    keys: Vec<String>,
    index: usize,
}

impl XpcMapAccess {
    fn new(dict: xpc_object_t) -> Self {
        // Collect all keys from the dictionary using xpc_dictionary_apply.
        // We use a callback that collects keys into a Vec.
        let keys = collect_dict_keys(dict);
        Self {
            dict,
            keys,
            index: 0,
        }
    }
}

/// Collect dictionary keys by iterating with xpc_dictionary_apply.
fn collect_dict_keys(dict: xpc_object_t) -> Vec<String> {
    use block::ConcreteBlock;
    use std::cell::RefCell;
    use std::os::raw::c_char;
    use std::rc::Rc;

    let keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let keys_clone = keys.clone();

    let block = ConcreteBlock::new(move |key: *const c_char, _value: xpc_object_t| {
        let cstr = unsafe { CStr::from_ptr(key) };
        keys_clone
            .borrow_mut()
            .push(cstr.to_string_lossy().to_string());
        true
    });
    let block = block.copy();

    unsafe {
        xpc_sys::xpc_dictionary_apply(dict, &*block as *const _ as *mut _);
    }

    drop(block);

    Rc::try_unwrap(keys)
        .expect("block should be dropped")
        .into_inner()
}

impl<'de> de::MapAccess<'de> for XpcMapAccess {
    type Error = Error;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.index >= self.keys.len() {
            return Ok(None);
        }
        let key_str = &self.keys[self.index];
        // Deserialize the key string.
        let key_obj = {
            let cstr = CString::new(key_str.as_str()).map_err(|e| Error::Message(e.to_string()))?;
            unsafe { xpc_string_create(cstr.as_ptr()) }
        };
        let mut de = Deserializer { obj: key_obj };
        let result = seed.deserialize(&mut de)?;
        // Don't let Deserializer::drop release — we need to do it ourselves
        // Actually the Deserializer drop will handle it since we set obj.
        Ok(Some(result))
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Error> {
        let key_str = &self.keys[self.index];
        self.index += 1;
        let ckey = CString::new(key_str.as_str()).map_err(|e| Error::Message(e.to_string()))?;
        let value = unsafe { xpc_dictionary_get_value(self.dict, ckey.as_ptr()) };
        let mut de = Deserializer::from_raw(value);
        seed.deserialize(&mut de)
    }
}

// ──────────────────────────────────────────────
// EnumAccess
// ──────────────────────────────────────────────

struct XpcEnumAccess {
    dict: xpc_object_t,
    variant: String,
}

impl<'de> de::EnumAccess<'de> for XpcEnumAccess {
    type Error = Error;
    type Variant = XpcVariantAccess;

    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Error> {
        use serde::de::IntoDeserializer;
        let variant_de: de::value::StringDeserializer<Error> =
            self.variant.clone().into_deserializer();
        let val = seed.deserialize(variant_de)?;

        let ckey = CString::new(self.variant.as_str()).map_err(|e| Error::Message(e.to_string()))?;
        let inner = unsafe { xpc_dictionary_get_value(self.dict, ckey.as_ptr()) };

        Ok((val, XpcVariantAccess { inner }))
    }
}

struct XpcVariantAccess {
    inner: xpc_object_t,
}

impl<'de> de::VariantAccess<'de> for XpcVariantAccess {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, Error> {
        let mut de = Deserializer::from_raw(self.inner);
        seed.deserialize(&mut de)
    }

    fn tuple_variant<V: de::Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.inner, _xpc_type_array) {
            visitor.visit_seq(XpcSeqAccess::new(self.inner))
        } else {
            Err(Error::Message("expected array for tuple variant".into()))
        }
    }

    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        if xpc_type_eq!(self.inner, _xpc_type_dictionary) {
            visitor.visit_map(XpcMapAccess::new(self.inner))
        } else {
            Err(Error::Message("expected dictionary for struct variant".into()))
        }
    }
}

// ──────────────────────────────────────────────
// Convenience functions
// ──────────────────────────────────────────────

/// Serialize a value into an `AppleObject` (XPC object).
pub fn to_xpc<T: Serialize>(value: &T) -> Result<AppleObject, Error> {
    value.serialize(&mut Serializer)
}

/// Deserialize an `AppleObject` (XPC object) into a value.
pub fn from_xpc<'de, T: Deserialize<'de>>(obj: &AppleObject) -> Result<T, Error> {
    let mut de = Deserializer::from_apple_object(obj);
    T::deserialize(&mut de)
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn roundtrip_bool() {
        let obj = to_xpc(&true).unwrap();
        let val: bool = from_xpc(&obj).unwrap();
        assert_eq!(val, true);
    }

    #[test]
    fn roundtrip_i64() {
        let obj = to_xpc(&-42i64).unwrap();
        let val: i64 = from_xpc(&obj).unwrap();
        assert_eq!(val, -42);
    }

    #[test]
    fn roundtrip_u64() {
        let obj = to_xpc(&999u64).unwrap();
        let val: u64 = from_xpc(&obj).unwrap();
        assert_eq!(val, 999);
    }

    #[test]
    fn roundtrip_f64() {
        let obj = to_xpc(&3.14f64).unwrap();
        let val: f64 = from_xpc(&obj).unwrap();
        assert!((val - 3.14).abs() < f64::EPSILON);
    }

    #[test]
    fn roundtrip_string() {
        let obj = to_xpc(&"hello world").unwrap();
        let val: String = from_xpc(&obj).unwrap();
        assert_eq!(val, "hello world");
    }

    #[test]
    fn roundtrip_option_some() {
        let obj = to_xpc(&Some(42i64)).unwrap();
        let val: Option<i64> = from_xpc(&obj).unwrap();
        assert_eq!(val, Some(42));
    }

    #[test]
    fn roundtrip_option_none() {
        let obj = to_xpc(&None::<i64>).unwrap();
        let val: Option<i64> = from_xpc(&obj).unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn roundtrip_vec() {
        let obj = to_xpc(&vec![1i64, 2, 3]).unwrap();
        let val: Vec<i64> = from_xpc(&obj).unwrap();
        assert_eq!(val, vec![1, 2, 3]);
    }

    #[test]
    fn roundtrip_struct() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Point {
            x: i64,
            y: i64,
        }

        let p = Point { x: 10, y: 20 };
        let obj = to_xpc(&p).unwrap();
        let val: Point = from_xpc(&obj).unwrap();
        assert_eq!(val, p);
    }

    #[test]
    fn roundtrip_nested_struct() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Inner {
            value: String,
        }

        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Outer {
            name: String,
            count: u64,
            inner: Inner,
        }

        let data = Outer {
            name: "test".into(),
            count: 42,
            inner: Inner {
                value: "nested".into(),
            },
        };

        let obj = to_xpc(&data).unwrap();
        let val: Outer = from_xpc(&obj).unwrap();
        assert_eq!(val, data);
    }

    #[test]
    fn roundtrip_unit_enum() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum Color {
            Red,
            Green,
            Blue,
        }

        let obj = to_xpc(&Color::Green).unwrap();
        let val: Color = from_xpc(&obj).unwrap();
        assert_eq!(val, Color::Green);
    }

    #[test]
    fn roundtrip_newtype_variant() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum Value {
            Int(i64),
            Text(String),
        }

        let obj = to_xpc(&Value::Int(99)).unwrap();
        let val: Value = from_xpc(&obj).unwrap();
        assert_eq!(val, Value::Int(99));
    }

    #[test]
    fn roundtrip_bytes() {
        let data: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF];
        let obj = to_xpc(&serde_bytes::Bytes::new(data)).unwrap();
        let val: serde_bytes::ByteBuf = from_xpc(&obj).unwrap();
        assert_eq!(val.as_ref(), data);
    }
}
