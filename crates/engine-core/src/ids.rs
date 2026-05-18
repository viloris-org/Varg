//! Opaque runtime identifiers.

macro_rules! id_type {
    ($name:ident) => {
        #[doc = concat!("Opaque ", stringify!($name), " value.")]
        #[derive(
            Clone,
            Copy,
            Debug,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            serde::Deserialize,
            serde::Serialize,
        )]
        pub struct $name(u128);

        impl $name {
            /// Creates an ID from raw bits.
            pub const fn from_u128(value: u128) -> Self {
                Self(value)
            }

            /// Returns raw ID bits for serialization boundaries.
            pub const fn as_u128(self) -> u128 {
                self.0
            }
        }
    };
}

id_type!(EntityId);
id_type!(AssetId);
id_type!(ResourceId);
