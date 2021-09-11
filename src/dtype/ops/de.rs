// Copyright 2021 Victor I. Afolabi
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Deserializer for `DType`.
//!

#[cfg(feature = "arbitrary_precision")]
use crate::dtype::number::NumberFromString;
use crate::{DType, DateTime, Error, Map, Number};

use std::{borrow::Cow, fmt, str::FromStr};

use serde::{
  de::{
    self, Deserialize, DeserializeSeed, EnumAccess, Expected, IntoDeserializer,
    MapAccess, SeqAccess, Unexpected, VariantAccess, Visitor,
  },
  forward_to_deserialize_any, serde_if_integer128,
};

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `serde::de::Deserialize` for `DType`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

impl<'de> Deserialize<'de> for DType {
  #[inline]
  fn deserialize<D>(deserializer: D) -> Result<DType, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    struct DTypeVisitor;

    impl<'de> Visitor<'de> for DTypeVisitor {
      type Value = DType;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any valid JSON value")
      }

      #[inline]
      fn visit_bool<E>(self, value: bool) -> Result<DType, E> {
        Ok(DType::Boolean(value))
      }

      #[inline]
      fn visit_i64<E>(self, value: i64) -> Result<DType, E> {
        Ok(DType::Number(value.into()))
      }

      #[inline]
      fn visit_u64<E>(self, value: u64) -> Result<DType, E> {
        Ok(DType::Number(value.into()))
      }

      #[inline]
      fn visit_f64<E>(self, value: f64) -> Result<DType, E> {
        Ok(Number::from_f64(value).map_or(DType::Null, DType::Number))
      }

      #[inline]
      fn visit_str<E>(self, value: &str) -> Result<DType, E>
      where
        E: serde::de::Error,
      {
        self.visit_string(String::from(value))
      }

      #[inline]
      fn visit_string<E>(self, value: String) -> Result<DType, E> {
        Ok(DType::String(value))
      }

      #[inline]
      fn visit_none<E>(self) -> Result<DType, E> {
        Ok(DType::Null)
      }

      #[inline]
      fn visit_some<D>(self, deserializer: D) -> Result<DType, D::Error>
      where
        D: serde::Deserializer<'de>,
      {
        Deserialize::deserialize(deserializer)
      }

      #[inline]
      fn visit_unit<E>(self) -> Result<DType, E> {
        Ok(DType::Null)
      }

      #[inline]
      fn visit_seq<V>(self, mut visitor: V) -> Result<DType, V::Error>
      where
        V: SeqAccess<'de>,
      {
        let mut vec = Vec::new();

        while let Some(elem) = tri!(visitor.next_element()) {
          vec.push(elem);
        }

        Ok(DType::Array(vec))
      }

      fn visit_map<V>(self, mut visitor: V) -> Result<DType, V::Error>
      where
        V: MapAccess<'de>,
      {
        match visitor.next_key_seed(KeyClassifier)? {
          #[cfg(feature = "arbitrary_precision")]
          Some(KeyClass::Number) => {
            let number: NumberFromString = visitor.next_value()?;
            Ok(DType::Number(number.value))
          }
          #[cfg(feature = "raw_value")]
          Some(KeyClass::RawDType) => {
            let value = visitor.next_value_seed(crate::raw::BoxedFromString)?;
            crate::from_str(value.get()).map_err(de::Error::custom)
          }
          Some(KeyClass::Map(first_key)) => {
            let mut values = Map::new();

            values.insert(first_key, tri!(visitor.next_value()));
            while let Some((key, value)) = tri!(visitor.next_entry()) {
              values.insert(key, value);
            }

            Ok(DType::Object(values))
          }
          None => Ok(DType::Object(Map::new())),
        }
      }
    }

    deserializer.deserialize_any(DTypeVisitor)
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `FromStr` for `DType`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

impl FromStr for DType {
  type Err = Error;
  fn from_str(s: &str) -> Result<DType, Error> {
    Ok(crate::json!(s))
  }
}

