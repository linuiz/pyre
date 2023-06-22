#![no_std]
#![feature(
    error_in_core   // #103765 <https://github.com/rust-lang/rust/issues/103765>
)]

#[macro_export]
macro_rules! errorgen {
    (
        $(#[$outer:meta])*
        $vis:vis enum $Error:ident {
            $(
                $(#[$inner:meta])*
                $Variant:ident$({
                    $($VarName:ident: $VarTy:ty),+
                })? => $SourceExpr:expr
            ),+ $(,)?
        }
    ) => {
        $(#[$outer])*
        $vis enum $Error {
            $(
                $(#[$inner])*
                $Variant$({ $($VarName: $VarTy),* })*
            ),*
        }

        impl core::fmt::Display for $Error {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                <Self as core::fmt::Debug>::fmt(self, f)
            }
        }

        impl core::error::Error for $Error {
            #[allow(unused_variables)]
            fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
                match self {
                    $(Self::$Variant$({ $($VarName),* })* => { $SourceExpr })*
                }
            }
        }

        $vis type Result<T> = core::result::Result<T, $Error>;
    };
}
