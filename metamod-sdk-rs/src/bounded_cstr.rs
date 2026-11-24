//! Byte-length validation and ownership for semantic C-string wrappers.

use std::{
    borrow::Borrow,
    ffi::{CStr, CString},
    ops::Deref,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TooLong;

#[repr(transparent)]
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct BoundedCStr<const N: usize>(CStr);

// Keep CString ownership distinct from the borrowed, dynamically sized view.
#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct BoundedCString<const N: usize>(CString);

impl<const N: usize> BoundedCString<N> {
    pub(crate) fn new(value: CString) -> Result<Self, TooLong> {
        BoundedCStr::<N>::new(&value)?;
        Ok(Self(value))
    }

    pub(crate) fn new_trunc(value: &CStr) -> Self {
        let bytes = value.to_bytes();
        let value =
            CString::new(&bytes[..bytes.len().min(N)]).expect("a C string payload has no NUL");
        Self::new(value).expect("truncated payload fits")
    }

    pub(crate) fn as_c_str(&self) -> &CStr {
        self.0.as_c_str()
    }
}

impl<const N: usize> Deref for BoundedCString<N> {
    type Target = BoundedCStr<N>;
    fn deref(&self) -> &Self::Target {
        BoundedCStr::new(&self.0).expect("owned string retains its validated length")
    }
}
impl<const N: usize> AsRef<BoundedCStr<N>> for BoundedCString<N> {
    fn as_ref(&self) -> &BoundedCStr<N> {
        self
    }
}
impl<const N: usize> Borrow<BoundedCStr<N>> for BoundedCString<N> {
    fn borrow(&self) -> &BoundedCStr<N> {
        self
    }
}
impl<const N: usize> AsRef<CStr> for BoundedCString<N> {
    fn as_ref(&self) -> &CStr {
        self.as_c_str()
    }
}
impl<const N: usize> Borrow<CStr> for BoundedCString<N> {
    fn borrow(&self) -> &CStr {
        self.as_c_str()
    }
}
impl<const N: usize> TryFrom<CString> for BoundedCString<N> {
    type Error = TooLong;
    fn try_from(value: CString) -> Result<Self, TooLong> {
        Self::new(value)
    }
}
impl<const N: usize> TryFrom<&CStr> for BoundedCString<N> {
    type Error = TooLong;
    fn try_from(value: &CStr) -> Result<Self, TooLong> {
        BoundedCStr::<N>::new(value).map(ToOwned::to_owned)
    }
}
impl<const N: usize> TryFrom<Box<CStr>> for BoundedCString<N> {
    type Error = TooLong;
    fn try_from(value: Box<CStr>) -> Result<Self, TooLong> {
        Self::new(value.into_c_string())
    }
}
impl<const N: usize> From<&BoundedCStr<N>> for BoundedCString<N> {
    fn from(value: &BoundedCStr<N>) -> Self {
        value.to_owned()
    }
}
impl<const N: usize> From<BoundedCString<N>> for CString {
    fn from(value: BoundedCString<N>) -> Self {
        value.0
    }
}
impl<const N: usize> From<BoundedCString<N>> for Box<CStr> {
    fn from(value: BoundedCString<N>) -> Self {
        value.0.into_boxed_c_str()
    }
}
impl<const N: usize> From<BoundedCString<N>> for Box<BoundedCStr<N>> {
    fn from(value: BoundedCString<N>) -> Self {
        BoundedCStr::try_box(value.0.into_boxed_c_str()).expect("validated length")
    }
}
impl<const N: usize> From<Box<BoundedCStr<N>>> for BoundedCString<N> {
    fn from(value: Box<BoundedCStr<N>>) -> Self {
        Self(Box::<CStr>::from(value).into_c_string())
    }
}

impl<const N: usize> BoundedCStr<N> {
    pub(crate) const fn new(value: &CStr) -> Result<&Self, TooLong> {
        if value.to_bytes().len() > N {
            return Err(TooLong);
        }
        // SAFETY: Self is transparent over CStr. The validated length, pointer
        // metadata, allocation, and borrow lifetime are preserved.
        Ok(unsafe { &*(value as *const CStr as *const Self) })
    }

    pub(crate) const fn as_c_str(&self) -> &CStr {
        &self.0
    }

    // A short input remains borrowed; the caller's buffer is used only on overflow.
    pub(crate) fn borrow_trunc<'a>(value: &'a CStr, buffer: &'a mut [u8]) -> &'a Self {
        if let Ok(value) = Self::new(value) {
            return value;
        }
        buffer[..N].copy_from_slice(&value.to_bytes()[..N]);
        buffer[N] = 0;
        let value =
            CStr::from_bytes_with_nul(&buffer[..=N]).expect("a C string payload has no NUL");
        Self::new(value).expect("truncated payload fits")
    }

    fn try_box(value: Box<CStr>) -> Result<Box<Self>, TooLong> {
        Self::new(&value)?;
        // SAFETY: Self and CStr have identical layout and pointer metadata.
        // Length was validated; ownership of the allocation is transferred once.
        Ok(unsafe { Box::from_raw(Box::into_raw(value) as *mut Self) })
    }
}

impl<const N: usize> Deref for BoundedCStr<N> {
    type Target = CStr;
    fn deref(&self) -> &CStr {
        self.as_c_str()
    }
}
impl<const N: usize> AsRef<CStr> for BoundedCStr<N> {
    fn as_ref(&self) -> &CStr {
        self.as_c_str()
    }
}
impl<const N: usize> Borrow<CStr> for BoundedCStr<N> {
    fn borrow(&self) -> &CStr {
        self.as_c_str()
    }
}
impl<const N: usize> AsRef<CStr> for Box<BoundedCStr<N>> {
    fn as_ref(&self) -> &CStr {
        self.as_c_str()
    }
}
impl<const N: usize> Borrow<CStr> for Box<BoundedCStr<N>> {
    fn borrow(&self) -> &CStr {
        self.as_c_str()
    }
}
impl<const N: usize> ToOwned for BoundedCStr<N> {
    type Owned = BoundedCString<N>;
    fn to_owned(&self) -> Self::Owned {
        BoundedCString(self.0.to_owned())
    }
}
impl<const N: usize> Clone for Box<BoundedCStr<N>> {
    fn clone(&self) -> Self {
        (**self).to_owned().into()
    }
}
impl<const N: usize> Default for Box<BoundedCStr<N>> {
    fn default() -> Self {
        BoundedCStr::new(c"")
            .expect("empty payload fits")
            .to_owned()
            .into()
    }
}
impl<const N: usize> From<&BoundedCStr<N>> for Box<BoundedCStr<N>> {
    fn from(value: &BoundedCStr<N>) -> Self {
        value.to_owned().into()
    }
}
impl<'a, const N: usize> TryFrom<&'a CStr> for &'a BoundedCStr<N> {
    type Error = TooLong;
    fn try_from(value: &'a CStr) -> Result<Self, TooLong> {
        BoundedCStr::new(value)
    }
}
impl<const N: usize> TryFrom<&CStr> for Box<BoundedCStr<N>> {
    type Error = TooLong;
    fn try_from(value: &CStr) -> Result<Self, TooLong> {
        BoundedCStr::new(value).map(|value| value.to_owned().into())
    }
}
impl<const N: usize> TryFrom<CString> for Box<BoundedCStr<N>> {
    type Error = TooLong;
    fn try_from(value: CString) -> Result<Self, TooLong> {
        BoundedCStr::try_box(value.into_boxed_c_str())
    }
}
impl<const N: usize> TryFrom<Box<CStr>> for Box<BoundedCStr<N>> {
    type Error = TooLong;
    fn try_from(value: Box<CStr>) -> Result<Self, TooLong> {
        BoundedCStr::try_box(value)
    }
}
impl<const N: usize> From<Box<BoundedCStr<N>>> for Box<CStr> {
    fn from(value: Box<BoundedCStr<N>>) -> Self {
        // SAFETY: The transparent wrapper preserves allocation layout and
        // pointer metadata. No references survive this ownership transfer.
        unsafe { Box::from_raw(Box::into_raw(value) as *mut CStr) }
    }
}
impl<const N: usize> From<Box<BoundedCStr<N>>> for CString {
    fn from(value: Box<BoundedCStr<N>>) -> Self {
        Box::<CStr>::from(value).into_c_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn borrowed_and_owned_preserve_bounds_and_allocations() {
        const EMPTY: &BoundedCStr<0> = match BoundedCStr::new(c"") {
            Ok(value) => value,
            Err(_) => panic!("empty fits"),
        };
        assert_eq!(EMPTY.to_bytes_with_nul(), b"\0");
        assert_eq!(BoundedCStr::<0>::new(c"x"), Err(TooLong));
        for len in [0, 3] {
            let input = CString::new(vec![0xff; len]).unwrap();
            let borrowed = BoundedCStr::<3>::new(&input).unwrap();
            assert_eq!(borrowed.as_ptr(), input.as_ptr());
            assert_eq!(borrowed.to_bytes_with_nul(), input.as_bytes_with_nul());
            let copied: Box<BoundedCStr<3>> = input.as_c_str().try_into().unwrap();
            assert_eq!(&*copied, borrowed);
            assert_ne!(copied.as_ptr(), input.as_ptr());
            let pointer = input.as_ptr();
            let owned: Box<BoundedCStr<3>> = input.try_into().unwrap();
            assert_eq!(owned.as_ptr(), pointer);
            assert_eq!(owned.clone(), owned);
            let boxed: Box<CStr> = owned.into();
            assert_eq!(boxed.as_ptr(), pointer);
            let bounded: Box<BoundedCStr<3>> = boxed.try_into().unwrap();
            assert_eq!(bounded.as_ptr(), pointer);
            let string: CString = bounded.into();
            assert_eq!(string.as_ptr(), pointer);
        }
        let long = c"four";
        assert_eq!(BoundedCStr::<3>::new(long), Err(TooLong));
        assert_eq!(Box::<BoundedCStr<3>>::try_from(long), Err(TooLong));
        assert_eq!(
            Box::<BoundedCStr<3>>::try_from(long.to_owned()),
            Err(TooLong)
        );
        assert_eq!(
            Box::<BoundedCStr<3>>::try_from(Box::<CStr>::from(long)),
            Err(TooLong)
        );
        assert!(CStr::from_bytes_with_nul(b"x").is_err());
        let interior_nul = [b'x', 0, b'y', 0];
        assert!(CStr::from_bytes_with_nul(&interior_nul).is_err());
        assert!(CString::new(b"x\0y".to_vec()).is_err());
    }

    #[test]
    fn owned_strings_round_trip_through_boxed_views_without_copying() {
        let empty = BoundedCString::<0>::default();
        assert_eq!(empty.to_bytes_with_nul(), b"\0");
        assert_eq!(BoundedCString::<0>::new(c"x".to_owned()), Err(TooLong));
        for len in [0, 3] {
            let input = CString::new(vec![0xff; len]).unwrap();
            let pointer = input.as_ptr();
            let owned = BoundedCString::<3>::new(input).unwrap();
            let borrowed: &BoundedCStr<3> = &owned;
            assert_eq!(borrowed.as_ptr(), pointer);
            assert_eq!(owned.as_c_str().as_ptr(), pointer);
            let copied = BoundedCString::from(borrowed);
            assert_eq!(copied, owned);
            assert_ne!(copied.as_ptr(), pointer);
            let boxed: Box<BoundedCStr<3>> = owned.into();
            assert_eq!(boxed.as_ptr(), pointer);
            let owned: BoundedCString<3> = boxed.into();
            assert_eq!(owned.as_ptr(), pointer);
            let cloned = owned.clone();
            assert_eq!(cloned, owned);
            assert_ne!(cloned.as_ptr(), pointer);
            let values = HashSet::from([cloned]);
            assert!(values.contains(BoundedCStr::<3>::new(owned.as_c_str()).unwrap()));
            assert!(values.contains(owned.as_c_str()));
            let string: CString = owned.into();
            assert_eq!(string.as_ptr(), pointer);
            let owned: BoundedCString<3> = string.try_into().unwrap();
            assert_eq!(owned.as_ptr(), pointer);
            let boxed: Box<CStr> = owned.into();
            assert_eq!(boxed.as_ptr(), pointer);
            let owned: BoundedCString<3> = boxed.try_into().unwrap();
            assert_eq!(owned.as_ptr(), pointer);
            let copied: BoundedCString<3> = owned.as_c_str().try_into().unwrap();
            assert_eq!(copied, owned);
            assert_ne!(copied.as_ptr(), pointer);
        }
        assert_eq!(BoundedCString::<3>::try_from(c"four"), Err(TooLong));
        assert_eq!(
            BoundedCString::<3>::try_from(c"four".to_owned()),
            Err(TooLong)
        );
        assert_eq!(
            BoundedCString::<3>::try_from(Box::<CStr>::from(c"four")),
            Err(TooLong)
        );
    }

    #[test]
    fn boxed_traits_keep_c_string_equality_and_hashing() {
        let empty = Box::<BoundedCStr<0>>::default();
        assert_eq!(empty.to_bytes_with_nul(), b"\0");
        let value: Box<BoundedCStr<3>> = c"abc".try_into().unwrap();
        let borrowed: &CStr = value.as_ref();
        assert_eq!(borrowed, c"abc");
        let cloned = value.clone();
        assert_ne!(cloned.as_ptr(), value.as_ptr());
        let values = HashSet::from([value]);
        assert!(values.contains(c"abc"));
        assert!(BoundedCStr::<3>::new(c"ab").unwrap() < &*cloned);
    }

    #[test]
    fn explicit_truncation_counts_bytes_and_keeps_a_terminator() {
        let input = CString::new("xé").unwrap();
        let mut storage = [42; 4];
        let truncated = BoundedCStr::<2>::borrow_trunc(&input, &mut storage);
        assert_eq!(truncated.to_bytes_with_nul(), b"x\xc3\0");
        assert_eq!(storage, [b'x', 0xc3, 0, 42]);
        assert_eq!(
            BoundedCString::<2>::new_trunc(&input).to_bytes_with_nul(),
            b"x\xc3\0"
        );
        assert_eq!(
            BoundedCString::<0>::new_trunc(&input).to_bytes_with_nul(),
            b"\0"
        );
        let mut zero = [42; 2];
        assert_eq!(
            BoundedCStr::<0>::borrow_trunc(&input, &mut zero).to_bytes_with_nul(),
            b"\0"
        );
        assert_eq!(zero, [0, 42]);
        let mut unused = [];
        assert_eq!(
            BoundedCStr::<3>::borrow_trunc(&input, &mut unused).as_ptr(),
            input.as_ptr()
        );
    }
}