macro_rules! deserialize_number {
  ($method:ident) => {
    #[cfg(not(feature = "arbitrary_precision"))]
    fn $method<V>(self, visitor: V) -> Result<V::Value, Error>
    where
      V: Visitor<'de>,
    {
      match self {
        DType::Number(n) => n.deserialize_any(visitor),
        _ => Err(self.invalid_type(&visitor)),
      }
    }

    #[cfg(feature = "arbitrary_precision")]
    fn $method<V>(self, visitor: V) -> Result<V::Value, Error>
    where
      V: Visitor<'de>,
    {
      match self {
        DType::Number(n) => n.$method(visitor),
        _ => self.deserialize_any(visitor),
      }
    }
  };
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | Owned visitor.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

fn visit_array<'de, V>(array: Vec<DType>, visitor: V) -> Result<V::Value, Error>
where
  V: Visitor<'de>,
{
  let len = array.len();
  let mut deserializer = SeqDeserializer::new(array);
  let seq = tri!(visitor.visit_seq(&mut deserializer));
  let remaining = deserializer.iter.len();
  if remaining == 0 {
    Ok(seq)
  } else {
    Err(serde::de::Error::invalid_length(
      len,
      &"fewer elements in array",
    ))
  }
}

fn visit_object<'de, V>(
  object: Map<String, DType>,
  visitor: V,
) -> Result<V::Value, Error>
where
  V: Visitor<'de>,
{
  let len = object.len();
  let mut deserializer = MapDeserializer::new(object);
  let map = tri!(visitor.visit_map(&mut deserializer));
  let remaining = deserializer.iter.len();
  if remaining == 0 {
    Ok(map)
  } else {
    Err(serde::de::Error::invalid_length(
      len,
      &"fewer elements in map",
    ))
  }
}

