
use std::mem::size_of;

use crate::pg_sys::Datum;

pub trait FromOptionalDatum {
    fn from_optional_datum(datum: Option<Datum>) -> Self;
}

pub trait ToOptionalDatum {
    fn to_optional_datum(self) -> Option<Datum>;
}

pub trait FromDatum {
    fn from_datum(datum: Datum) -> Self;
}

pub trait ToDatum {
    fn to_datum(self) -> Datum;
}

impl<T: FromDatum> FromOptionalDatum for T {
    fn from_optional_datum(datum: Option<Datum>) -> Self {
        match datum {
            Some(datum) => Self::from_datum(datum),
            None => panic!("tried to convert NULL into non-nullable value"),
        }
    }
}

impl<T: ToDatum> ToOptionalDatum for T {
    fn to_optional_datum(self) -> Option<Datum> {
        Some(self.to_datum())
    }
}

impl<T: FromDatum> FromOptionalDatum for Option<T> {
    fn from_optional_datum(datum: Option<Datum>) -> Self {
        datum.map(<T as FromDatum>::from_datum)
    }
}

impl<T: ToDatum> ToOptionalDatum for Option<T> {
    fn to_optional_datum(self) -> Option<Datum> {
        self.map(<T as ToDatum>::to_datum)
    }
}

macro_rules! int_datum_convert {
    ($($typ:ty)*) => {
        $(
            // compile time assert that the the size of $typ is not larger
            // than that of datum
            const _: [(); 0 - !{ const ASSERT: bool = size_of::<$typ>() <= size_of::<Datum>(); ASSERT } as usize] = [];
            impl FromDatum for $typ {
                fn from_datum(datum: Datum) -> Self {
                    datum as Self
                }
            }

            impl ToDatum for $typ {
                fn to_datum(self) -> Datum {
                    self as Datum
                }
            }
        )*
    };
}

int_datum_convert!(i8 u8 i16 u16 i32 u32 i64 u64 isize usize);

impl<T> FromDatum for *mut T {
    fn from_datum(datum: Datum) -> Self {
        datum as Self
    }
}

impl<T> ToDatum for *mut T {
    fn to_datum(self) -> Datum {
        self as Datum
    }
}

impl<T> FromDatum for *const T {
    fn from_datum(datum: Datum) -> Self {
        datum as Self
    }
}

impl<T> ToDatum for *const T {
    fn to_datum(self) -> Datum {
        self as Datum
    }
}

impl FromDatum for f32 {
    fn from_datum(datum: Datum) -> Self {
        f32::from_bits(datum as _)
    }
}

impl ToDatum for f32 {
    fn to_datum(self) -> Datum {
        self.to_bits() as _
    }
}

// compile time assert that the the size of f64 is not larger than that of datum
const _: [(); 0 - !{ const ASSERT: bool = size_of::<f64>() <= size_of::<Datum>(); ASSERT } as usize] = [];

impl FromDatum for f64 {
    fn from_datum(datum: Datum) -> Self {
        f64::from_bits(datum as _)
    }
}

impl ToDatum for f64 {
    fn to_datum(self) -> Datum {
        self.to_bits() as _
    }
}
