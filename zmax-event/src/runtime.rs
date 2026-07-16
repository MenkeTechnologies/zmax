//! The event system makes use of global to decouple different systems.
//! However, this can cause problems for the integration test system because
//! it runs multiple zmax applications in parallel. Making the globals
//! thread-local does not work because a applications can/does have multiple
//! runtime threads. Instead this crate implements a similar notion to a thread
//! local but instead of being local to a single thread, the statics are local to
//! a single tokio-runtime. The implementation requires locking so it's not exactly efficient.
//!
//! Therefore this function is only enabled during integration tests and behaves like
//! a normal static otherwise. I would prefer this module to be fully private and to only
//! export the macro but the macro still need to construct these internals so it's marked
//! `doc(hidden)` instead

use std::ops::Deref;

#[cfg(not(feature = "integration_test"))]
pub struct RuntimeLocal<T: 'static> {
    /// inner API used in the macro, not part of public API
    #[doc(hidden)]
    pub __data: T,
}

#[cfg(not(feature = "integration_test"))]
impl<T> Deref for RuntimeLocal<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.__data
    }
}

#[cfg(not(feature = "integration_test"))]
#[macro_export]
macro_rules! runtime_local {
    ($($(#[$attr:meta])* $vis: vis static $name:ident: $ty: ty = $init: expr;)*) => {
        $($(#[$attr])* $vis static $name: $crate::runtime::RuntimeLocal<$ty> = $crate::runtime::RuntimeLocal {
            __data: $init
        };)*
    };
}

#[cfg(feature = "integration_test")]
pub struct RuntimeLocal<T: 'static> {
    data: parking_lot::RwLock<
        hashbrown::HashMap<tokio::runtime::Id, &'static T, foldhash::fast::FixedState>,
    >,
    /// Process-wide instance used when `deref` runs on a thread with no current
    /// tokio runtime — see the comment in `deref`.
    fallback: std::sync::OnceLock<&'static T>,
    init: fn() -> T,
}

#[cfg(feature = "integration_test")]
impl<T> RuntimeLocal<T> {
    /// inner API used in the macro, not part of public API
    #[doc(hidden)]
    pub const fn __new(init: fn() -> T) -> Self {
        Self {
            data: parking_lot::RwLock::new(hashbrown::HashMap::with_hasher(
                foldhash::fast::FixedState::with_seed(12345678910),
            )),
            fallback: std::sync::OnceLock::new(),
            init,
        }
    }
}

#[cfg(feature = "integration_test")]
impl<T> Deref for RuntimeLocal<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // `Handle::current()` panics when there is no tokio runtime on this
        // thread. That happens during teardown: a nucleo/rayon worker spawned by
        // a fuzzy picker can outlive the per-test runtime and still deref a
        // `runtime_local!` static, and rayon promotes the resulting panic to a
        // process `SIGABRT` — aborting the whole integration-test binary even
        // though every test passed. Fall back to a process-wide instance instead
        // of panicking; the runtime is already gone, so which instance a stray
        // teardown access sees is immaterial.
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return self
                .fallback
                .get_or_init(|| Box::leak(Box::new((self.init)())));
        };
        let id = handle.id();
        let guard = self.data.read();
        match guard.get(&id) {
            Some(res) => res,
            None => {
                drop(guard);
                let data = Box::leak(Box::new((self.init)()));
                let mut guard = self.data.write();
                guard.insert(id, data);
                data
            }
        }
    }
}

#[cfg(feature = "integration_test")]
#[macro_export]
macro_rules! runtime_local {
    ($($(#[$attr:meta])* $vis: vis static $name:ident: $ty: ty = $init: expr;)*) => {
         $($(#[$attr])* $vis static $name: $crate::runtime::RuntimeLocal<$ty> = $crate::runtime::RuntimeLocal::__new(|| $init);)*
    };
}