// TODO: Implement this function for `visit_datetime`.
fn visit_datetime<'de, V>(
  _datetime: DateTime,
  _visitor: V,
) -> Result<V::Value, Error>
where
  V: Visitor<'de>,
{
  todo!()
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `serde::Deserializer` for `DType`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

impl<'de> serde::Deserializer<'de> for DType {
  type Error = Error;

  #[inline]
  fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::Null => visitor.visit_unit(),
      DType::Boolean(v) => visitor.visit_bool(v),
      DType::Number(n) => n.deserialize_any(visitor),
      DType::String(v) => visitor.visit_string(v),
      DType::Array(v) => visit_array(v, visitor),
      DType::Object(v) => visit_object(v, visitor),
      DType::DateTime(d) => visit_datetime(d, visitor),
    }
  }

  deserialize_number!(deserialize_i8);
  deserialize_number!(deserialize_i16);
  deserialize_number!(deserialize_i32);
  deserialize_number!(deserialize_i64);
  deserialize_number!(deserialize_u8);
  deserialize_number!(deserialize_u16);
  deserialize_number!(deserialize_u32);
  deserialize_number!(deserialize_u64);
  deserialize_number!(deserialize_f32);
  deserialize_number!(deserialize_f64);

  serde_if_integer128! {
      deserialize_number!(deserialize_i128);
      deserialize_number!(deserialize_u128);
  }

  #[inline]
  fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::Null => visitor.visit_none(),
      _ => visitor.visit_some(self),
    }
  }

  #[inline]
  fn deserialize_enum<V>(
    self,
    _name: &str,
    _variants: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    let (variant, value) = match self {
      DType::Object(value) => {
        let mut iter = value.into_iter();
        let (variant, value) = match iter.next() {
          Some(v) => v,
          None => {
            return Err(serde::de::Error::invalid_value(
              Unexpected::Map,
              &"map with a single key",
            ));
          }
        };
        // enums are encoded in json as maps with a single key:value pair
        if iter.next().is_some() {
          return Err(serde::de::Error::invalid_value(
            Unexpected::Map,
            &"map with a single key",
          ));
        }
        (variant, Some(value))
      }
      DType::String(variant) => (variant, None),
      other => {
        return Err(serde::de::Error::invalid_type(
          other.unexpected(),
          &"string or map",
        ));
      }
    };

    visitor.visit_enum(EnumDeserializer { variant, value })
  }

  #[inline]
  fn deserialize_newtype_struct<V>(
    self,
    name: &'static str,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    #[cfg(feature = "raw_value")]
    {
      if name == crate::raw::TOKEN {
        return visitor.visit_map(crate::raw::OwnedRawDeserializer {
          raw_value: Some(self.to_string()),
        });
      }
    }

    let _ = name;
    visitor.visit_newtype_struct(self)
  }

  fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::Boolean(v) => visitor.visit_bool(v),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_string(visitor)
  }

  fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_string(visitor)
  }

  fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::String(v) => visitor.visit_string(v),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_byte_buf(visitor)
  }

  fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::String(v) => visitor.visit_string(v),
      DType::Array(v) => visit_array(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::Null => visitor.visit_unit(),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_unit_struct<V>(
    self,
    _name: &'static str,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_unit(visitor)
  }

  fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::Array(v) => visit_array(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_tuple<V>(
    self,
    _len: usize,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_seq(visitor)
  }

  fn deserialize_tuple_struct<V>(
    self,
    _name: &'static str,
    _len: usize,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_seq(visitor)
  }

  fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::Object(v) => visit_object(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_struct<V>(
    self,
    _name: &'static str,
    _fields: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self {
      DType::Array(v) => visit_array(v, visitor),
      DType::Object(v) => visit_object(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_string(visitor)
  }

  fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    drop(self);
    visitor.visit_unit()
  }

  fn is_human_readable(&self) -> bool {
    true
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `EnumDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct EnumDeserializer {
  variant: String,
  value: Option<DType>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
  type Error = Error;
  type Variant = VariantDeserializer;

  fn variant_seed<V>(
    self,
    seed: V,
  ) -> Result<(V::Value, VariantDeserializer), Error>
  where
    V: DeserializeSeed<'de>,
  {
    let variant = self.variant.into_deserializer();
    let visitor = VariantDeserializer { value: self.value };
    seed.deserialize(variant).map(|v| (v, visitor))
  }
}

impl<'de> IntoDeserializer<'de, Error> for DType {
  type Deserializer = Self;

  fn into_deserializer(self) -> Self::Deserializer {
    self
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `VariantDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct VariantDeserializer {
  value: Option<DType>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer {
  type Error = Error;

  fn unit_variant(self) -> Result<(), Error> {
    match self.value {
      Some(value) => Deserialize::deserialize(value),
      None => Ok(()),
    }
  }

  fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.value {
      Some(value) => seed.deserialize(value),
      None => Err(serde::de::Error::invalid_type(
        Unexpected::UnitVariant,
        &"newtype variant",
      )),
    }
  }

  fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self.value {
      Some(DType::Array(v)) => {
        if v.is_empty() {
          visitor.visit_unit()
        } else {
          visit_array(v, visitor)
        }
      }
      Some(other) => Err(serde::de::Error::invalid_type(
        other.unexpected(),
        &"tuple variant",
      )),
      None => Err(serde::de::Error::invalid_type(
        Unexpected::UnitVariant,
        &"tuple variant",
      )),
    }
  }

  fn struct_variant<V>(
    self,
    _fields: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self.value {
      Some(DType::Object(v)) => visit_object(v, visitor),
      Some(other) => Err(serde::de::Error::invalid_type(
        other.unexpected(),
        &"struct variant",
      )),
      None => Err(serde::de::Error::invalid_type(
        Unexpected::UnitVariant,
        &"struct variant",
      )),
    }
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `SeqDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct SeqDeserializer {
  iter: std::vec::IntoIter<DType>,
}

impl SeqDeserializer {
  fn new(vec: Vec<DType>) -> Self {
    SeqDeserializer {
      iter: vec.into_iter(),
    }
  }
}

impl<'de> SeqAccess<'de> for SeqDeserializer {
  type Error = Error;

  fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.iter.next() {
      Some(value) => seed.deserialize(value).map(Some),
      None => Ok(None),
    }
  }

  fn size_hint(&self) -> Option<usize> {
    match self.iter.size_hint() {
      (lower, Some(upper)) if lower == upper => Some(upper),
      _ => None,
    }
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `MapDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct MapDeserializer {
  iter: <Map<String, DType> as IntoIterator>::IntoIter,
  value: Option<DType>,
}

impl MapDeserializer {
  fn new(map: Map<String, DType>) -> Self {
    MapDeserializer {
      iter: map.into_iter(),
      value: None,
    }
  }
}

impl<'de> MapAccess<'de> for MapDeserializer {
  type Error = Error;

  fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.iter.next() {
      Some((key, value)) => {
        self.value = Some(value);
        let key_de = MapKeyDeserializer {
          key: Cow::Owned(key),
        };
        seed.deserialize(key_de).map(Some)
      }
      None => Ok(None),
    }
  }

  fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.value.take() {
      Some(value) => seed.deserialize(value),
      None => Err(serde::de::Error::custom("value is missing")),
    }
  }

  fn size_hint(&self) -> Option<usize> {
    match self.iter.size_hint() {
      (lower, Some(upper)) if lower == upper => Some(upper),
      _ => None,
    }
  }
}

macro_rules! deserialize_value_ref_number {
  ($method:ident) => {
    #[cfg(not(feature = "arbitrary_precision"))]
    fn $method<V>(self, visitor: V) -> Result<V::Value, Error>
    where
      V: Visitor<'de>,
    {
      match *self {
        DType::Number(ref n) => n.deserialize_any(visitor),
        _ => Err(self.invalid_type(&visitor)),
      }
    }

    #[cfg(feature = "arbitrary_precision")]
    fn $method<V>(self, visitor: V) -> Result<V::Value, Error>
    where
      V: Visitor<'de>,
    {
      match *self {
        DType::Number(ref n) => n.$method(visitor),
        _ => self.deserialize_any(visitor),
      }
    }
  };
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `visit_*_ref`
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

fn visit_array_ref<'de, V>(
  array: &'de [DType],
  visitor: V,
) -> Result<V::Value, Error>
where
  V: Visitor<'de>,
{
  let len = array.len();
  let mut deserializer = SeqRefDeserializer::new(array);
  let seq = tri!(visitor.visit_seq(&mut deserializer));
  let remaining = deserializer.iter.len();
  if remaining == 0 {
    Ok(seq)
  } else {
    Err(serde::de::Error::invalid_length(
      len,
      &"fewer elements in array",
    ))
  }
}

fn visit_object_ref<'de, V>(
  object: &'de Map<String, DType>,
  visitor: V,
) -> Result<V::Value, Error>
where
  V: Visitor<'de>,
{
  let len = object.len();
  let mut deserializer = MapRefDeserializer::new(object);
  let map = tri!(visitor.visit_map(&mut deserializer));
  let remaining = deserializer.iter.len();
  if remaining == 0 {
    Ok(map)
  } else {
    Err(serde::de::Error::invalid_length(
      len,
      &"fewer elements in map",
    ))
  }
}

// TODO: Implement this function for datetime.
fn visit_datetime_ref<'de, V>(
  _datetime: &'de DateTime,
  _visitor: V,
) -> Result<V::Value, Error>
where
  V: Visitor<'de>,
{
  todo!()
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `serde::Deserializer` for `&DType`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

impl<'de> serde::Deserializer<'de> for &'de DType {
  type Error = Error;

  fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::Null => visitor.visit_unit(),
      DType::Boolean(v) => visitor.visit_bool(v),
      DType::Number(ref n) => n.deserialize_any(visitor),
      DType::String(ref v) => visitor.visit_borrowed_str(v),
      DType::Array(ref v) => visit_array_ref(v, visitor),
      DType::Object(ref v) => visit_object_ref(v, visitor),
      DType::DateTime(ref d) => visit_datetime_ref(d, visitor),
    }
  }

  deserialize_value_ref_number!(deserialize_i8);
  deserialize_value_ref_number!(deserialize_i16);
  deserialize_value_ref_number!(deserialize_i32);
  deserialize_value_ref_number!(deserialize_i64);
  deserialize_value_ref_number!(deserialize_u8);
  deserialize_value_ref_number!(deserialize_u16);
  deserialize_value_ref_number!(deserialize_u32);
  deserialize_value_ref_number!(deserialize_u64);
  deserialize_value_ref_number!(deserialize_f32);
  deserialize_value_ref_number!(deserialize_f64);

  serde_if_integer128! {
      deserialize_number!(deserialize_i128);
      deserialize_number!(deserialize_u128);
  }

  fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::Null => visitor.visit_none(),
      _ => visitor.visit_some(self),
    }
  }

  fn deserialize_enum<V>(
    self,
    _name: &str,
    _variants: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    let (variant, value) = match *self {
      DType::Object(ref value) => {
        let mut iter = value.into_iter();
        let (variant, value) = match iter.next() {
          Some(v) => v,
          None => {
            return Err(serde::de::Error::invalid_value(
              Unexpected::Map,
              &"map with a single key",
            ));
          }
        };
        // enums are encoded in json as maps with a single key:value pair
        if iter.next().is_some() {
          return Err(serde::de::Error::invalid_value(
            Unexpected::Map,
            &"map with a single key",
          ));
        }
        (variant, Some(value))
      }
      DType::String(ref variant) => (variant, None),
      ref other => {
        return Err(serde::de::Error::invalid_type(
          other.unexpected(),
          &"string or map",
        ));
      }
    };

    visitor.visit_enum(EnumRefDeserializer { variant, value })
  }

  #[inline]
  fn deserialize_newtype_struct<V>(
    self,
    name: &'static str,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    #[cfg(feature = "raw_value")]
    {
      if name == crate::raw::TOKEN {
        return visitor.visit_map(crate::raw::OwnedRawDeserializer {
          raw_value: Some(self.to_string()),
        });
      }
    }

    let _ = name;
    visitor.visit_newtype_struct(self)
  }

  fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::Boolean(v) => visitor.visit_bool(v),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_str(visitor)
  }

  fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::String(ref v) => visitor.visit_borrowed_str(v),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_str(visitor)
  }

  fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::String(ref v) => visitor.visit_borrowed_str(v),
      DType::Array(ref v) => visit_array_ref(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_bytes(visitor)
  }

  fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::Null => visitor.visit_unit(),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_unit_struct<V>(
    self,
    _name: &'static str,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_unit(visitor)
  }

  fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::Array(ref v) => visit_array_ref(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_tuple<V>(
    self,
    _len: usize,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_seq(visitor)
  }

  fn deserialize_tuple_struct<V>(
    self,
    _name: &'static str,
    _len: usize,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_seq(visitor)
  }

  fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::Object(ref v) => visit_object_ref(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_struct<V>(
    self,
    _name: &'static str,
    _fields: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match *self {
      DType::Array(ref v) => visit_array_ref(v, visitor),
      DType::Object(ref v) => visit_object_ref(v, visitor),
      _ => Err(self.invalid_type(&visitor)),
    }
  }

  fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self.deserialize_str(visitor)
  }

  fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    visitor.visit_unit()
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `EnumRefDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct EnumRefDeserializer<'de> {
  variant: &'de str,
  value: Option<&'de DType>,
}

impl<'de> EnumAccess<'de> for EnumRefDeserializer<'de> {
  type Error = Error;
  type Variant = VariantRefDeserializer<'de>;

  fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Error>
  where
    V: DeserializeSeed<'de>,
  {
    let variant = self.variant.into_deserializer();
    let visitor = VariantRefDeserializer { value: self.value };
    seed.deserialize(variant).map(|v| (v, visitor))
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `VariantRefDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct VariantRefDeserializer<'de> {
  value: Option<&'de DType>,
}

