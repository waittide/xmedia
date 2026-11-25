#[doc(hidden)]
pub mod __private {
    pub use uuid::{Error as UuidError, Uuid};
}

/// Defines one or more strongly-typed UUID v7 newtype identifiers with transparent serde
/// serialization, `Default`, `Display`, `FromStr`, and `From<Uuid>` conversions.
#[macro_export]
macro_rules! define_uuid_id {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(
                Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
                ::serde::Serialize, ::serde::Deserialize
            )]
            #[serde(transparent)]
            pub struct $name(pub $crate::__private::Uuid);

            impl $name {
                pub fn new() -> Self {
                    Self($crate::__private::Uuid::now_v7())
                }
            }

            impl Default for $name {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl ::std::fmt::Display for $name {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    self.0.fmt(f)
                }
            }

            impl ::std::str::FromStr for $name {
                type Err = $crate::__private::UuidError;

                fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                    $crate::__private::Uuid::parse_str(s).map(Self)
                }
            }

            impl ::std::convert::From<$crate::__private::Uuid> for $name {
                fn from(value: $crate::__private::Uuid) -> Self {
                    Self(value)
                }
            }

            impl ::std::convert::From<$name> for $crate::__private::Uuid {
                fn from(value: $name) -> Self {
                    value.0
                }
            }
        )+
    };
}

pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

#[cfg(feature = "server")]
pub use infra::persistence::{
    CoreSchemaMigrationReport, ensure_core_schema, ensure_core_schema_with_report,
    ensure_sqlite_parent_dir, is_core_schema_initialized, register_core_models,
};

#[cfg(test)]
mod tests;
