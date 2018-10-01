//! Generic connection pooling support for Finchers, based on r2d2.

#![doc(html_root_url = "https://finchers-rs.github.io/docs/finchers-r2d2/v0.1.0")]
#![warn(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    unused,
)]
//#![warn(rust_2018_compatibility)]
#![cfg_attr(feature = "strict", deny(warnings))]
#![cfg_attr(feature = "strict", doc(test(attr(deny(warnings)))))]

extern crate finchers;
extern crate futures;
#[macro_use]
extern crate log;
extern crate r2d2;
extern crate tokio_executor;
extern crate tokio_threadpool;

pub mod current_thread;

#[doc(no_inline)]
pub use r2d2::*;

pub use imp::{pool_endpoint, PoolEndpoint};

mod imp {
    use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint};
    use finchers::error;
    use finchers::error::Error;

    use futures::future::poll_fn;
    use futures::sync::oneshot;
    use futures::{Async, Future, Poll};
    use r2d2::{ManageConnection, Pool, PooledConnection};
    use std::fmt;
    use tokio_executor::DefaultExecutor;
    use tokio_threadpool::blocking;

    /// Create an endpoint which acquires the connection from the specified connection pool.
    ///
    /// This endpoint internally spawns the task for executing the blocking section,
    /// by using the Tokio's default executor.
    pub fn pool_endpoint<M>(pool: Pool<M>) -> PoolEndpoint<M>
    where
        M: ManageConnection,
    {
        PoolEndpoint {
            pool,
            preflight_before_spawn: true,
        }
    }

    /// The endpoint which retrieves a connection from a connection pool.
    #[derive(Debug, Clone)]
    pub struct PoolEndpoint<M: ManageConnection> {
        pool: Pool<M>,
        preflight_before_spawn: bool,
    }

    impl<M> PoolEndpoint<M>
    where
        M: ManageConnection,
    {
        /// Sets whether to call `Pool::try_get()` before spawning the task.
        ///
        /// The default value is `true`.
        pub fn preflight_before_spawn(self, enabled: bool) -> Self {
            Self {
                preflight_before_spawn: enabled,
                ..self
            }
        }

        fn preflight(&self) -> Option<PooledConnection<M>> {
            if self.preflight_before_spawn {
                self.pool.try_get()
            } else {
                None
            }
        }
    }

    impl<'a, M> Endpoint<'a> for PoolEndpoint<M>
    where
        M: ManageConnection + 'a,
    {
        type Output = (PooledConnection<M>,);
        type Future = PoolFuture<'a, M>;

        fn apply(&'a self, _: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
            Ok(PoolFuture {
                endpoint: self,
                handle: None,
            })
        }
    }

    // not a public API.
    pub struct PoolFuture<'a, M: ManageConnection> {
        endpoint: &'a PoolEndpoint<M>,
        handle: Option<oneshot::SpawnHandle<PooledConnection<M>, Error>>,
    }

    impl<'a, M> fmt::Debug for PoolFuture<'a, M>
    where
        M: ManageConnection + fmt::Debug,
        M::Connection: fmt::Debug,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("PoolFuture")
                .field("endpoint", &self.endpoint)
                .field("handle", &self.handle)
                .finish()
        }
    }

    impl<'a, M> Future for PoolFuture<'a, M>
    where
        M: ManageConnection,
    {
        type Item = (PooledConnection<M>,);
        type Error = Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            loop {
                if let Some(ref mut handle) = self.handle {
                    trace!("retrieved the connection from spawned task");
                    return handle.poll().map(|x| x.map(|conn| (conn,)));
                }

                if let Some(conn) = self.endpoint.preflight() {
                    trace!("retrieved the connection without spawning the task");
                    return Ok(Async::Ready((conn,)));
                }

                trace!("spawning the task for executing blocking section");
                let pool = self.endpoint.pool.clone();
                let future = poll_fn(move || match blocking(|| pool.get()) {
                    Ok(Async::NotReady) => Ok(Async::NotReady),
                    Ok(Async::Ready(res)) => res.map(Async::Ready).map_err(error::fail),
                    Err(blocking_err) => Err(error::fail(blocking_err)),
                });

                self.handle = Some(oneshot::spawn(future, &DefaultExecutor::current()));
            }
        }
    }
}