impl<'de> VariantAccess<'de> for VariantRefDeserializer<'de> {
  type Error = Error;

  fn unit_variant(self) -> Result<(), Error> {
    match self.value {
      Some(value) => Deserialize::deserialize(value),
      None => Ok(()),
    }
  }

  fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.value {
      Some(value) => seed.deserialize(value),
      None => Err(serde::de::Error::invalid_type(
        Unexpected::UnitVariant,
        &"newtype variant",
      )),
    }
  }

  fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self.value {
      Some(&DType::Array(ref v)) => {
        if v.is_empty() {
          visitor.visit_unit()
        } else {
          visit_array_ref(v, visitor)
        }
      }
      Some(other) => Err(serde::de::Error::invalid_type(
        other.unexpected(),
        &"tuple variant",
      )),
      None => Err(serde::de::Error::invalid_type(
        Unexpected::UnitVariant,
        &"tuple variant",
      )),
    }
  }

  fn struct_variant<V>(
    self,
    _fields: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    match self.value {
      Some(&DType::Object(ref v)) => visit_object_ref(v, visitor),
      Some(other) => Err(serde::de::Error::invalid_type(
        other.unexpected(),
        &"struct variant",
      )),
      None => Err(serde::de::Error::invalid_type(
        Unexpected::UnitVariant,
        &"struct variant",
      )),
    }
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `SeqRefDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct SeqRefDeserializer<'de> {
  iter: std::slice::Iter<'de, DType>,
}

impl<'de> SeqRefDeserializer<'de> {
  fn new(slice: &'de [DType]) -> Self {
    SeqRefDeserializer { iter: slice.iter() }
  }
}

impl<'de> SeqAccess<'de> for SeqRefDeserializer<'de> {
  type Error = Error;

  fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.iter.next() {
      Some(value) => seed.deserialize(value).map(Some),
      None => Ok(None),
    }
  }

  fn size_hint(&self) -> Option<usize> {
    match self.iter.size_hint() {
      (lower, Some(upper)) if lower == upper => Some(upper),
      _ => None,
    }
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `MapRefDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct MapRefDeserializer<'de> {
  iter: <&'de Map<String, DType> as IntoIterator>::IntoIter,
  value: Option<&'de DType>,
}

impl<'de> MapRefDeserializer<'de> {
  fn new(map: &'de Map<String, DType>) -> Self {
    MapRefDeserializer {
      iter: map.into_iter(),
      value: None,
    }
  }
}

impl<'de> MapAccess<'de> for MapRefDeserializer<'de> {
  type Error = Error;

  fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.iter.next() {
      Some((key, value)) => {
        self.value = Some(value);
        let key_de = MapKeyDeserializer {
          key: Cow::Borrowed(&**key),
        };
        seed.deserialize(key_de).map(Some)
      }
      None => Ok(None),
    }
  }

  fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Error>
  where
    T: DeserializeSeed<'de>,
  {
    match self.value.take() {
      Some(value) => seed.deserialize(value),
      None => Err(serde::de::Error::custom("value is missing")),
    }
  }

  fn size_hint(&self) -> Option<usize> {
    match self.iter.size_hint() {
      (lower, Some(upper)) if lower == upper => Some(upper),
      _ => None,
    }
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `MapKeyDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct MapKeyDeserializer<'de> {
  key: Cow<'de, str>,
}

macro_rules! deserialize_integer_key {
  ($method:ident => $visit:ident) => {
    fn $method<V>(self, visitor: V) -> Result<V::Value, Error>
    where
      V: Visitor<'de>,
    {
      match (self.key.parse(), self.key) {
        (Ok(integer), _) => visitor.$visit(integer),
        (Err(_), Cow::Borrowed(s)) => visitor.visit_borrowed_str(s),
        (Err(_), Cow::Owned(s)) => visitor.visit_string(s),
      }
    }
  };
}

impl<'de> serde::Deserializer<'de> for MapKeyDeserializer<'de> {
  type Error = Error;

  fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    BorrowedCowStrDeserializer::new(self.key).deserialize_any(visitor)
  }

  deserialize_integer_key!(deserialize_i8 => visit_i8);
  deserialize_integer_key!(deserialize_i16 => visit_i16);
  deserialize_integer_key!(deserialize_i32 => visit_i32);
  deserialize_integer_key!(deserialize_i64 => visit_i64);
  deserialize_integer_key!(deserialize_u8 => visit_u8);
  deserialize_integer_key!(deserialize_u16 => visit_u16);
  deserialize_integer_key!(deserialize_u32 => visit_u32);
  deserialize_integer_key!(deserialize_u64 => visit_u64);

  serde_if_integer128! {
      deserialize_integer_key!(deserialize_i128 => visit_i128);
      deserialize_integer_key!(deserialize_u128 => visit_u128);
  }

  #[inline]
  fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    // Map keys cannot be null.
    visitor.visit_some(self)
  }

  #[inline]
  fn deserialize_newtype_struct<V>(
    self,
    _name: &'static str,
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    visitor.visit_newtype_struct(self)
  }

  fn deserialize_enum<V>(
    self,
    name: &'static str,
    variants: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: Visitor<'de>,
  {
    self
      .key
      .into_deserializer()
      .deserialize_enum(name, variants, visitor)
  }

  forward_to_deserialize_any! {
      bool f32 f64 char str string bytes byte_buf unit unit_struct seq tuple
      tuple_struct map struct identifier ignored_any
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `KeyClassifier`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct KeyClassifier;

enum KeyClass {
  Map(String),
  #[cfg(feature = "arbitrary_precision")]
  Number,
  #[cfg(feature = "raw_value")]
  RawDType,
}

impl<'de> DeserializeSeed<'de> for KeyClassifier {
  type Value = KeyClass;

  fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    deserializer.deserialize_str(self)
  }
}

impl<'de> Visitor<'de> for KeyClassifier {
  type Value = KeyClass;

  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    formatter.write_str("a string key")
  }

  fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
  where
    E: de::Error,
  {
    match s {
      #[cfg(feature = "arbitrary_precision")]
      crate::number::TOKEN => Ok(KeyClass::Number),
      #[cfg(feature = "raw_value")]
      crate::raw::TOKEN => Ok(KeyClass::RawDType),
      _ => Ok(KeyClass::Map(s.to_owned())),
    }
  }

  fn visit_string<E>(self, s: String) -> Result<Self::Value, E>
  where
    E: de::Error,
  {
    match s.as_str() {
      #[cfg(feature = "arbitrary_precision")]
      crate::number::TOKEN => Ok(KeyClass::Number),
      #[cfg(feature = "raw_value")]
      crate::raw::TOKEN => Ok(KeyClass::RawDType),
      _ => Ok(KeyClass::Map(s)),
    }
  }
}

impl DType {
  #[cold]
  fn invalid_type<E>(&self, exp: &dyn Expected) -> E
  where
    E: serde::de::Error,
  {
    serde::de::Error::invalid_type(self.unexpected(), exp)
  }

  #[cold]
  fn unexpected(&self) -> Unexpected {
    match *self {
      DType::Null => Unexpected::Unit,
      DType::Boolean(b) => Unexpected::Bool(b),
      DType::Number(ref n) => n.unexpected(),
      DType::String(ref s) => Unexpected::Str(s),
      DType::Array(_) => Unexpected::Seq,
      DType::Object(_) => Unexpected::Map,
      DType::DateTime(_) => Unexpected::Other("datetime"),
    }
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `BorrowedCowStrDeserializer`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct BorrowedCowStrDeserializer<'de> {
  value: Cow<'de, str>,
}

impl<'de> BorrowedCowStrDeserializer<'de> {
  fn new(value: Cow<'de, str>) -> Self {
    BorrowedCowStrDeserializer { value }
  }
}

impl<'de> de::Deserializer<'de> for BorrowedCowStrDeserializer<'de> {
  type Error = Error;

  fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
  where
    V: de::Visitor<'de>,
  {
    match self.value {
      Cow::Borrowed(string) => visitor.visit_borrowed_str(string),
      Cow::Owned(string) => visitor.visit_string(string),
    }
  }

  fn deserialize_enum<V>(
    self,
    _name: &str,
    _variants: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: de::Visitor<'de>,
  {
    visitor.visit_enum(self)
  }

  forward_to_deserialize_any! {
      bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
      bytes byte_buf option unit unit_struct newtype_struct seq tuple
      tuple_struct map struct identifier ignored_any
  }
}

impl<'de> de::EnumAccess<'de> for BorrowedCowStrDeserializer<'de> {
  type Error = Error;
  type Variant = UnitOnly;

  fn variant_seed<T>(self, seed: T) -> Result<(T::Value, Self::Variant), Error>
  where
    T: de::DeserializeSeed<'de>,
  {
    let value = seed.deserialize(self)?;
    Ok((value, UnitOnly))
  }
}

/*
 * +----------------------------------------------------------------------+
 * | +------------------------------------------------------------------+ |
 * | | `UnitOnly`.
 * | +------------------------------------------------------------------+ |
 * +----------------------------------------------------------------------+
*/

struct UnitOnly;

impl<'de> de::VariantAccess<'de> for UnitOnly {
  type Error = Error;

  fn unit_variant(self) -> Result<(), Error> {
    Ok(())
  }

  fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value, Error>
  where
    T: de::DeserializeSeed<'de>,
  {
    Err(de::Error::invalid_type(
      Unexpected::UnitVariant,
      &"newtype variant",
    ))
  }

  fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Error>
  where
    V: de::Visitor<'de>,
  {
    Err(de::Error::invalid_type(
      Unexpected::UnitVariant,
      &"tuple variant",
    ))
  }

  fn struct_variant<V>(
    self,
    _fields: &'static [&'static str],
    _visitor: V,
  ) -> Result<V::Value, Error>
  where
    V: de::Visitor<'de>,
  {
    Err(de::Error::invalid_type(
      Unexpected::UnitVariant,
      &"struct variant",
    ))
  }
}
